// Chaos probes for the billing flow. Each test asserts the CORRECT behaviour;
// a failure is a finding. Outcomes are recorded before the assertions.
import { callFunction, getAnonClient, getServiceClient } from "../clients";
import { expect, test } from "../fixture";
import { recordOutcome } from "../record";
import { addMember, createRole, type TestUser } from "../seed";
import {
  auditActions,
  checkoutAndPay,
  entitlementAs,
  nowSeconds,
  postWebhook,
  stripeEvent,
  subscriptionRow,
} from "./billing-fixtures";

const rand = () => Math.random().toString(36).slice(2, 10);

async function buy(appId: string, planId: string, subscriptionId: string, created: number) {
  for (const type of ["checkout.session.completed", "customer.subscription.created"] as const) {
    const r = await postWebhook(stripeEvent({ type, appId, planId, subscriptionId, created }));
    if (r.status !== 200) throw new Error(`${type} -> ${r.status} ${JSON.stringify(r.body)}`);
  }
}

test("C01 an unpaid checkout must not replace an active paid subscription", async ({ cloud }) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  const t = nowSeconds() - 60;
  await buy(app.id, "team", `sub_A_${rand()}`, t);
  const before = await entitlementAs(owner, app.id);
  const event = stripeEvent({
    type: "checkout.session.completed",
    appId: app.id,
    planId: "business",
    subscriptionId: `sub_B_${rand()}`,
    created: t + 30,
  });
  (event.data as { object: Record<string, unknown> }).object.payment_status = "unpaid";
  const res = await postWebhook(event);
  const row = await subscriptionRow(app.id);
  const after = await entitlementAs(owner, app.id);
  recordOutcome("chaos-01-unpaid-checkout-replaces", {
    expectations: ["Unpaid checkout for a new sub leaves the paid sub A active and entitled."],
    details: { before, res: res.body, row, after },
  });
  expect(after.allowed).toBe(true);
  expect(row?.stripe_subscription_id).toMatch(/^sub_A_/);
});

test("C02 interleaved same-second purchases leave the app on a live subscription", async ({
  cloud,
}) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  const t = nowSeconds() - 60;
  const B = `sub_B_${rand()}`;
  const C = `sub_C_${rand()}`;
  const post = (type: Parameters<typeof stripeEvent>[0]["type"], sub: string, created = t) =>
    postWebhook(stripeEvent({ type, appId: app.id, planId: "team", subscriptionId: sub, created }));
  await post("checkout.session.completed", B);
  await post("checkout.session.completed", C); // replaces B -> provider cancels B
  await post("customer.subscription.created", B); // late B event, same second
  const mid = await subscriptionRow(app.id);
  // Stripe then reports B deleted (it was cancelled when C replaced it).
  await post("customer.subscription.deleted", B, t + 5);
  const row = await subscriptionRow(app.id);
  const after = await entitlementAs(owner, app.id);
  recordOutcome("chaos-02-interleaved-purchases", {
    expectations: ["App stays on C (paid, live) and entitled."],
    details: { mid, row, after },
  });
  expect(row?.stripe_subscription_id).toBe(C);
  expect(after.allowed).toBe(true);
});

test("C03 a same-second update cannot revive a deleted subscription", async ({ cloud }) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  const S = `sub_S_${rand()}`;
  const t = nowSeconds() - 60;
  await buy(app.id, "team", S, t - 30);
  const ev = (type: Parameters<typeof stripeEvent>[0]["type"]) =>
    stripeEvent({ type, appId: app.id, planId: "team", subscriptionId: S, created: t });
  await postWebhook(ev("customer.subscription.deleted"));
  await postWebhook(ev("customer.subscription.updated")); // delivered late, same second
  const row = await subscriptionRow(app.id);
  const after = await entitlementAs(owner, app.id);
  recordOutcome("chaos-03-same-second-revive", {
    expectations: ["Deleted is terminal: status stays canceled."],
    details: { row, after },
  });
  expect(row?.status).toBe("canceled");
});

test("C04 past_due grace is bounded to 7 days from the failure", async ({ cloud }) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  const S = `sub_S_${rand()}`;
  const t = nowSeconds() - 120;
  await buy(app.id, "team", S, t);
  // Stripe semantics: renewal rolls current_period_end forward, then payment fails.
  await postWebhook(
    stripeEvent({
      type: "customer.subscription.updated",
      appId: app.id,
      planId: "team",
      subscriptionId: S,
      status: "past_due",
      created: t + 10,
      currentPeriodEnd: nowSeconds() + 30 * 86_400,
    }),
  );
  const row = await subscriptionRow(app.id);
  const ent = await entitlementAs(owner, app.id);
  const graceEnds = new Date(Date.parse(row!.current_period_end!) + 7 * 86_400_000);
  // And a past_due row without a period end.
  await getServiceClient()
    .from("subscriptions")
    .update({ current_period_end: null })
    .eq("app_id", app.id);
  const entNull = await entitlementAs(owner, app.id);
  recordOutcome("chaos-04-grace-window", {
    expectations: ["Grace ends <= 7 days after the failed renewal, never open-ended."],
    details: { row, ent, graceEnds, entNull },
  });
  expect(graceEnds.getTime()).toBeLessThan(Date.now() + 8 * 86_400_000);
  expect(entNull.allowed).toBe(false);
});

