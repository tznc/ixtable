# Billing chaos test report

- **Date:** 2026-10-31. **Commit:** `1440d54`.
- **Stack:** local Supabase CLI 2.122 (`scripts/cloud/up.mjs`). `BILLING_PROVIDER=fake`. Events are signed with the real webhook secret, so they go through the real `stripe-webhook`.
- **Probes:** `web/e2e/service-qa/specs/billing-chaos.spec.ts` (13 contract probes) and `web/e2e/service-qa/ui/billing-chaos.spec.ts` (3 website captures). Each probe asserts the **correct** behaviour, so a failing probe is a finding. Recorded outputs are in this folder (`chaos-*.json`, `ui-chaos-*.png`).
- **Rerun:** `cd web && npx playwright test --config playwright.service-qa.config.ts --project=contracts specs/billing-chaos.spec.ts`
- **Limit:** the fake provider cannot show real Stripe charges. Findings about charges are confirmed by state plus code reading, and are marked "code-verified" where that applies.

## Summary

| # | Sev | Finding | Evidence |
|---|---|---|---|
| F1 | Critical | Every plan change is a new full-price subscription. The old one is cancelled at once with no proration or refund, so the customer is double-charged. | code + C06, billing-07 |
| F2 | Critical | A double-submitted checkout creates 2 paid subscriptions. | C05 ✗ |
| F3 | Critical | Purchase events that arrive interleaved in the same second leave the app on a cancelled subscription. The paid subscription keeps billing, and its events are ignored. | C02 ✗ |
| F4 | High | An unpaid or async checkout replaces and cancels the active paid subscription, so access is lost. For a row without a subscription id, an unpaid checkout gives a free upgrade. | C01 ✗, ui-chaos-01 |
| F5 | High | The stale-event check uses 1 s resolution with a strict `<`. A same-second event can revive a deleted subscription or undo a cancel. | C03 ✗, C13 ✗ |
| F6 | High | The past_due grace period has no real bound: about 37 days after a renewal fails, and forever when the period end is null. | C04 ✗, ui-chaos-04 |
| F7 | Medium | There is no way to undo a scheduled cancel. The UI only offers new full-price checkouts. | C06 ✗, ui-chaos-06 |
| F8 | Medium | Invoice webhooks read `invoice.subscription`, which newer Stripe API versions removed. Payment-failed and paid events would then be silently ignored. | code |
| F9 | Medium | Currency display: every currency is divided by 100 (JPY/KRW shown 100× too small), and plan prices are hard-coded to USD. | code |
| F10 | Medium | Desktop shows "The developer must upgrade" for every 402, even when the real reason is a lapsed payment. | cloud-90-entitlement-lapsed.png |
| F11 | Low | `billing-cancel` writes the local row optimistically, without a compare-and-set, so it can race webhooks. | code, C08 not reproduced in 4 runs |
| F12 | Low | Error text shown to users includes the internal field name ("sessionId: checkout session is no longer open"). | C12 |
| F13 | Low | Two concurrent first checkouts create an orphan Stripe customer. | code |

**Held up under chaos (no action needed):**

- **Forged or odd signatures:** wrong secret, ±10 min skew, whitespace changes, missing header and `v0`-only were all rejected with 400 (C11).
- **Bad metadata:** an injected app id and an unknown price were ignored.
- **RLS (C09):** user writes and upserts to `subscriptions` and `plans`, and reads of `billing_events`, `billing_checkout_sessions` and `org_billing_customers`, all returned 42501. Outsiders see 0 rows, `app_entitlement` reports not_found, and outsider checkouts get 404. Anon is denied.
- **Duplicate delivery (C07):** 8 concurrent copies of one event were all 200, applied once, with one audit row.
- **Seat race (C10):** 4 concurrent activations with 1 seat free gave one 200 and three 402s. Usage ended at 5/5.
- **Checkout sessions (C12):** expired, reused and plan-swapped sessions were all refused.

---

## F1 · Critical · A plan change double-charges the customer

**Where:**

- `billing-checkout` always opens a new Checkout subscription.
- In `stripe-webhook` `apply()`, `replacesSubscriptionId` leads to `cancelSubscription({atPeriodEnd:false})`.
- That becomes Stripe `DELETE /v1/subscriptions/:id` with no `prorate`/`invoice_now`, so Stripe issues no credit for the unused time.

**Repro:**

1. App on Team ($49), 10 days into the period.
2. Click "Choose Business".
3. Pay $149 for a new subscription. Team is cancelled with ~$33 unused and no credit.

The same path runs for downgrades and for re-subscribing after a cancel (F7).

**Impact:** the customer is overcharged on every plan change. Expect chargebacks.

**Options:**

- **A.** Change plans in place: `POST /v1/subscriptions/:id` with the new price and `proration_behavior=create_prorations`. Use Checkout only when there is no live subscription.
- **B.** Keep Checkout, but cancel the old subscription with `prorate=true&invoice_now=true` so the credit is applied.
- **C.** Send plan changes to the Stripe Customer Portal (subscription_update enabled), and keep in-app Checkout for first purchase only.

