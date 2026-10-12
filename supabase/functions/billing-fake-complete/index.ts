// billing-fake-complete {sessionId, appId, planId} → {ok, status}
// Local/QA only (BILLING_PROVIDER=fake): the website's test checkout page
// calls this to "pay". It posts Stripe-shaped events signed with
// STRIPE_WEBHOOK_SECRET to stripe-webhook, the same path real Stripe uses,
// so the subscription is written only by the webhook. 404 with any other
// provider, and unless IXTABLE_ALLOW_FAKE_BILLING=1.
import { buildFakeEvent, fakeBillingAllowed } from "../_shared/billing.ts";
import { getSubscription, orgBillingCustomerId, postSignedEvent } from "../_shared/commercial.ts";
import { serviceClient } from "../_shared/db.ts";
import { handler, HttpError, readJson, requireUser } from "../_shared/http.ts";
import { enforceNamedRateLimit } from "../_shared/rateLimit.ts";
import { randomToken } from "../_shared/crypto.ts";
import { str, uuid } from "../_shared/validate.ts";

Deno.serve(
  handler(async (req) => {
    if (!fakeBillingAllowed() || Deno.env.get("BILLING_PROVIDER") !== "fake")
      throw new HttpError("NOT_FOUND", "Not available");
    const { user } = await requireUser(req);
    const body = await readJson(req);
    const sessionId = str(body, "sessionId", { min: 8, max: 255 });
    const appId = uuid(body, "appId");
    const planId = str(body, "planId", { min: 1, max: 64 });
    await enforceNamedRateLimit("billing-fake-complete", user.id);

    const db = serviceClient();
    const { data: session } = await db
      .from("billing_checkout_sessions")
      .select("id, app_id, plan_id, billing_interval, user_id, provider, status, expires_at")
      .eq("id", sessionId)
      .maybeSingle();
    if (
      !session ||
      session.user_id !== user.id ||
      session.provider !== "fake" ||
      session.app_id !== appId ||
      session.plan_id !== planId
    ) {
      throw new HttpError("NOT_FOUND", "Checkout session not found");
    }
    if (session.status !== "pending" || Date.parse(session.expires_at) < Date.now()) {
      throw new HttpError("VALIDATION", "sessionId: checkout session is no longer open", {
        field: "sessionId",
      });
    }
    const { data: claimed } = await db
      .from("billing_checkout_sessions")
      .update({ status: "completed", completed_at: new Date().toISOString() })
      .eq("id", sessionId)
      .eq("status", "pending")
      .select("id");
    if (!claimed || claimed.length === 0) {
      throw new HttpError("VALIDATION", "sessionId: checkout session is no longer open", {
        field: "sessionId",
      });
    }

    const { data: app } = await db.from("cloud_apps").select("org_id").eq("id", appId).single();
    const existing = await getSubscription(appId);
    // billing-checkout created the org's customer; older rows fall back.
    const customerId =
      (app ? await orgBillingCustomerId(app.org_id) : null) ??
      existing?.stripe_customer_id ??
      `cus_fake_${randomToken(9)}`;
    const interval = session.billing_interval as "month" | "year";
    // Every checkout creates a new subscription, as Stripe does.
    const subscriptionId = `sub_fake_${randomToken(9)}`;
    for (const type of ["checkout.session.completed", "customer.subscription.created"] as const) {
      const result = await postSignedEvent(
        buildFakeEvent({ type, appId, planId, interval, customerId, subscriptionId }),
      );
      if (result.status !== 200) {
        throw new Error(`stripe-webhook rejected the fake ${type} event (${result.status})`);
      }
    }
    const subscription = await getSubscription(appId);
    return { ok: true, status: subscription?.status ?? null };
  }),
);
