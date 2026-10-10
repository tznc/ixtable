import { assert, assertEquals, assertThrows } from "jsr:@std/assert@1";
import {
  billingProvider,
  buildFakeEvent,
  fakeProvider,
  parseStripeSignatureHeader,
  signStripePayload,
  verifyStripeSignature,
} from "./billing.ts";
import { hmacSha256Hex } from "./crypto.ts";

Deno.test("Stripe signatures verify for the right secret, body and time window", async () => {
  const payload = JSON.stringify({ id: "evt_1" });
  const header = await signStripePayload(payload, "whsec_test", 1_700_000_000);
  assertEquals(
    header,
    `t=1700000000,v1=${await hmacSha256Hex("whsec_test", `1700000000.${payload}`)}`,
  );
  assert(await verifyStripeSignature(payload, header, "whsec_test", 300, 1_700_000_100));
  assert(!(await verifyStripeSignature(payload, header, "whsec_other", 300, 1_700_000_100)));
  assert(!(await verifyStripeSignature(`${payload} `, header, "whsec_test", 300, 1_700_000_100)));
  assert(!(await verifyStripeSignature(payload, header, "whsec_test", 300, 1_700_001_000)));
  assert(!(await verifyStripeSignature(payload, null, "whsec_test")));
  assert(!(await verifyStripeSignature(payload, "garbage", "whsec_test")));
});

Deno.test("parseStripeSignatureHeader keeps every v1 signature", () => {
  assertEquals(parseStripeSignatureHeader("t=5,v1=aa,v0=zz,v1=bb"), {
    timestamp: 5,
    signatures: ["aa", "bb"],
  });
});

Deno.test("buildFakeEvent produces Stripe-shaped events carrying app metadata", () => {
  const event = buildFakeEvent({
    type: "customer.subscription.updated",
    appId: "app-1234567890",
    planId: "team",
  });
  assertEquals(event.type, "customer.subscription.updated");
  assert(String(event.id).startsWith("evt_fake_"));
  const object = (event.data as { object: Record<string, unknown> }).object;
  assertEquals(object.status, "active");
  assertEquals(object.metadata, {
    app_id: "app-1234567890",
    plan_id: "team",
    billing_interval: "month",
  });
  const checkout = buildFakeEvent({
    type: "checkout.session.completed",
    appId: "a",
    planId: "starter",
    eventId: "evt_x",
  });
  assertEquals(checkout.id, "evt_x");
  assertEquals(
    (checkout.data as { object: Record<string, unknown> }).object.client_reference_id,
    "a",
  );
});

Deno.test("fake provider returns website URLs; billingProvider requires a provider name", async () => {
  const provider = fakeProvider("http://127.0.0.1:3001");
  const checkout = await provider.createCheckout({
    appId: "app",
    orgId: "org",
    planId: "team",
    interval: "year",
    priceId: "price_fake_team_annual",
    userId: "u",
    customerId: "cus_fake_org",
    successUrl: "http://127.0.0.1:3001/ok",
    cancelUrl: "http://127.0.0.1:3001/cancel",
  });
  assert(
    checkout.url.startsWith("http://127.0.0.1:3001/cloud/billing/fake-checkout?session=cs_fake_"),
  );
  assertEquals(new URL(checkout.url).searchParams.get("interval"), "year");
  const customer = await provider.createCustomer({
    orgId: "org",
    orgName: "Acme",
    email: "a@b.co",
  });
  assert(customer.customerId.startsWith("cus_fake_"));
  Deno.env.delete("BILLING_PROVIDER");
  assertThrows(() => billingProvider());
  Deno.env.set("BILLING_PROVIDER", "fake");
  Deno.env.delete("IXTABLE_ALLOW_FAKE_BILLING");
  assertThrows(() => billingProvider(), Error, "IXTABLE_ALLOW_FAKE_BILLING");
  Deno.env.set("IXTABLE_ALLOW_FAKE_BILLING", "1");
  assertEquals(billingProvider().name, "fake");
});
