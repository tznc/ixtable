import { assert, assertEquals } from "jsr:@std/assert@1";
import {
  accessChangeAction,
  buildFakeEvent,
  decideSubscriptionEvent,
  eventAppId,
  eventPlanHint,
  eventSubscriptionId,
  mapStripeStatus,
  parseStripeEvent,
  type StripeEvent,
  type SubscriptionState,
} from "./billing.ts";
import { RATE_LIMITS, rateLimitBucket } from "./rateLimit.ts";

const APP = "11111111-2222-3333-4444-555555555555";
const T0 = 1_790_000_000;

function event(input: Parameters<typeof buildFakeEvent>[0]): StripeEvent {
  return buildFakeEvent({
    subscriptionId: "sub_a",
    customerId: "cus_a",
    ...input,
  }) as unknown as StripeEvent;
}

function state(overrides: Partial<SubscriptionState> = {}): SubscriptionState {
  return {
    plan_id: "team",
    billing_interval: "month",
    status: "active",
    current_period_end: new Date((T0 + 30 * 86_400) * 1000).toISOString(),
    cancel_at_period_end: false,
    stripe_customer_id: "cus_a",
    stripe_subscription_id: "sub_a",
    provider_event_at: new Date(T0 * 1000).toISOString(),
    ...overrides,
  };
}

Deno.test("parseStripeEvent accepts Stripe-shaped events and rejects anything else", () => {
  const ok = parseStripeEvent(
    JSON.stringify(event({ type: "invoice.paid", appId: APP, planId: "team" })),
  );
  assert(ok);
  assertEquals(eventAppId(ok), APP);
  assertEquals(eventSubscriptionId(ok), "sub_a");
  assertEquals(parseStripeEvent("not json"), null);
  assertEquals(parseStripeEvent(JSON.stringify({ id: "evt", type: "x" })), null);
  assertEquals(parseStripeEvent("[]"), null);
});

Deno.test("checkout completion creates an active subscription for the purchased plan", () => {
  const checkout = event({
    type: "checkout.session.completed",
    appId: APP,
    planId: "starter",
    created: T0,
  });
  assertEquals(eventPlanHint(checkout).planId, "starter");
  const decision = decideSubscriptionEvent(checkout, null, "starter");
  assert(decision.kind === "apply");
  assertEquals(decision.next.status, "active");
  assertEquals(decision.next.plan_id, "starter");
  assertEquals(decision.next.stripe_subscription_id, "sub_a");
  assertEquals(decision.next.provider_event_at, new Date(T0 * 1000).toISOString());
  const unpaid = decideSubscriptionEvent(
    event({
      type: "checkout.session.completed",
      appId: APP,
      planId: "starter",
      paymentStatus: "unpaid",
    }),
    null,
    "starter",
  );
  assert(unpaid.kind === "apply" && unpaid.next.status === "incomplete");
});

Deno.test("older events are stale and never regress the subscription", () => {
  const canceled = state({
    status: "canceled",
    provider_event_at: new Date((T0 + 60) * 1000).toISOString(),
  });
  const lateActive = event({
    type: "customer.subscription.updated",
    appId: APP,
    planId: "team",
    status: "active",
    created: T0,
  });
  assertEquals(decideSubscriptionEvent(lateActive, canceled, "team").kind, "stale");
  const sameSecond = event({
    type: "customer.subscription.updated",
    appId: APP,
    planId: "team",
    status: "past_due",
    created: T0,
  });
  const applied = decideSubscriptionEvent(sameSecond, state(), "team");
  assert(applied.kind === "apply" && applied.next.status === "past_due");
});

Deno.test("events about a replaced subscription are ignored; a new purchase replaces it", () => {
  const current = state({ stripe_subscription_id: "sub_new" });
  const oldDeleted = event({
    type: "customer.subscription.deleted",
    appId: APP,
    planId: "team",
    created: T0 + 10,
  });
  assertEquals(decideSubscriptionEvent(oldDeleted, current, "team"), {
    kind: "ignore",
    reason: "other_subscription",
  });
  const newer = event({
    type: "checkout.session.completed",
    appId: APP,
    planId: "business",
    subscriptionId: "sub_next",
    created: T0 + 10,
  });
  const decision = decideSubscriptionEvent(newer, current, "business");
  assert(decision.kind === "apply");
  assertEquals(decision.replacesSubscriptionId, "sub_new");
  assertEquals(decision.next.plan_id, "business");
  assertEquals(decision.next.stripe_subscription_id, "sub_next");
});