**Recommendation: A.** It gives one subscription per app, native proration, and removes the replacement logic that causes F2, F3 and F4.

## F2 · Critical · A double-submitted checkout creates two paid subscriptions

**Repro (C05):**

1. Run two `billing-checkout` calls in parallel: both return 200.
2. Complete both sessions: both return 200.
3. Result: 2 distinct `customer.subscription.created` subscription ids (`sub_fake_PE4K…`, `sub_fake_Ltsh…`).

With Stripe, the first subscription is charged and then cancelled without a refund (F1).

**Options:**

- **A.** Allow one open checkout per app. Reuse or expire the pending `billing_checkout_sessions` row (`POST /v1/checkout/sessions/:id/expire`) before creating a new one, with a partial unique index on `(app_id) where status='pending'`.
- **B.** Pass an idempotency key to Stripe (app + plan + 10 min bucket).
- **C.** Refund a replaced subscription automatically when it is under N minutes old.

**Recommendation: A**, with B as a cheap extra layer.

## F3 · Critical · Interleaved purchases leave the app on a dead subscription

**Repro (C02).** Post these events, all with the same `created` second:

1. `checkout.completed(B)`
2. `checkout.completed(C)`: replaces B, so B is cancelled at the provider.
3. Late `subscription.created(B)`: replaces C again and cancels C.
4. `subscription.deleted(B)`.

**End state:**

- The row shows B as `canceled`, and entitlement is `subscription_inactive`.
- C is still the customer's live, billing subscription, but all of C's events are now ignored as `other_subscription`.

**Options:**

- **A.** Fix it at the root with F1-A and F2-A, so only one subscription exists per app.
- **B.** In `decideSubscriptionEvent`, a purchase may replace the current subscription only when its event is strictly newer, or when the current one has ended. Never re-adopt a subscription that has already been replaced: track replaced ids.
- **C.** Before replacing, fetch the subscription from Stripe (`GET /v1/subscriptions/:id`) and act on its live status instead of the event body.

**Recommendation: A + B.** C is a good hardening step for every event (see F5).

## F4 · High · An unpaid checkout wipes out an active plan

**Repro (C01):**

1. Team is active (sub A).
2. `checkout.session.completed` arrives for sub B with `payment_status:"unpaid"` (async payment method, or 3DS pending).
3. Result: the row becomes Business `incomplete`, entitlement is `subscription_inactive`, and A is queued for cancellation.

**Variant (ui-chaos-01.png):**

- When the existing row has no `stripe_subscription_id` (comp or manual grants), the same unpaid event leaves it **active** and moves it to **Business**: a free upgrade.

**Options:**

- **A.** Only a paid purchase may replace: `payment_status ∈ {paid, no_payment_required}`, or a subscription event with `status ∈ {active, trialing}`. Otherwise record the event and change nothing. Handle `checkout.session.async_payment_succeeded`.
- **B.** Keep a separate `pending_subscription_id` and promote it on `invoice.paid`.
- **C.** Disable async payment methods in Checkout (cards only) and treat unpaid as ignore.

**Recommendation: A.** For the variant, also require the current status to be `incomplete` before a checkout event sets `active`.

## F5 · High · Same-second events are not ordered

**Where:** `decideSubscriptionEvent` checks `Date.parse(createdAt) < provider_event_at`. Stripe `created` is in seconds, and Stripe emits several events in the same second.

**Repro:**

- **C03:** `subscription.deleted(t)`, then `subscription.updated(active, t)`. The subscription comes back as active and entitled.
- **C13:** `billing-cancel` (cancel at period end), then a renewal-era `subscription.updated` from the same second. The cancel flag returns to false.

**Options:**

- **A.** Treat `canceled` and `incomplete_expired` as terminal for that subscription id: never move out of them.
- **B.** On ties, re-fetch the subscription from Stripe and apply its live state (the Stripe-recommended pattern).
- **C.** Order ties by a status precedence table.

**Recommendation: A + B.** A is a one-line guard. B removes the ordering problem in general.

## F6 · High · The past_due grace period is unbounded

**Where:** `app_entitlement` uses `coalesce(current_period_end, now()) + 7 days > now()`.

**Repro (C04):**

- **Rolled period:** Stripe moves `current_period_end` forward at renewal, then the payment fails. With `past_due` and the period end at now+30d, entitlement reports `grace` until **2026-12-07**, which is 37 days.
- **Null period end:** with `current_period_end = null`, entitlement reports `grace` forever.
- **UI (ui-chaos-04.png):** shows "Payment overdue · Access continues for 7 days after the period ends" next to "Renews: Never".

**Options:**

- **A.** Add `subscriptions.past_due_since`, set when the status enters past_due. Grace is `past_due_since + 7d`.
- **B.** Compute grace from the failed invoice's `period_start`.
- **C.** Shorten grace to Stripe's own retry schedule and rely on `subscription.updated → unpaid/canceled`.

