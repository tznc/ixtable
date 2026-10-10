// Billing provider abstraction (PRD §4.2, Phase 5).
//
// BILLING_PROVIDER selects the implementation:
// - "stripe": real Stripe REST API via fetch (STRIPE_SECRET_KEY).
// - "fake": local/QA. Checkout returns a website URL; the flow completes when
//   a Stripe-shaped event signed with STRIPE_WEBHOOK_SECRET is posted to
//   stripe-webhook (see buildFakeEvent + signStripePayload).
// stripe-webhook verifies both the same way (verifyStripeSignature).
import { hmacSha256Hex, randomToken, timingSafeEqual } from "./crypto.ts";

export type BillingInterval = "month" | "year";
export const BILLING_INTERVALS: readonly BillingInterval[] = ["month", "year"];

export interface CheckoutInput {
  appId: string;
  orgId: string;
  planId: string;
  interval: BillingInterval;
  /** plans.stripe_price_id (month) or plans.stripe_annual_price_id (year) */
  priceId: string;
  userId: string;
  /** The organization's billing customer (org_billing_customers). */
  customerId: string;
  successUrl: string;
  cancelUrl: string;
}

export interface Invoice {
  id: string;
  number: string | null;
  status: string | null;
  amountDue: number;
  amountPaid: number;
  currency: string;
  created: string;
  hostedInvoiceUrl: string | null;
  pdfUrl: string | null;
}

export interface BillingProvider {
  readonly name: "stripe" | "fake";
  /** Creates the organization's billing customer (PRD §4.5: the org pays). */
  createCustomer(input: { orgId: string; orgName: string; email: string }): Promise<{
    customerId: string;
  }>;
  createCheckout(input: CheckoutInput): Promise<{ url: string; sessionId: string }>;
  createPortal(input: {
    customerId: string;
    appId: string;
    returnUrl: string;
  }): Promise<{ url: string }>;
  listInvoices(input: { customerId: string }): Promise<Invoice[]>;
  cancelSubscription(input: { subscriptionId: string; atPeriodEnd: boolean }): Promise<void>;
}

/** True only where the fake provider was explicitly allowed (local/QA stacks). */
export function fakeBillingAllowed(): boolean {
  return Deno.env.get("IXTABLE_ALLOW_FAKE_BILLING") === "1";
}

/**
 * The provider named by BILLING_PROVIDER (required; "stripe" or "fake").
 * "fake" also needs IXTABLE_ALLOW_FAKE_BILLING=1, so a production project
 * left on the default cannot hand out free subscriptions.
 */
export function billingProvider(): BillingProvider {
  const name = Deno.env.get("BILLING_PROVIDER");
  if (name === "fake") {
    if (!fakeBillingAllowed())
      throw new Error("BILLING_PROVIDER=fake needs IXTABLE_ALLOW_FAKE_BILLING=1 (local/QA only)");
    return fakeProvider(Deno.env.get("SITE_URL") ?? "http://127.0.0.1:3001");
  }
  if (name === "stripe") {
    const key = Deno.env.get("STRIPE_SECRET_KEY");
    if (!key) throw new Error("Missing required environment variable STRIPE_SECRET_KEY");
    return stripeProvider(key);
  }
  throw new Error("BILLING_PROVIDER must be 'stripe' or 'fake'");
}

// Stripe signatures ------------------------------------------------------------

/** Parses `t=<unix>,v1=<hex>[,v1=<hex>]`. */
export function parseStripeSignatureHeader(header: string): {
  timestamp: number;
  signatures: string[];
} {
  let timestamp = Number.NaN;
  const signatures: string[] = [];
  for (const part of header.split(",")) {
    const [key, value] = part.split("=", 2).map((item) => item?.trim());
    if (key === "t") timestamp = Number(value);
    if (key === "v1" && value) signatures.push(value);
  }
  return { timestamp, signatures };
}

/** Builds a `Stripe-Signature` header value for `payload` (used by the fake provider and tests). */
export async function signStripePayload(
  payload: string,
  secret: string,
  timestamp = Math.floor(Date.now() / 1000),
): Promise<string> {
  return `t=${timestamp},v1=${await hmacSha256Hex(secret, `${timestamp}.${payload}`)}`;
}