test("C05 double-submitted checkout cannot produce two paid subscriptions", async ({ cloud }) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  const start = () =>
    callFunction<{ url: string }>("billing-checkout", {
      jwt: owner.jwt,
      body: { appId: app.id, planId: "team" },
    });
  const [a, b] = await Promise.all([start(), start()]);
  const pay = (r: typeof a) => {
    const u = new URL(r.body.url);
    return callFunction("billing-fake-complete", {
      jwt: owner.jwt,
      body: { sessionId: u.searchParams.get("session"), appId: app.id, planId: "team" },
    });
  };
  const pa = await pay(a);
  const pb = await pay(b);
  const { data: events } = await getServiceClient()
    .from("billing_events")
    .select("type, payload, outcome")
    .eq("app_id", app.id)
    .eq("type", "customer.subscription.created");
  const subs = new Set((events ?? []).map((e) => (e.payload as { id: string }).id));
  recordOutcome("chaos-05-double-checkout", {
    expectations: ["Second checkout is refused or the second payment is refused; one subscription."],
    details: { starts: [a.status, b.status], pays: [pa.status, pb.status], subs: [...subs] },
  });
  expect(subs.size).toBe(1);
});

test("C06 re-subscribing after scheduling cancellation does not start a second subscription", async ({
  cloud,
}) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  await checkoutAndPay(owner, app.id, "team");
  await callFunction("billing-cancel", { jwt: owner.jwt, body: { appId: app.id } });
  const again = await callFunction<{ url?: string }>("billing-checkout", {
    jwt: owner.jwt,
    body: { appId: app.id, planId: "team" },
  });
  recordOutcome("chaos-06-resubscribe-after-cancel", {
    expectations: ["Undo-cancel path instead of a new paid checkout for the same plan."],
    details: { status: again.status, body: again.body },
  });
  expect(again.status).not.toBe(200);
});

test("C07 concurrent duplicate deliveries apply once and leave a clean event row", async ({
  cloud,
}) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  const S = `sub_S_${rand()}`;
  await buy(app.id, "team", S, nowSeconds() - 60);
  const event = stripeEvent({
    type: "customer.subscription.updated",
    appId: app.id,
    planId: "business",
    subscriptionId: S,
    created: nowSeconds() - 30,
  });
  (event.data as { object: Record<string, unknown> }).object.items = {
    data: [{ price: { id: "price_fake_business" } }],
  };
  const results = await Promise.all(Array.from({ length: 8 }, () => postWebhook(event)));
  const { data: rows } = await getServiceClient()
    .from("billing_events")
    .select("outcome, error, processed_at")
    .eq("event_id", event.id as string);
  const audit = (await auditActions({ appId: app.id })).filter(
    (a) => a.action === "billing.plan_changed",
  );
  recordOutcome("chaos-07-concurrent-duplicates", {
    expectations: ["All 2xx (or retried), one plan_changed audit, event row outcome=applied."],
    details: { statuses: results.map((r) => r.status), rows, auditCount: audit.length },
  });
  expect(results.filter((r) => r.status >= 500)).toHaveLength(0);
  expect(rows?.[0]?.outcome).toBe("applied");
  expect(audit).toHaveLength(1);
});

test("C08 cancel racing a plan change does not mark the new subscription canceled", async ({
  cloud,
}) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  await checkoutAndPay(owner, app.id, "team");
  const findings: unknown[] = [];
  for (let i = 0; i < 4; i += 1) {
    const co = await callFunction<{ url: string }>("billing-checkout", {
      jwt: owner.jwt,
      body: { appId: app.id, planId: i % 2 ? "team" : "business" },
    });
    if (co.status !== 200) break;
    const u = new URL(co.body.url);
    const [pay, cancel] = await Promise.all([
      callFunction("billing-fake-complete", {
        jwt: owner.jwt,
        body: { sessionId: u.searchParams.get("session"), appId: app.id, planId: u.searchParams.get("plan") },
      }),
      callFunction("billing-cancel", { jwt: owner.jwt, body: { appId: app.id, atPeriodEnd: false } }),
    ]);
    const row = await subscriptionRow(app.id);
    findings.push({ pay: pay.status, cancel: cancel.status, row });
    if (row?.status === "canceled") {
      // was the canceled row's sub the one the cancel targeted?
      const { data } = await getServiceClient()
        .from("billing_events")
        .select("type, payload")
        .eq("app_id", app.id)
        .eq("type", "customer.subscription.deleted");
      const deleted = new Set((data ?? []).map((e) => (e.payload as { id: string }).id));
      findings.push({ deletedSubs: [...deleted], rowSub: row.stripe_subscription_id });
      if (!deleted.has(row.stripe_subscription_id ?? "")) break;
      await checkoutAndPay(owner, app.id, "team");
    }
  }
  recordOutcome("chaos-08-cancel-vs-change", { expectations: ["no drift"], details: { findings } });
  const drift = findings.some(
    (f) =>
      (f as { deletedSubs?: string[]; rowSub?: string }).deletedSubs &&
      !(f as { deletedSubs: string[] }).deletedSubs.includes((f as { rowSub: string }).rowSub),
  );
  expect(drift).toBe(false);
});

