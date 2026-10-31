// Chaos screenshots of the Billing tab (scratch; not for commit).
import { test, expect } from "../fixture";
import { captureOutcome } from "../record";
import { getServiceClient } from "../clients";
import { loginFromHome, openCloudApp } from "../../helpers";
import { seedPublishedApp } from "../../cloud-fixtures";
import { nowSeconds, postWebhook, stripeEvent, subscriptionRow } from "../specs/billing-fixtures";

async function open(page: import("@playwright/test").Page, owner: { email: string; password: string }, name: string) {
  await loginFromHome(page, owner.email, owner.password);
  await openCloudApp(page, name);
  await page.getByRole("tab", { name: "Billing" }).click();
  await expect(page.getByTestId("billing-plan")).toBeVisible();
}

test("UI-C01 unpaid upgrade locks the app out", async ({ page, cloud }) => {
  const { owner, app } = await seedPublishedApp(cloud);
  const ev = stripeEvent({ type: "checkout.session.completed", appId: app.id, planId: "business", subscriptionId: "sub_unpaid_" + Date.now(), created: nowSeconds() + 1 });
  (ev.data as { object: Record<string, unknown> }).object.payment_status = "unpaid";
  await postWebhook(ev);
  await open(page, owner, app.name);
  await captureOutcome(page, "ui-chaos-01-unpaid-upgrade", { expectations: ["Previously active Team plan should still be shown/entitled."], details: { row: await subscriptionRow(app.id) } });
});

test("UI-C04 past_due with no period end shows open-ended grace", async ({ page, cloud }) => {
  const { owner, app } = await seedPublishedApp(cloud);
  await getServiceClient().from("subscriptions").update({ status: "past_due", current_period_end: null }).eq("app_id", app.id);
  await open(page, owner, app.name);
  await captureOutcome(page, "ui-chaos-04-pastdue-null", { expectations: ["Grace must have an end date."] });
});

test("UI-C06 cancel scheduled offers new checkouts, no resume", async ({ page, cloud }) => {
  const { owner, app } = await seedPublishedApp(cloud);
  await getServiceClient().from("subscriptions").update({ cancel_at_period_end: true }).eq("app_id", app.id);
  await open(page, owner, app.name);
  await captureOutcome(page, "ui-chaos-06-cancel-scheduled", { expectations: ["A Resume action should exist; other plans start a new full-price checkout."] });
});