/**
 * Verifies a Stripe-Signature header against the raw request body with the
 * webhook secret, rejecting timestamps outside `toleranceSeconds`.
 */
export async function verifyStripeSignature(
  payload: string,
  header: string | null,
  secret: string,
  toleranceSeconds = 300,
  nowSeconds = Math.floor(Date.now() / 1000),
): Promise<boolean> {
  if (!header || !secret) return false;
  const { timestamp, signatures } = parseStripeSignatureHeader(header);
  if (!Number.isFinite(timestamp) || signatures.length === 0) return false;
  if (Math.abs(nowSeconds - timestamp) > toleranceSeconds) return false;
  const expected = await hmacSha256Hex(secret, `${timestamp}.${payload}`);
  return signatures.some((signature) => timingSafeEqual(signature, expected));
}

// Fake provider ----------------------------------------------------------------

export interface FakeEventInput {
  type:
    | "checkout.session.completed"
    | "customer.subscription.created"
    | "customer.subscription.updated"
    | "customer.subscription.deleted"
    | "invoice.paid"
    | "invoice.payment_failed";
  appId: string;
  planId: string;
  /** Defaults to "month". */
  interval?: BillingInterval;
  status?: string;
  customerId?: string;
  subscriptionId?: string;
  currentPeriodEnd?: number;
  cancelAtPeriodEnd?: boolean;
  eventId?: string;
  /** `event.created` (unix seconds); defaults to now. */
  created?: number;
  /** checkout.session.completed only; defaults to "paid". */
  paymentStatus?: "paid" | "unpaid" | "no_payment_required";
}

/**
 * A Stripe-shaped event for the fake provider. Objects carry
 * `metadata.app_id` / `metadata.plan_id`, as real checkouts created by
 * stripeProvider do.
 */
export function buildFakeEvent(input: FakeEventInput): Record<string, unknown> {
  const now = Math.floor(Date.now() / 1000);
  const interval = input.interval ?? "month";
  const metadata = { app_id: input.appId, plan_id: input.planId, billing_interval: interval };
  const customer = input.customerId ?? `cus_fake_${input.appId.slice(0, 8)}`;
  const subscription = input.subscriptionId ?? `sub_fake_${input.appId.slice(0, 8)}`;
  const periodEnd = input.currentPeriodEnd ?? now + (interval === "year" ? 365 : 30) * 86_400;
  const object =
    input.type === "checkout.session.completed"
      ? {
          object: "checkout.session",
          id: `cs_fake_${randomToken(8)}`,
          mode: "subscription",
          client_reference_id: input.appId,
          customer,
          subscription,
          payment_status: input.paymentStatus ?? "paid",
          metadata,
        }
      : input.type === "invoice.payment_failed" || input.type === "invoice.paid"
        ? {
            object: "invoice",
            id: `in_fake_${randomToken(8)}`,
            customer,
            subscription,
            status: input.type === "invoice.paid" ? "paid" : "open",
            metadata,
            lines: { data: [{ period: { end: periodEnd } }] },
          }
        : {
            object: "subscription",
            id: subscription,
            customer,
            status:
              input.status ??
              (input.type === "customer.subscription.deleted" ? "canceled" : "active"),
            current_period_end: periodEnd,
            cancel_at_period_end: input.cancelAtPeriodEnd ?? false,
            metadata,
            items: { data: [{ price: { id: fakePriceId(input.planId, interval) } }] },
          };
  return {
    id: input.eventId ?? `evt_fake_${randomToken(12)}`,
    object: "event",
    type: input.type,
    created: input.created ?? now,
    livemode: false,
    data: { object },
  };
}

/** The price ids seed.sql gives local plans. */
export function fakePriceId(planId: string, interval: BillingInterval): string {
  return interval === "year" ? `price_fake_${planId}_annual` : `price_fake_${planId}`;
}