**Recommendation: A.** It is explicit and testable, and it fixes both the rolled and the null case. Update the UI help text to show the actual grace end date.

## F7 · Medium · No undo for a scheduled cancel

**Repro:**

1. Cancel at period end.
2. The Billing tab (ui-chaos-06.png) hides Cancel and offers only "Choose Starter/Business". The current plan has no button.
3. `billing-checkout` for the same plan returns 200 (C06), which starts a second subscription and the F1 double charge.

**Options:**

- **A.** Add a "Resume subscription" action that calls `POST /v1/subscriptions/:id cancel_at_period_end=false`. While a cancel is scheduled, refuse checkout for the same plan.
- **B.** Send resume through the Customer Portal.
- **C.** Block any checkout while `cancel_at_period_end` is set.

**Recommendation: A.**

## F8 · Medium · Invoice events depend on a removed Stripe field

**Where:** `eventSubscriptionId` reads `object.subscription`. Stripe API 2025-03-31 ("basil") and later move it to `invoice.parent.subscription_details.subscription`. Outgoing calls pin `Stripe-Version: 2024-06-20`, but webhook payloads use the **endpoint's** version. The same applies to `current_period_end`, which the code already falls back for.

**Impact:** on a newer endpoint version, `invoice.paid` and `invoice.payment_failed` resolve to `no_app` or `unknown_subscription` and are silently ignored.

**Options:**

- **A.** Read both shapes, and add a unit test for each version.
- **B.** Pin the endpoint API version in the deployment runbook and check it at startup.
- **C.** Stop using invoice events for state and rely on `customer.subscription.updated`.

**Recommendation: A + B.**

## F9 · Medium · Currency and tax display

**Where:**

- `BillingTab.money()` uses `cents / 100` for every currency, so zero-decimal currencies (JPY, KRW and others) show 100× too small.
- `formatPrice` hard-codes `"USD"` and ignores `plan.currency`.
- Invoices show `amountDue` with no subtotal or tax split. A refunded invoice still shows the full amount due.

**Options:**

- **A.** Format with the currency's minor units (`Intl.NumberFormat(…).resolvedOptions().maximumFractionDigits`) and pass `plan.currency`.
- **B.** Return pre-formatted strings from the backend.
- **C.** Restrict plans to two-decimal currencies with a DB check.

**Recommendation: A**, plus showing `total`/`tax` from Stripe.

## F10 · Medium · Desktop 402 message is misleading

**Where:** `ui/cloud/errors.rs` maps every `ENTITLEMENT_REQUIRED` to "This application's plan does not allow this. The developer must upgrade.", even though the server sends `details.reason` (`subscription_inactive`, `over_allowance`, `no_subscription`).

A runtime user whose app lapsed for non-payment is told to upgrade.

**Capture (`cloud-90-entitlement-lapsed.png`):**

- The bold headline reads "This application's plan does not allow this. The developer must upgrade."
- The server's line "The app's subscription is not active." appears only as small print below it.
- The sidebar still says "Requesting database access…" next to the refusal, a stale status.

**Options:**

- **A.** Choose the message from `details.reason` (lapsed / seats full / no plan).
- **B.** Pass the server `message` through.
- **C.** Show a generic message plus a "Contact the app owner" hint.

**Recommendation: A.**

## F11 · Low · Optimistic writes in billing-cancel

The local patch is keyed only on `app_id`. When a webhook replaces the subscription in between, the patch can mark the **new** subscription cancelled. Not reproduced in 4 racing runs (C08).

**Options:**

- **A.** Make the patch conditional with `.eq("stripe_subscription_id", sub.stripe_subscription_id)`.
- **B.** Drop the optimistic write and wait for the webhook.
- **C.** Wrap it in a SQL function with `FOR UPDATE`.

**Recommendation: A.**

## F12 · Low · Error text shown to users

`billing-fake-complete`, `billing-checkout` and `billing-cancel` return `"sessionId: …"` / `"planId: …"` / `"appId: the app has no active subscription"`, and `ErrorNotice` shows it verbatim.

**Options:**

- **A.** Map `details.field` codes to friendly text on the client.
- **B.** Write user-facing messages on the server and keep `field` in `details`.
- **C.** Leave as is.

**Recommendation: B.**

## F13 · Low · Orphan Stripe customers

`ensureOrgBillingCustomer` creates the Stripe customer before inserting the row. Concurrent first checkouts create two Stripe customers, and only one is stored.

**Options:**

- **A.** Pass a Stripe idempotency key of `org:<id>:customer`.
- **B.** Insert a placeholder row first (claim it), then create the customer.
- **C.** Run a periodic cleanup.

**Recommendation: A.**

---

## Questions

1. **Offline desktop:** Postgres grants are cached in memory for at most 24 h, which is fine. Installs with an embedded datasource (DuckDB/SQLite) keep running indefinitely after cancellation, because the manifest is checked only at install and sync. Is that intended?
2. **Downgrade below seats:** the downgrade is allowed, which locks out **every** runtime user (billing-07, PRD §29). Should the backend refuse it instead of only the UI?
