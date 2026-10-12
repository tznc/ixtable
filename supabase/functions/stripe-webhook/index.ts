// stripe-webhook (no user JWT; the Stripe-Signature HMAC authenticates).
// Verifies the signature (5 minute tolerance) → 400 on failure, records the
// event once in billing_events (event_id unique: a replay returns 200 without
// reapplying), and applies it to `subscriptions` with the rules in
// _shared/billing.ts decideSubscriptionEvent (no regression from late or
// out-of-order events). Access-relevant changes are audited as billing.*.
// Real Stripe and the fake provider's signed events take this same path.
//
// POST <Stripe event> (Stripe-Signature header) → {received, outcome} | {received, duplicate}
import { audit } from "../_shared/audit.ts";
import {
  accessChangeAction,
  billingProvider,
  decideSubscriptionEvent,
  eventAppId,
  eventPlanHint,
  eventSubscriptionId,
  eventCustomerId,
  parseStripeEvent,
  type ResolvedPlan,
  type StripeEvent,
  verifyStripeSignature,
} from "../_shared/billing.ts";
import { getSubscription } from "../_shared/commercial.ts";
import { env, serviceClient } from "../_shared/db.ts";
import { handler, json } from "../_shared/http.ts";
import { incrementMetric } from "../_shared/rateLimit.ts";

const SIGNATURE_TOLERANCE_SECONDS = 300;

type Outcome = { outcome: "applied" | "ignored" | "stale"; reason?: string };

/** Minimal copy of the provider object for billing_events (no customer PII). */
function eventSummary(event: StripeEvent): Record<string, unknown> {
  const object = event.data.object;
  const pick = (key: string) =>
    typeof object[key] === "string" || typeof object[key] === "boolean" ? object[key] : undefined;
  return {
    object: pick("object"),
    id: pick("id"),
    status: pick("status"),
    payment_status: pick("payment_status"),
    cancel_at_period_end: pick("cancel_at_period_end"),
    subscription: eventSubscriptionId(event),
    metadata: object.metadata ?? null,
  };
}

async function resolveAppId(event: StripeEvent): Promise<string | null> {
  const fromMetadata = eventAppId(event);
  if (fromMetadata && /^[0-9a-f-]{36}$/i.test(fromMetadata)) return fromMetadata;
  const subscriptionId = eventSubscriptionId(event);
  if (!subscriptionId) return null;
  const { data } = await serviceClient()
    .from("subscriptions")
    .select("app_id")
    .eq("stripe_subscription_id", subscriptionId)
    .maybeSingle();
  return data?.app_id ?? null;
}

/**
 * Plan and interval of the event. The price wins over metadata because
 * portal plan or interval switches change the price, not the metadata.
 */
async function resolvePlan(event: StripeEvent): Promise<ResolvedPlan | null> {
  const { planId, interval, priceId } = eventPlanHint(event);
  const db = serviceClient();
  if (priceId) {
    for (const [column, interval] of [
      ["stripe_price_id", "month"],
      ["stripe_annual_price_id", "year"],
    ] as const) {
      const { data } = await db.from("plans").select("id").eq(column, priceId).maybeSingle();
      if (data) return { planId: data.id, interval };
    }
  }
  if (planId) {
    const { data } = await db.from("plans").select("id").eq("id", planId).maybeSingle();
    if (data) return { planId: data.id, interval: interval ?? "month" };
  }
  return null;
}

/** Remembers the org's billing customer from a purchase (customers created before org billing). */
async function rememberOrgCustomer(event: StripeEvent, orgId: string, provider: string) {
  const customerId = eventCustomerId(event);
  if (!customerId || event.type !== "checkout.session.completed") return;
  const { error } = await serviceClient()
    .from("org_billing_customers")
    .upsert(
      { org_id: orgId, provider, stripe_customer_id: customerId },
      { onConflict: "org_id", ignoreDuplicates: true },
    );
  if (error) console.error("remember org customer failed", error.message);
}