export function fakeProvider(siteUrl: string): BillingProvider {
  return {
    name: "fake",
    createCustomer() {
      return Promise.resolve({ customerId: `cus_fake_${randomToken(9)}` });
    },
    createCheckout(input) {
      const sessionId = `cs_fake_${randomToken(12)}`;
      const url = new URL("/cloud/billing/fake-checkout", siteUrl);
      url.searchParams.set("session", sessionId);
      url.searchParams.set("app", input.appId);
      url.searchParams.set("plan", input.planId);
      url.searchParams.set("interval", input.interval);
      return Promise.resolve({ url: url.toString(), sessionId });
    },
    createPortal(input) {
      const url = new URL("/cloud/billing/fake-portal", siteUrl);
      url.searchParams.set("app", input.appId);
      return Promise.resolve({ url: url.toString() });
    },
    listInvoices(input) {
      return Promise.resolve([
        {
          id: `in_fake_${input.customerId}`,
          number: "FAKE-0001",
          status: "paid",
          amountDue: 0,
          amountPaid: 0,
          currency: "usd",
          created: new Date().toISOString(),
          hostedInvoiceUrl: null,
          pdfUrl: null,
        },
      ]);
    },
    cancelSubscription() {
      return Promise.resolve();
    },
  };
}

// Stripe provider ----------------------------------------------------------------

function form(
  fields: Record<string, string | number | boolean | undefined | null>,
): URLSearchParams {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(fields)) {
    if (value !== undefined && value !== null) params.append(key, String(value));
  }
  return params;
}

export function stripeProvider(
  secretKey: string,
  apiBase = "https://api.stripe.com",
): BillingProvider {
  async function call<T>(method: string, path: string, body?: URLSearchParams): Promise<T> {
    const response = await fetch(`${apiBase}${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${secretKey}`,
        "Content-Type": "application/x-www-form-urlencoded",
        "Stripe-Version": "2024-06-20",
      },
      body,
    });
    const data = await response.json().catch(() => ({}));
    if (!response.ok) {
      const message =
        (data as { error?: { message?: string } }).error?.message ?? response.statusText;
      throw new Error(`Stripe ${method} ${path} failed: ${message}`);
    }
    return data as T;
  }

  return {
    name: "stripe",
    async createCustomer(input) {
      const customer = await call<{ id: string }>(
        "POST",
        "/v1/customers",
        form({ name: input.orgName, email: input.email, "metadata[org_id]": input.orgId }),
      );
      return { customerId: customer.id };
    },
    async createCheckout(input) {
      // No trial (PRD §4.5): no trial_period_days, and payment is collected up front.
      const session = await call<{ id: string; url: string }>(
        "POST",
        "/v1/checkout/sessions",
        form({
          mode: "subscription",
          "line_items[0][price]": input.priceId,
          "line_items[0][quantity]": 1,
          success_url: input.successUrl,
          cancel_url: input.cancelUrl,
          client_reference_id: input.appId,
          customer: input.customerId,
          payment_method_collection: "always",
          "metadata[app_id]": input.appId,
          "metadata[org_id]": input.orgId,
          "metadata[plan_id]": input.planId,
          "metadata[billing_interval]": input.interval,
          "metadata[user_id]": input.userId,
          "subscription_data[metadata][app_id]": input.appId,
          "subscription_data[metadata][org_id]": input.orgId,
          "subscription_data[metadata][plan_id]": input.planId,
          "subscription_data[metadata][billing_interval]": input.interval,
        }),
      );
      return { url: session.url, sessionId: session.id };
    },
    async createPortal(input) {
      const session = await call<{ url: string }>(
        "POST",
        "/v1/billing_portal/sessions",
        form({ customer: input.customerId, return_url: input.returnUrl }),
      );
      return { url: session.url };
    },
    async listInvoices(input) {
      const list = await call<{ data: Record<string, unknown>[] }>(
        "GET",
        `/v1/invoices?customer=${encodeURIComponent(input.customerId)}&limit=24`,
      );
      return list.data.map((invoice) => ({
        id: String(invoice.id),
        number: (invoice.number as string | null) ?? null,
        status: (invoice.status as string | null) ?? null,
        amountDue: Number(invoice.amount_due ?? 0),
        amountPaid: Number(invoice.amount_paid ?? 0),
        currency: String(invoice.currency ?? "usd"),
        created: new Date(Number(invoice.created ?? 0) * 1000).toISOString(),
        hostedInvoiceUrl: (invoice.hosted_invoice_url as string | null) ?? null,
        pdfUrl: (invoice.invoice_pdf as string | null) ?? null,
      }));
    },
    async cancelSubscription(input) {
      if (input.atPeriodEnd) {
        await call(
          "POST",
          `/v1/subscriptions/${encodeURIComponent(input.subscriptionId)}`,
          form({ cancel_at_period_end: true }),
        );
      } else {
        await call("DELETE", `/v1/subscriptions/${encodeURIComponent(input.subscriptionId)}`);
      }
    },
  };
}