Deno.test("invoice events move between active and past_due without reviving a canceled plan", () => {
  const failed = decideSubscriptionEvent(
    event({ type: "invoice.payment_failed", appId: APP, planId: "team", created: T0 + 1 }),
    state(),
    "team",
  );
  assert(failed.kind === "apply" && failed.next.status === "past_due");
  const paid = decideSubscriptionEvent(
    event({ type: "invoice.paid", appId: APP, planId: "team", created: T0 + 2 }),
    state({ status: "past_due" }),
    "team",
  );
  assert(paid.kind === "apply" && paid.next.status === "active");
  const paidAfterCancel = decideSubscriptionEvent(
    event({ type: "invoice.paid", appId: APP, planId: "team", created: T0 + 2 }),
    state({ status: "canceled" }),
    "team",
  );
  assert(paidAfterCancel.kind === "apply" && paidAfterCancel.next.status === "canceled");
  assertEquals(
    decideSubscriptionEvent(
      event({ type: "invoice.paid", appId: APP, planId: "team" }),
      null,
      "team",
    ).kind,
    "ignore",
  );
});

Deno.test("deleted cancels; unknown types and unknown plans are ignored", () => {
  const deleted = decideSubscriptionEvent(
    event({ type: "customer.subscription.deleted", appId: APP, planId: "team", created: T0 + 5 }),
    state(),
    "team",
  );
  assert(deleted.kind === "apply" && deleted.next.status === "canceled");
  const other = {
    ...event({ type: "invoice.paid", appId: APP, planId: "team" }),
    type: "customer.created",
  };
  assertEquals(decideSubscriptionEvent(other, null, null).kind, "ignore");
  assertEquals(
    decideSubscriptionEvent(
      event({ type: "customer.subscription.created", appId: APP, planId: "team" }),
      null,
      null,
    ),
    { kind: "ignore", reason: "unknown_plan" },
  );
  assertEquals(mapStripeStatus("bogus"), "incomplete");
  assertEquals(mapStripeStatus("trialing"), "trialing");
});

Deno.test("accessChangeAction names only access-relevant changes", () => {
  const active = { status: "active", plan_id: "team", cancel_at_period_end: false };
  assertEquals(accessChangeAction(null, active), "billing.subscription_activated");
  assertEquals(
    accessChangeAction(active, { ...active, status: "past_due" }),
    "billing.subscription_past_due",
  );
  assertEquals(
    accessChangeAction({ ...active, status: "past_due" }, active),
    "billing.payment_recovered",
  );
  assertEquals(
    accessChangeAction(active, { ...active, status: "canceled" }),
    "billing.subscription_canceled",
  );
  assertEquals(
    accessChangeAction(active, { ...active, status: "unpaid" }),
    "billing.subscription_inactive",
  );
  assertEquals(
    accessChangeAction(active, { ...active, plan_id: "starter" }),
    "billing.plan_changed",
  );
  assertEquals(
    accessChangeAction(active, { ...active, cancel_at_period_end: true }),
    "billing.cancel_scheduled",
  );
  assertEquals(accessChangeAction(active, active), null);
});

Deno.test("RATE_LIMITS covers every sensitive endpoint with sane windows", () => {
  for (const name of [
    "key-grant",
    "bundle-manifest",
    "desktop-auth-exchange",
    "billing-checkout",
    "account-export",
    "account-delete",
    "admin-support",
  ] as const) {
    const limit = RATE_LIMITS[name];
    assert(limit.max > 0 && limit.windowSeconds >= 60, name);
  }
  assertEquals(rateLimitBucket("account-export", "u1"), "account-export:u1");
});

Deno.test("annual purchases record the year interval from the plan resolution", () => {
  const checkout = event({
    type: "checkout.session.completed",
    appId: APP,
    planId: "team",
    interval: "year",
    created: T0,
  });
  const hint = eventPlanHint(checkout);
  assertEquals(hint.interval, "year");
  assertEquals(hint.planId, "team");
  const decision = decideSubscriptionEvent(checkout, null, { planId: "team", interval: "year" });
  assert(decision.kind === "apply");
  assertEquals(decision.next.billing_interval, "year");
  assertEquals(decision.next.status, "active");
});

Deno.test("a bare plan id keeps the current interval; an interval switch is a plan change", () => {
  const renewal = event({
    type: "customer.subscription.updated",
    appId: APP,
    planId: "team",
    created: T0 + 60,
  });
  const kept = decideSubscriptionEvent(renewal, state({ billing_interval: "year" }), "team");
  assert(kept.kind === "apply");
  assertEquals(kept.next.billing_interval, "year");

  const switched = decideSubscriptionEvent(renewal, state(), { planId: "team", interval: "year" });
  assert(switched.kind === "apply");
  assertEquals(switched.next.billing_interval, "year");
  assertEquals(accessChangeAction(state(), switched.next), "billing.plan_changed");
  assertEquals(accessChangeAction(state(), state()), null);
});

Deno.test("fake annual events carry the annual price and a one-year period", () => {
  const annual = event({
    type: "customer.subscription.created",
    appId: APP,
    planId: "starter",
    interval: "year",
    created: T0,
  });
  assertEquals(eventPlanHint(annual).priceId, "price_fake_starter_annual");
  const end = Number(annual.data.object.current_period_end);
  assert(end - Math.floor(Date.now() / 1000) > 360 * 86_400);
});
