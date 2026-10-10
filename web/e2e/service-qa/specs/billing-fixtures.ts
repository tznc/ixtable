/**
 * Helpers for the billing, account, admin and rate-limit specs. Webhook
 * events are Stripe-shaped and signed with the local STRIPE_WEBHOOK_SECRET
 * exactly as Stripe signs them (`t=<unix>,v1=HMAC-SHA256(secret, "<t>.<body>")`),
 * so stripe-webhook runs its real verification. Billing is the only outbound
 * provider, and the only thing faked (BILLING_PROVIDER=fake).
 */
import { createHmac, randomBytes } from "node:crypto";
import { callFunction, devSecret, getServiceClient } from "../clients";
import type { TestUser } from "../seed";

export type EventType =
  | "checkout.session.completed"
  | "customer.subscription.created"
  | "customer.subscription.updated"
  | "customer.subscription.deleted"
  | "invoice.paid"
  | "invoice.payment_failed";

export interface StripeEventInput {
  type: EventType;
  appId: string;
  planId: string;
  subscriptionId: string;
  customerId?: string;
  status?: string;
  /** event.created, unix seconds (default now). */
  created?: number;
  currentPeriodEnd?: number;
  cancelAtPeriodEnd?: boolean;
  id?: string;
}

export const nowSeconds = () => Math.floor(Date.now() / 1000);

/** A Stripe-shaped event, mirroring supabase/functions/_shared/billing.ts buildFakeEvent. */
export function stripeEvent(input: StripeEventInput): Record<string, unknown> {
  const metadata = { app_id: input.appId, plan_id: input.planId };
  const customer = input.customerId ?? `cus_qa_${input.appId.slice(0, 8)}`;
  const periodEnd = input.currentPeriodEnd ?? nowSeconds() + 30 * 86_400;
  const object =
    input.type === "checkout.session.completed"
      ? {
          object: "checkout.session",
          id: `cs_qa_${randomBytes(6).toString("hex")}`,
          mode: "subscription",
          client_reference_id: input.appId,
          customer,
          subscription: input.subscriptionId,
          payment_status: "paid",
          metadata,
        }
      : input.type.startsWith("invoice.")
        ? {
            object: "invoice",
            id: `in_qa_${randomBytes(6).toString("hex")}`,
            customer,
            subscription: input.subscriptionId,
            metadata,
            lines: { data: [{ period: { end: periodEnd } }] },
          }
        : {
            object: "subscription",
            id: input.subscriptionId,
            customer,
            status:
              input.status ??
              (input.type === "customer.subscription.deleted" ? "canceled" : "active"),
            current_period_end: periodEnd,
            cancel_at_period_end: input.cancelAtPeriodEnd ?? false,
            metadata,
            items: { data: [{ price: { id: `price_fake_${input.planId}` } }] },
          };
  return {
    id: input.id ?? `evt_qa_${randomBytes(8).toString("hex")}`,
    object: "event",
    type: input.type,
    created: input.created ?? nowSeconds(),
    livemode: false,
    data: { object },
  };
}

export function signPayload(payload: string, secret: string, timestamp = nowSeconds()): string {
  const v1 = createHmac("sha256", secret).update(`${timestamp}.${payload}`).digest("hex");
  return `t=${timestamp},v1=${v1}`;
}

/** Posts a signed event to stripe-webhook (no user JWT, like Stripe). */
export async function postWebhook(
  event: Record<string, unknown>,
  opts: { secret?: string; timestamp?: number; tamper?: (payload: string) => string } = {},
) {
  const payload = JSON.stringify(event);
  const signature = signPayload(
    payload,
    opts.secret ?? devSecret("STRIPE_WEBHOOK_SECRET"),
    opts.timestamp,
  );
  return callFunction<{
    received?: boolean;
    duplicate?: boolean;
    outcome?: string;
    error?: { code: string; message: string };
  }>("stripe-webhook", {
    body: opts.tamper ? opts.tamper(payload) : payload,
    headers: { "Stripe-Signature": signature },
  });
}

/** billing-checkout, then the fake checkout page's billing-fake-complete call. */
export async function checkoutAndPay(
  user: TestUser,
  appId: string,
  planId: string,
  interval?: "month" | "year",
) {
  const checkout = await callFunction<{ url: string; overAllowance: boolean }>("billing-checkout", {
    jwt: user.jwt,
    body: interval ? { appId, planId, interval } : { appId, planId },
  });
  if (checkout.status !== 200)
    throw new Error(`billing-checkout ${checkout.status}: ${JSON.stringify(checkout.body)}`);
  const url = new URL(checkout.body.url);
  const complete = await callFunction<{ ok: boolean; status: string }>("billing-fake-complete", {
    jwt: user.jwt,
    body: { sessionId: url.searchParams.get("session"), appId, planId },
  });
  return { checkout, complete, sessionId: url.searchParams.get("session") ?? "" };
}

export interface SubscriptionRow {
  plan_id: string;
  billing_interval: "month" | "year";
  status: string;
  provider: string;
  current_period_end: string | null;
  cancel_at_period_end: boolean;
  stripe_customer_id: string | null;
  stripe_subscription_id: string | null;
  provider_event_at: string | null;
}

export async function subscriptionRow(appId: string): Promise<SubscriptionRow | null> {
  const { data, error } = await getServiceClient()
    .from("subscriptions")
    .select(
      "plan_id, billing_interval, status, provider, current_period_end, cancel_at_period_end, stripe_customer_id, stripe_subscription_id, provider_event_at",
    )
    .eq("app_id", appId)
    .maybeSingle();
  if (error) throw new Error(`subscription query failed: ${error.message}`);
  return data as SubscriptionRow | null;
}

/** app_entitlement as the given user sees it (PostgREST + RLS). */
export async function entitlementAs(user: TestUser, appId: string) {
  const { data, error } = await user.client.rpc("app_entitlement", { p_app_id: appId });
  if (error) throw new Error(`app_entitlement failed: ${error.message}`);
  return data as {
    allowed: boolean;
    reason: string;
    allowance: number;
    used: number;
    status?: string;
    planId?: string;
    interval?: "month" | "year";
  };
}

export async function billingEventRows(eventId: string) {
  const { data, error } = await getServiceClient()
    .from("billing_events")
    .select("event_id, type, outcome, outcome_reason, processed_at")
    .eq("event_id", eventId);
  if (error) throw new Error(`billing_events query failed: ${error.message}`);
  return data ?? [];
}

export async function auditActions(filter: { appId?: string; actorId?: string; target?: string }) {
  let query = getServiceClient().from("audit_events").select("action, actor_id, target, details");
  if (filter.appId) query = query.eq("app_id", filter.appId);
  if (filter.actorId) query = query.eq("actor_id", filter.actorId);
  if (filter.target) query = query.eq("target", filter.target);
  const { data, error } = await query.order("at", { ascending: true });
  if (error) throw new Error(`audit query failed: ${error.message}`);
  return data ?? [];
}