// Webhook event reduction --------------------------------------------------------
//
// Pure decision logic for stripe-webhook (tested in billing_events_test.ts).
// Rules:
// - Events older than the newest applied one (`provider_event_at`) are stale.
// - Events about a subscription other than the app's current one are ignored,
//   except a new purchase (checkout completed / subscription created), which
//   replaces the old subscription (the caller cancels the old one).
// - invoice.payment_failed moves active/trialing to past_due (grace in
//   app_entitlement); invoice.paid recovers past_due/unpaid/incomplete.

export const SUBSCRIPTION_STATUSES = [
  "trialing",
  "active",
  "past_due",
  "canceled",
  "incomplete",
  "incomplete_expired",
  "unpaid",
  "paused",
] as const;
export type SubscriptionStatus = (typeof SUBSCRIPTION_STATUSES)[number];

/** Statuses app_entitlement treats as paid (past_due within the grace window). */
export const ENTITLED_STATUSES: readonly string[] = ["active", "trialing", "past_due"];
/** Statuses that no longer bill and need no cancellation. */
export const ENDED_STATUSES: readonly string[] = ["canceled", "incomplete_expired"];

export const HANDLED_EVENT_TYPES = [
  "checkout.session.completed",
  "customer.subscription.created",
  "customer.subscription.updated",
  "customer.subscription.deleted",
  "invoice.paid",
  "invoice.payment_failed",
] as const;

export interface StripeEvent {
  id: string;
  type: string;
  created: number;
  data: { object: Record<string, unknown> };
}

/** The subscriptions columns the webhook reads and writes. */
export interface SubscriptionState {
  plan_id: string;
  billing_interval: BillingInterval;
  status: string;
  current_period_end: string | null;
  cancel_at_period_end: boolean;
  stripe_customer_id: string | null;
  stripe_subscription_id: string | null;
  provider_event_at: string | null;
}

export type EventDecision =
  | { kind: "ignore"; reason: string }
  | { kind: "stale"; reason: string }
  | { kind: "apply"; next: SubscriptionState; replacesSubscriptionId?: string };

/** Parses and shape-checks a webhook body. Returns null when it is not a Stripe event. */
export function parseStripeEvent(raw: string): StripeEvent | null {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  const event = value as Partial<StripeEvent> | null;
  if (
    !event ||
    typeof event !== "object" ||
    typeof event.id !== "string" ||
    event.id.length === 0 ||
    event.id.length > 255 ||
    typeof event.type !== "string" ||
    typeof event.created !== "number" ||
    !event.data ||
    typeof event.data.object !== "object" ||
    event.data.object === null
  ) {
    return null;
  }
  return event as StripeEvent;
}

function text(value: unknown): string | null {
  if (typeof value === "string" && value.length > 0) return value;
  // Expanded Stripe objects carry the id inside.
  if (value && typeof value === "object" && typeof (value as { id?: unknown }).id === "string") {
    return (value as { id: string }).id;
  }
  return null;
}

function metadataOf(object: Record<string, unknown>): Record<string, unknown> {
  const metadata = object.metadata;
  return metadata && typeof metadata === "object" ? (metadata as Record<string, unknown>) : {};
}