test("C09 RLS blocks direct writes and reads of billing tables", async ({ cloud }) => {
  const owner = await cloud.user();
  const outsider = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  await checkoutAndPay(owner, app.id, "starter");
  const tries: Record<string, unknown> = {};
  const c = owner.client;
  tries.updateOwnSub = await c
    .from("subscriptions")
    .update({ plan_id: "business", status: "active" })
    .eq("app_id", app.id)
    .select();
  tries.insertSub = await c.from("subscriptions").insert({
    app_id: app.id,
    plan_id: "business",
    status: "active",
  });
  tries.upsertSub = await c
    .from("subscriptions")
    .upsert({ app_id: app.id, plan_id: "business", status: "active" }, { onConflict: "app_id" });
  tries.deleteSub = await c.from("subscriptions").delete().eq("app_id", app.id).select();
  tries.updatePlan = await c.from("plans").update({ runtime_user_allowance: 9999 }).eq("id", "starter").select();
  tries.readEvents = await c.from("billing_events").select("*").limit(5);
  tries.insertEvent = await c.from("billing_events").insert({ event_id: `evt_${rand()}`, type: "x" });
  tries.readSessions = await c.from("billing_checkout_sessions").select("*").limit(5);
  tries.readCustomers = await c.from("org_billing_customers").select("*").limit(5);
  tries.outsiderSub = await outsider.client.from("subscriptions").select("*").eq("app_id", app.id);
  tries.outsiderEnt = await outsider.client.rpc("app_entitlement", { p_app_id: app.id });
  tries.anonPlans = await getAnonClient().from("plans").select("id");
  tries.anonSubs = await getAnonClient().from("subscriptions").select("*").limit(1);
  const outsiderCheckout = await callFunction("billing-checkout", {
    jwt: outsider.jwt,
    body: { appId: app.id, planId: "team" },
  });
  const outsiderFake = await callFunction("billing-fake-complete", {
    jwt: outsider.jwt,
    body: { sessionId: "cs_fake_doesnotexist", appId: app.id, planId: "team" },
  });
  const row = await subscriptionRow(app.id);
  const { data: plan } = await getServiceClient().from("plans").select("runtime_user_allowance").eq("id", "starter").single();
  const summary = Object.fromEntries(
    Object.entries(tries).map(([k, v]) => {
      const r = v as { data: unknown; error: { code?: string; message: string } | null; status: number };
      return [k, { status: r.status, error: r.error?.code ?? r.error?.message ?? null, rows: Array.isArray(r.data) ? r.data.length : r.data ? 1 : 0 }];
    }),
  );
  recordOutcome("chaos-09-rls", {
    expectations: ["No user/anon write lands; billing internals unreadable; outsiders see nothing."],
    details: { summary, outsiderCheckout: outsiderCheckout.status, outsiderFake: outsiderFake.status, row, plan },
  });
  expect(row).toMatchObject({ plan_id: "starter", status: "active" });
  expect(plan?.runtime_user_allowance).toBe(5);
  expect((tries.readEvents as { data: unknown[] }).data ?? []).toHaveLength(0);
  expect((tries.outsiderSub as { data: unknown[] }).data ?? []).toHaveLength(0);
  expect([403, 404]).toContain(outsiderCheckout.status);
});

test("C10 concurrent seat activations cannot exceed the allowance", async ({ cloud }) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  await checkoutAndPay(owner, app.id, "starter"); // 5 seats
  const role = await createRole(app.id);
  const members: TestUser[] = [];
  for (let i = 0; i < 8; i += 1) {
    const m = await cloud.user();
    await addMember(app.id, m.user.id, role.id, i < 4 ? "active" : "revoked");
    members.push(m);
  }
  const res = await Promise.all(
    members.slice(4).map((m) =>
      callFunction("members-update", {
        jwt: owner.jwt,
        body: { appId: app.id, userId: m.user.id, status: "active" },
      }),
    ),
  );
  const ent = await entitlementAs(owner, app.id);
  recordOutcome("chaos-10-seat-race", {
    expectations: ["Exactly one activation succeeds; used <= 5; others 402."],
    details: { statuses: res.map((r) => r.status), bodies: res.map((r) => r.body), ent },
  });
  expect(ent.used).toBeLessThanOrEqual(5);
});