async function apply(event: StripeEvent, appId: string | null): Promise<Outcome> {
  if (!appId) return { outcome: "ignored", reason: "no_app" };
  const db = serviceClient();
  const { data: app } = await db
    .from("cloud_apps")
    .select("id, org_id")
    .eq("id", appId)
    .maybeSingle();
  if (!app) return { outcome: "ignored", reason: "app_not_found" };

  const existing = await getSubscription(appId);
  const decision = decideSubscriptionEvent(event, existing, await resolvePlan(event));
  if (decision.kind !== "apply") {
    return { outcome: decision.kind === "stale" ? "stale" : "ignored", reason: decision.reason };
  }
  const next = decision.next;
  const provider = billingProvider();
  const row = { app_id: appId, provider: provider.name, ...next };
  // Compare-and-set on provider_event_at so two concurrent deliveries cannot
  // regress the row.
  if (existing) {
    let update = db.from("subscriptions").update(row).eq("app_id", appId);
    update = existing.provider_event_at
      ? update.eq("provider_event_at", existing.provider_event_at)
      : update.is("provider_event_at", null);
    const { data, error } = await update.select("id");
    if (error) throw new Error(`update subscription failed: ${error.message}`);
    if (!data || data.length === 0) throw new Error("subscription changed concurrently; retry");
  } else {
    const { error } = await db.from("subscriptions").insert(row);
    if (error) throw new Error(`insert subscription failed: ${error.message}`);
  }

  await rememberOrgCustomer(event, app.org_id, provider.name);

  if (decision.replacesSubscriptionId) {
    // A new checkout replaced the previous subscription: stop billing the old one.
    try {
      await provider.cancelSubscription({
        subscriptionId: decision.replacesSubscriptionId,
        atPeriodEnd: false,
      });
    } catch (err) {
      console.error(
        "cancel replaced subscription failed",
        err instanceof Error ? err.message : err,
      );
      await incrementMetric("billing.replace_cancel_failed");
    }
  }

  const action = accessChangeAction(existing, next);
  if (action) {
    const { data: entitlement } = await db.rpc("app_entitlement", { p_app_id: appId });
    await audit({
      action,
      orgId: app.org_id,
      appId,
      target: next.stripe_subscription_id ? `subscription:${next.stripe_subscription_id}` : null,
      details: {
        eventId: event.id,
        eventType: event.type,
        from: existing
          ? {
              status: existing.status,
              planId: existing.plan_id,
              interval: existing.billing_interval,
            }
          : null,
        to: {
          status: next.status,
          planId: next.plan_id,
          interval: next.billing_interval,
          cancelAtPeriodEnd: next.cancel_at_period_end,
        },
        entitlement: entitlement
          ? {
              allowed: entitlement.allowed,
              reason: entitlement.reason,
              allowance: entitlement.allowance,
              used: entitlement.used,
            }
          : null,
      },
    });
  }
  return { outcome: "applied" };
}

Deno.serve(
  handler(async (req) => {
    const payload = await req.text();
    const valid = await verifyStripeSignature(
      payload,
      req.headers.get("stripe-signature"),
      env("STRIPE_WEBHOOK_SECRET"),
      SIGNATURE_TOLERANCE_SECONDS,
    );
    const event = valid ? parseStripeEvent(payload) : null;
    if (!event) {
      await incrementMetric(
        valid ? "billing.webhook.malformed" : "billing.webhook.signature_failed",
      );
      return json(
        req,
        { error: { code: "INVALID_SIGNATURE", message: "Invalid webhook signature or payload" } },
        400,
      );
    }
    await incrementMetric("billing.webhook.received");

    const db = serviceClient();
    const appId = await resolveAppId(event);
    const { error: insertError } = await db.from("billing_events").insert({
      event_id: event.id,
      provider: "stripe",
      type: event.type,
      app_id: appId,
      payload: eventSummary(event),
    });
    if (insertError) {
      if (insertError.code !== "23505")
        throw new Error(`record event failed: ${insertError.message}`);
      const { data: seen } = await db
        .from("billing_events")
        .select("processed_at")
        .eq("event_id", event.id)
        .maybeSingle();
      if (seen?.processed_at) {
        await incrementMetric("billing.webhook.duplicate");
        return { received: true, duplicate: true };
      }
      // A previous delivery failed before finishing: process it again.
    }

    try {
      const result = await apply(event, appId);
      await db
        .from("billing_events")
        .update({
          processed_at: new Date().toISOString(),
          outcome: result.outcome,
          outcome_reason: result.reason ?? null,
          error: null,
          app_id: appId,
        })
        .eq("event_id", event.id);
      await incrementMetric(`billing.webhook.${result.outcome}`);
      return { received: true, outcome: result.outcome };
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      await db
        .from("billing_events")
        .update({ outcome: "failed", error: message.slice(0, 500) })
        .eq("event_id", event.id);
      await incrementMetric("billing.webhook.failed");
      // 500 makes Stripe retry the delivery later.
      throw err;
    }
  }),
);