/** `metadata.app_id`, then `client_reference_id`. */
export function eventAppId(event: StripeEvent): string | null {
  const object = event.data.object;
  return text(metadataOf(object).app_id) ?? text(object.client_reference_id);
}

/** The Stripe subscription id the event is about. */
export function eventSubscriptionId(event: StripeEvent): string | null {
  const object = event.data.object;
  return object.object === "subscription" ? text(object.id) : text(object.subscription);
}

export function eventCustomerId(event: StripeEvent): string | null {
  return text(event.data.object.customer);
}

/** `metadata.plan_id`, `metadata.billing_interval` and the first price id, for resolving the plan. */
export function eventPlanHint(event: StripeEvent): {
  planId: string | null;
  interval: BillingInterval | null;
  priceId: string | null;
} {
  const object = event.data.object;
  const items = (object.items as { data?: { price?: { id?: unknown } }[] } | undefined)?.data;
  const interval = text(metadataOf(object).billing_interval);
  return {
    planId: text(metadataOf(object).plan_id),
    interval: (BILLING_INTERVALS as readonly (string | null)[]).includes(interval)
      ? (interval as BillingInterval)
      : null,
    priceId: text(items?.[0]?.price?.id),
  };
}

/** A plan and interval resolved from an event (price first, then metadata). */
export interface ResolvedPlan {
  planId: string;
  interval: BillingInterval;
}

export function mapStripeStatus(status: unknown): SubscriptionStatus {
  return (SUBSCRIPTION_STATUSES as readonly unknown[]).includes(status)
    ? (status as SubscriptionStatus)
    : "incomplete";
}

function unixToIso(value: unknown): string | null {
  const seconds = Number(value);
  return Number.isFinite(seconds) && seconds > 0 ? new Date(seconds * 1000).toISOString() : null;
}

function periodEnd(object: Record<string, unknown>): string | null {
  if (object.current_period_end !== undefined) return unixToIso(object.current_period_end);
  // Newer Stripe API versions keep the period on the items, invoices on lines.
  const item = (object.items as { data?: { current_period_end?: unknown }[] } | undefined)
    ?.data?.[0];
  if (item?.current_period_end !== undefined) return unixToIso(item.current_period_end);
  const line = (object.lines as { data?: { period?: { end?: unknown } }[] } | undefined)?.data?.[0];
  return unixToIso(line?.period?.end);
}

function later(a: string | null, b: string | null): string | null {
  if (!a) return b;
  if (!b) return a;
  return Date.parse(a) >= Date.parse(b) ? a : b;
}

/**
 * Decides what a webhook event does to the app's subscription row.
 * `plan` is the plan and interval resolved from the event (price or
 * metadata), if any; a bare plan id keeps the current interval.
 */
