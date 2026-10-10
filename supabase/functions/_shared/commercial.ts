// Shared helpers for the commercial functions (billing-*, stripe-webhook,
// account-*, admin-support). Authorization rules: billing actions need the
// `billing` app capability (app owner, org owner/admin/billing, per-app
// admin/billing; SQL app_capabilities_for). The organization is the billing
// customer (org_billing_customers, PRD §4.5).
import { audit } from "./audit.ts";
import { billingProvider, ENDED_STATUSES, signStripePayload } from "./billing.ts";
import { env, optionalEnv, serviceClient } from "./db.ts";
import { HttpError } from "./http.ts";
import type { SubscriptionState } from "./billing.ts";

export interface BillingApp {
  id: string;
  org_id: string;
  owner_id: string;
  name: string;
  deleted_at: string | null;
}

export interface SubscriptionRow extends SubscriptionState {
  id: string;
  app_id: string;
  provider: "stripe" | "fake";
  created_at: string;
  updated_at: string;
}

export const SUBSCRIPTION_COLUMNS =
  "id, app_id, plan_id, billing_interval, provider, status, current_period_end, cancel_at_period_end, stripe_customer_id, stripe_subscription_id, provider_event_at, created_at, updated_at";

/** Client-facing subscription shape (no provider ids). */
export function publicSubscription(row: SubscriptionRow | null): Record<string, unknown> | null {
  if (!row) return null;
  return {
    app_id: row.app_id,
    plan_id: row.plan_id,
    billing_interval: row.billing_interval,
    provider: row.provider,
    status: row.status,
    current_period_end: row.current_period_end,
    cancel_at_period_end: row.cancel_at_period_end,
  };
}

/** The user's capabilities on a live app: owner, admin, billing, view (SQL app_capabilities_for). */
export async function appCapabilities(appId: string, userId: string): Promise<string[]> {
  const { data, error } = await serviceClient().rpc("app_capabilities_for", {
    p_app_id: appId,
    p_user_id: userId,
  });
  if (error) throw new Error(`load app capabilities failed: ${error.message}`);
  return (data as string[] | null) ?? [];
}

/**
 * Loads a live app the caller may manage billing for. Callers with no
 * relation to the app get NOT_FOUND (existence is not revealed); members
 * without the billing capability get FORBIDDEN.
 */
export async function requireBillingApp(appId: string, userId: string): Promise<BillingApp> {
  const db = serviceClient();
  const { data: app, error } = await db
    .from("cloud_apps")
    .select("id, org_id, owner_id, name, deleted_at")
    .eq("id", appId)
    .maybeSingle();
  if (error) throw new Error(`load app failed: ${error.message}`);
  if (!app || app.deleted_at) throw new HttpError("NOT_FOUND", "App not found");
  const capabilities = await appCapabilities(appId, userId);
  if (capabilities.includes("billing")) return app as BillingApp;
  const [{ data: membership }, { data: appMember }] = await Promise.all([
    db
      .from("org_members")
      .select("role")
      .eq("org_id", app.org_id)
      .eq("user_id", userId)
      .maybeSingle(),
    db.from("app_members").select("status").eq("app_id", appId).eq("user_id", userId).maybeSingle(),
  ]);
  if (capabilities.length > 0 || membership || appMember) {
    throw new HttpError(
      "FORBIDDEN",
      "Only the app owner, an organization owner, admin or billing member, or an app admin or billing member can manage billing",
    );
  }
  throw new HttpError("NOT_FOUND", "App not found");
}

/** The organization's billing customer id, if it has one. */
export async function orgBillingCustomerId(orgId: string): Promise<string | null> {
  const { data, error } = await serviceClient()
    .from("org_billing_customers")
    .select("stripe_customer_id")
    .eq("org_id", orgId)
    .maybeSingle();
  if (error) throw new Error(`load billing customer failed: ${error.message}`);
  return data?.stripe_customer_id ?? null;
}

/**
 * The organization's billing customer, created with the provider on first
 * checkout. Concurrent first checkouts keep whichever row was stored first.
 */