test("C11 webhook rejects forged/odd signatures and absurd payloads", async ({ cloud }) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  const base = () =>
    stripeEvent({ type: "checkout.session.completed", appId: app.id, planId: "business", subscriptionId: `sub_${rand()}` });
  const results: Record<string, number> = {};
  results.wrongSecret = (await postWebhook(base(), { secret: "whsec_wrong" })).status;
  results.future10m = (await postWebhook(base(), { timestamp: nowSeconds() + 600 })).status;
  results.whitespace = (await postWebhook(base(), { tamper: (p) => ` ${p}` })).status;
  const raw = JSON.stringify(base());
  results.noHeader = (await callFunction("stripe-webhook", { body: raw })).status;
  results.v0Only = (
    await callFunction("stripe-webhook", { body: raw, headers: { "Stripe-Signature": `t=${nowSeconds()},v0=abc` } })
  ).status;
  const badApp = base();
  (badApp.data as { object: Record<string, unknown> }).object.metadata = { app_id: "' or 1=1 --", plan_id: "business" };
  (badApp.data as { object: Record<string, unknown> }).object.client_reference_id = null;
  const badRes = await postWebhook(badApp);
  const unknownPlan = stripeEvent({ type: "customer.subscription.created", appId: app.id, planId: "nope", subscriptionId: `sub_${rand()}` });
  (unknownPlan.data as { object: Record<string, unknown> }).object.items = { data: [{ price: { id: "price_unknown" } }] };
  const unknownRes = await postWebhook(unknownPlan);
  const row = await subscriptionRow(app.id);
  recordOutcome("chaos-11-forged", {
    expectations: ["All forged variants 400; bad app id ignored; unknown plan ignored; no row."],
    details: { results, badRes: badRes.body, unknownRes: unknownRes.body, row },
  });
  for (const s of Object.values(results)) expect(s).toBe(400);
  expect(row).toBeNull();
});

test("C12 expired or reused checkout sessions cannot be completed", async ({ cloud }) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  const co = await callFunction<{ url: string }>("billing-checkout", {
    jwt: owner.jwt,
    body: { appId: app.id, planId: "team" },
  });
  const sessionId = new URL(co.body.url).searchParams.get("session")!;
  await getServiceClient()
    .from("billing_checkout_sessions")
    .update({ expires_at: new Date(Date.now() - 1000).toISOString() })
    .eq("id", sessionId);
  const expired = await callFunction("billing-fake-complete", {
    jwt: owner.jwt,
    body: { sessionId, appId: app.id, planId: "team" },
  });
  const swapPlan = await callFunction("billing-fake-complete", {
    jwt: owner.jwt,
    body: { sessionId, appId: app.id, planId: "business" },
  });
  const { sessionId: s2 } = await checkoutAndPay(owner, app.id, "starter");
  const reuse = await callFunction("billing-fake-complete", {
    jwt: owner.jwt,
    body: { sessionId: s2, appId: app.id, planId: "starter" },
  });
  recordOutcome("chaos-12-expired-reused", {
    expectations: ["expired 400, plan swap 404, reuse 400, with clear messages."],
    details: { expired: expired.body, swapPlan: swapPlan.body, reuse: reuse.body, statuses: [expired.status, swapPlan.status, reuse.status] },
  });
  expect(expired.status).toBe(400);
  expect(swapPlan.status).toBe(404);
  expect(reuse.status).toBe(400);
});

test("C13 a cancelled-then-deleted app's late webhook does not resurrect billing", async ({
  cloud,
}) => {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  await checkoutAndPay(owner, app.id, "team");
  const before = await subscriptionRow(app.id);
  const cancel = await callFunction("billing-cancel", {
    jwt: owner.jwt,
    body: { appId: app.id, atPeriodEnd: true },
  });
  // A renewal-time update that Stripe created before the cancel lands after it.
  const late = stripeEvent({
    type: "customer.subscription.updated",
    appId: app.id,
    planId: "team",
    subscriptionId: before!.stripe_subscription_id!,
    created: Math.floor(Date.parse(before!.provider_event_at!) / 1000),
    cancelAtPeriodEnd: false,
  });
  const lateRes = await postWebhook(late);
  const row = await subscriptionRow(app.id);
  recordOutcome("chaos-13-late-update-after-cancel", {
    expectations: ["cancel_at_period_end stays true."],
    details: { cancel: cancel.status, lateRes: lateRes.body, row },
  });
  expect(row?.cancel_at_period_end).toBe(true);
});