export function decideSubscriptionEvent(
  event: StripeEvent,
  existing: SubscriptionState | null,
  plan: ResolvedPlan | string | null,
): EventDecision {
  const planId = typeof plan === "string" ? plan : (plan?.planId ?? null);
  const interval = typeof plan === "string" ? null : (plan?.interval ?? null);
  if (!(HANDLED_EVENT_TYPES as readonly string[]).includes(event.type)) {
    return { kind: "ignore", reason: "unhandled_type" };
  }
  const object = event.data.object;
  const createdAt = new Date(event.created * 1000).toISOString();
  const subscriptionId = eventSubscriptionId(event);
  const isPurchase =
    event.type === "checkout.session.completed" || event.type === "customer.subscription.created";
  if (event.type === "checkout.session.completed" && object.mode !== "subscription") {
    return { kind: "ignore", reason: "not_a_subscription_checkout" };
  }

  let replacesSubscriptionId: string | undefined;
  const current =
    existing?.stripe_subscription_id &&
    subscriptionId &&
    subscriptionId !== existing.stripe_subscription_id
      ? null
      : existing;
  if (existing && current === null) {
    if (!isPurchase) return { kind: "ignore", reason: "other_subscription" };
    if (!ENDED_STATUSES.includes(existing.status)) {
      replacesSubscriptionId = existing.stripe_subscription_id ?? undefined;
    }
  }
  if (
    existing?.provider_event_at &&
    Date.parse(createdAt) < Date.parse(existing.provider_event_at)
  ) {
    return { kind: "stale", reason: "older_than_applied_event" };
  }

  const base: SubscriptionState = {
    plan_id: planId ?? current?.plan_id ?? existing?.plan_id ?? "",
    billing_interval:
      interval ?? current?.billing_interval ?? existing?.billing_interval ?? "month",
    status: current?.status ?? "incomplete",
    current_period_end: current?.current_period_end ?? null,
    cancel_at_period_end: current?.cancel_at_period_end ?? false,
    stripe_customer_id: eventCustomerId(event) ?? current?.stripe_customer_id ?? null,
    stripe_subscription_id: subscriptionId ?? current?.stripe_subscription_id ?? null,
    provider_event_at: later(existing?.provider_event_at ?? null, createdAt),
  };
  if (!base.plan_id) return { kind: "ignore", reason: "unknown_plan" };

  switch (event.type) {
    case "checkout.session.completed": {
      const paid = ["paid", "no_payment_required", undefined].includes(
        object.payment_status as string | undefined,
      );
      // A subscription event for the same subscription may have arrived first.
      if (!current || current.status === "incomplete") {
        base.status = paid ? "active" : "incomplete";
      }
      if (!current) base.cancel_at_period_end = false;
      break;
    }
    case "customer.subscription.created":
    case "customer.subscription.updated":
      base.status = mapStripeStatus(object.status);
      base.current_period_end = periodEnd(object) ?? base.current_period_end;
      base.cancel_at_period_end = object.cancel_at_period_end === true;
      break;
    case "customer.subscription.deleted":
      base.status = "canceled";
      base.cancel_at_period_end = false;
      base.current_period_end = periodEnd(object) ?? base.current_period_end;
      break;
    case "invoice.paid":
      if (!current) return { kind: "ignore", reason: "unknown_subscription" };
      if (["past_due", "unpaid", "incomplete"].includes(current.status)) base.status = "active";
      base.current_period_end = later(current.current_period_end, periodEnd(object));
      break;
    case "invoice.payment_failed":
      if (!current) return { kind: "ignore", reason: "unknown_subscription" };
      if (["active", "trialing"].includes(current.status)) base.status = "past_due";
      break;
  }
  return replacesSubscriptionId
    ? { kind: "apply", next: base, replacesSubscriptionId }
    : { kind: "apply", next: base };
}

/**
 * The audit action for an access-relevant change (PRD §25), or null when the
 * change does not affect access (e.g. a renewal with the same plan).
 */
export function accessChangeAction(
  before:
    | (Pick<SubscriptionState, "status" | "plan_id" | "cancel_at_period_end"> &
        Partial<Pick<SubscriptionState, "billing_interval">>)
    | null,
  after: Pick<SubscriptionState, "status" | "plan_id" | "cancel_at_period_end"> &
    Partial<Pick<SubscriptionState, "billing_interval">>,
): string | null {
  const wasEntitled = before !== null && ENTITLED_STATUSES.includes(before.status);
  const isEntitled = ENTITLED_STATUSES.includes(after.status);
  if (!wasEntitled && isEntitled) {
    return after.status === "past_due"
      ? "billing.subscription_past_due"
      : "billing.subscription_activated";
  }
  if (wasEntitled && !isEntitled) {
    return after.status === "canceled"
      ? "billing.subscription_canceled"
      : "billing.subscription_inactive";
  }
  if (before && before.status !== after.status) {
    if (after.status === "past_due") return "billing.subscription_past_due";
    if (before.status === "past_due") return "billing.payment_recovered";
  }
  if (before && before.plan_id !== after.plan_id) return "billing.plan_changed";
  if (
    before?.billing_interval &&
    after.billing_interval &&
    before.billing_interval !== after.billing_interval
  )
    return "billing.plan_changed";
  if (before && before.cancel_at_period_end !== after.cancel_at_period_end) {
    return after.cancel_at_period_end ? "billing.cancel_scheduled" : "billing.cancel_reverted";
  }
  return null;
}