export async function ensureOrgBillingCustomer(orgId: string, email: string): Promise<string> {
  const existing = await orgBillingCustomerId(orgId);
  if (existing) return existing;
  const db = serviceClient();
  const { data: org } = await db.from("organizations").select("name").eq("id", orgId).single();
  const provider = billingProvider();
  const { customerId } = await provider.createCustomer({
    orgId,
    orgName: org?.name ?? "",
    email,
  });
  const { error } = await db
    .from("org_billing_customers")
    .insert({ org_id: orgId, provider: provider.name, stripe_customer_id: customerId });
  if (error && error.code !== "23505")
    throw new Error(`store billing customer failed: ${error.message}`);
  return (await orgBillingCustomerId(orgId)) ?? customerId;
}

/** Customer for an app's portal and invoices: the org's, else the subscription's (pre-org rows). */
export async function appBillingCustomerId(app: BillingApp): Promise<string | null> {
  return (
    (await orgBillingCustomerId(app.org_id)) ??
    (await getSubscription(app.id))?.stripe_customer_id ??
    null
  );
}

export async function getSubscription(appId: string): Promise<SubscriptionRow | null> {
  const { data, error } = await serviceClient()
    .from("subscriptions")
    .select(SUBSCRIPTION_COLUMNS)
    .eq("app_id", appId)
    .maybeSingle();
  if (error) throw new Error(`load subscription failed: ${error.message}`);
  return (data as SubscriptionRow | null) ?? null;
}

export function siteUrl(): string {
  return (optionalEnv("SITE_URL") ?? "http://127.0.0.1:3001").replace(/\/$/, "");
}

/** The website's billing tab of an app (checkout return and portal return URL). */
export function appBillingUrl(appId: string, extra: Record<string, string> = {}): string {
  const url = new URL("/cloud/app", siteUrl());
  url.searchParams.set("id", appId);
  url.searchParams.set("tab", "billing");
  for (const [key, value] of Object.entries(extra)) url.searchParams.set(key, value);
  return url.toString();
}

/**
 * Signs a Stripe-shaped event with STRIPE_WEBHOOK_SECRET and posts it to
 * stripe-webhook, exactly as Stripe would. Used by the fake provider only, so
 * the fake and real flows share one webhook path.
 */
export async function postSignedEvent(event: Record<string, unknown>): Promise<{
  status: number;
  body: unknown;
}> {
  const payload = JSON.stringify(event);
  const signature = await signStripePayload(payload, env("STRIPE_WEBHOOK_SECRET"));
  const response = await fetch(`${env("SUPABASE_URL")}/functions/v1/stripe-webhook`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Stripe-Signature": signature,
      apikey: env("SUPABASE_ANON_KEY"),
    },
    body: payload,
  });
  const body = await response.json().catch(() => null);
  return { status: response.status, body };
}

/** A subscription that still bills (or could): anything not canceled or expired. */
export function isBilling(status: string | null | undefined): boolean {
  return !!status && !ENDED_STATUSES.includes(status);
}

/**
 * Cancels a subscription with the provider at once (no period-end grace) and
 * marks the local row canceled, then audits billing.subscription_canceled.
 * Used when the app goes away (apps-delete, account-delete); billing-cancel
 * is the user-facing path with the period-end option.
 */
export async function cancelSubscriptionNow(
  sub: { app_id: string; status: string; plan_id: string; stripe_subscription_id: string | null },
  ctx: { actorId: string; orgId?: string | null; reason: string; req: Request },
): Promise<void> {
  if (sub.stripe_subscription_id) {
    await billingProvider().cancelSubscription({
      subscriptionId: sub.stripe_subscription_id,
      atPeriodEnd: false,
    });
  }
  const { error } = await serviceClient()
    .from("subscriptions")
    .update({ status: "canceled", cancel_at_period_end: false })
    .eq("app_id", sub.app_id);
  if (error) throw new Error(`cancel subscription failed: ${error.message}`);
  await audit({
    action: "billing.subscription_canceled",
    actorId: ctx.actorId,
    orgId: ctx.orgId ?? null,
    appId: sub.app_id,
    target: `plan:${sub.plan_id}`,
    details: { reason: ctx.reason, previousStatus: sub.status },
    req: ctx.req,
  });
}
