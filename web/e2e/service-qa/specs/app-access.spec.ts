/**
 * Contract: per-app console access for organization members and
 * organization-level billing with monthly or annual intervals (PRD §4.5,
 * §20.3): app-access-update, app_capabilities, the RLS policies that read
 * them, billing-checkout with `interval`, and org_billing_customers.
 */
import { callFunction, getServiceClient } from "../clients";
import { expect, test } from "../fixture";
import { recordOutcome } from "../record";
import { createVersion, type TestUser } from "../seed";
import { auditActions, checkoutAndPay, entitlementAs, subscriptionRow } from "./billing-fixtures";

type ErrorBody = { error: { code: string; message: string } };

async function joinOrg(orgId: string, user: TestUser, role: "admin" | "billing" | "member") {
  const { error } = await getServiceClient()
    .from("org_members")
    .insert({ org_id: orgId, user_id: user.user.id, role });
  if (error) throw new Error(`joinOrg failed: ${error.message}`);
}

async function capabilities(user: TestUser, appId: string): Promise<string[]> {
  const { data, error } = await user.client.rpc("app_capabilities", { p_app_id: appId });
  if (error) throw new Error(`app_capabilities failed: ${error.message}`);
  return data as string[];
}

function grant(caller: TestUser, appId: string, userId: string, role: string | null) {
  return callFunction<{ collaborator: { role: string } | null } & Partial<ErrorBody>>(
    "app-access-update",
    { jwt: caller.jwt, body: { appId, userId, role } },
  );
}

test("org admins grant per-app roles; a viewer reads the app and changes nothing", async ({
  cloud,
}) => {
  const owner = await cloud.user();
  const admin = await cloud.user();
  const viewer = await cloud.user();
  const outsider = await cloud.user();
  const org = await cloud.org(owner);
  const app = await cloud.app(org, owner);
  const other = await cloud.app(org, owner);
  await joinOrg(org.id, admin, "admin");
  await joinOrg(org.id, viewer, "member");
  await createVersion(app, owner.user.id, { status: "published" });

  const before = await capabilities(viewer, app.id);
  const beforeApps = (await viewer.client.from("cloud_apps").select("id")).data ?? [];
  const byMember = await grant(viewer, app.id, viewer.user.id, "admin");
  const toOutsider = await grant(admin, app.id, outsider.user.id, "viewer");
  const toOwner = await grant(admin, app.id, owner.user.id, "viewer");
  const granted = await grant(admin, app.id, viewer.user.id, "viewer");

  const after = await capabilities(viewer, app.id);
  const apps = ((await viewer.client.from("cloud_apps").select("id")).data ?? []).map((a) => a.id);
  const versions = (await viewer.client.from("app_versions").select("id").eq("app_id", app.id))
    .data;
  const rename = await viewer.client
    .from("cloud_apps")
    .update({ name: "renamed by viewer" })
    .eq("id", app.id)
    .select("id");
  const entitlement = await entitlementAs(viewer, app.id);

  expect(before).toEqual([]);
  expect(beforeApps).toEqual([]);
  expect(byMember.status).toBe(403);
  expect(toOutsider.status).toBe(422);
  expect(toOwner.status).toBe(422);
  expect(granted.status).toBe(200);
  expect(granted.body.collaborator).toMatchObject({ role: "viewer" });
  expect(after).toEqual(["view"]);
  expect(apps).toEqual([app.id]);
  expect(apps).not.toContain(other.id);
  expect(versions).toHaveLength(1);
  expect(rename.data).toEqual([]);
  expect(entitlement.reason).toBe("no_subscription");

  recordOutcome("app-access-01-viewer-grant", {
    expectations: [
      "A plain org member has no capabilities and sees no apps until an org admin grants per-app access.",
      "app-access-update refuses a non-admin caller (403), a non-org-member target and the app owner (422).",
      "A viewer sees exactly the granted app and its versions, and cannot rename it.",
    ],
    details: { before, after, apps, byMember: byMember.body, toOutsider: toOutsider.body },
  });
});

test("per-app admin manages the app but not credentials; role changes and removals are audited", async ({
  cloud,
}) => {
  const owner = await cloud.user();
  const appAdmin = await cloud.user();
  const org = await cloud.org(owner);
  const app = await cloud.app(org, owner);
  await joinOrg(org.id, appAdmin, "member");

  await grant(owner, app.id, appAdmin.user.id, "admin");
  const caps = await capabilities(appAdmin, app.id);
  const rename = await appAdmin.client
    .from("cloud_apps")
    .update({ name: "renamed by app admin" })
    .eq("id", app.id)
    .select("name");
  const envelopes = await appAdmin.client
    .from("credential_envelopes")
    .select("id")
    .eq("app_id", app.id);
  const changed = await grant(owner, app.id, appAdmin.user.id, "billing");
  const capsAfterChange = await capabilities(appAdmin, app.id);
  const removed = await grant(owner, app.id, appAdmin.user.id, null);
  const capsAfterRemove = await capabilities(appAdmin, app.id);
  const audit = await auditActions({ appId: app.id });

  expect(caps).toEqual(["admin", "billing", "view"]);
  expect(rename.data).toEqual([{ name: "renamed by app admin" }]);
  expect(envelopes.data).toEqual([]);
  expect(changed.body.collaborator).toMatchObject({ role: "billing" });
  expect(capsAfterChange).toEqual(["billing"]);
  expect(removed.body.collaborator).toBeNull();
  expect(capsAfterRemove).toEqual([]);
  expect(audit.map((event) => event.action)).toEqual(
    expect.arrayContaining(["access.grant", "access.change", "access.remove"]),
  );

  recordOutcome("app-access-02-admin-change-remove", {
    expectations: [
      "A per-app admin has admin, billing and view on that app and can change its settings.",
      "Credentials stay owner-only: the per-app admin reads no credential envelopes.",
      "Changing to billing and removing access take effect at once and are audited as access.grant, access.change and access.remove.",
    ],
    details: { caps, capsAfterChange, capsAfterRemove, audit: audit.map((e) => e.action) },
  });
});

test("leaving the organization drops every per-app grant in it", async ({ cloud }) => {
  const owner = await cloud.user();
  const member = await cloud.user();
  const org = await cloud.org(owner);
  const app = await cloud.app(org, owner);
  await joinOrg(org.id, member, "member");
  await grant(owner, app.id, member.user.id, "viewer");

  const leave = await member.client
    .from("org_members")
    .delete()
    .eq("org_id", org.id)
    .eq("user_id", member.user.id);
  const { data: rows } = await getServiceClient()
    .from("app_collaborators")
    .select("user_id")
    .eq("app_id", app.id);

  expect(leave.error).toBeNull();
  expect(rows).toEqual([]);
  expect(await capabilities(member, app.id)).toEqual([]);

  recordOutcome("app-access-03-leave-org-drops-grants", {
    expectations: ["A member who leaves the organization loses every per-app grant in it."],
    details: { rows },
  });
});

test("annual checkout bills the organization's one customer and records the year interval", async ({
  cloud,
}) => {
  const owner = await cloud.user();
  const billing = await cloud.user();
  const viewer = await cloud.user();
  const org = await cloud.org(owner);
  const first = await cloud.app(org, owner);
  const second = await cloud.app(org, owner);
  await joinOrg(org.id, billing, "member");
  await joinOrg(org.id, viewer, "member");
  await grant(owner, second.id, billing.user.id, "billing");
  await grant(owner, second.id, viewer.user.id, "viewer");

  await checkoutAndPay(owner, first.id, "starter", "year");
  const byViewer = await callFunction<ErrorBody>("billing-checkout", {
    jwt: viewer.jwt,
    body: { appId: second.id, planId: "team" },
  });
  await checkoutAndPay(billing, second.id, "team");
  const annual = await subscriptionRow(first.id);
  const monthly = await subscriptionRow(second.id);
  const entitlement = await entitlementAs(owner, first.id);
  const { data: customers } = await getServiceClient()
    .from("org_billing_customers")
    .select("stripe_customer_id")
    .eq("org_id", org.id);
  const sameInterval = await callFunction<ErrorBody>("billing-checkout", {
    jwt: owner.jwt,
    body: { appId: first.id, planId: "starter", interval: "year" },
  });
  const badInterval = await callFunction<ErrorBody>("billing-checkout", {
    jwt: owner.jwt,
    body: { appId: first.id, planId: "starter", interval: "week" },
  });
  const { data: plans } = await owner.client
    .from("plans")
    .select("id, price_cents, annual_price_cents");

  expect(annual).toMatchObject({ plan_id: "starter", billing_interval: "year", status: "active" });
  expect(monthly).toMatchObject({ plan_id: "team", billing_interval: "month", status: "active" });
  expect(entitlement).toMatchObject({ allowed: true, interval: "year" });
  expect(customers).toHaveLength(1);
  expect(annual?.stripe_customer_id).toBe(customers?.[0].stripe_customer_id);
  expect(monthly?.stripe_customer_id).toBe(customers?.[0].stripe_customer_id);
  expect(byViewer.status).toBe(403);
  expect(sameInterval.status).toBe(422);
  expect(badInterval.status).toBe(422);
  for (const plan of plans ?? []) expect(plan.annual_price_cents).toBe(plan.price_cents * 10);

  recordOutcome("app-access-04-annual-org-billing", {
    expectations: [
      "An annual checkout writes a year-interval subscription; entitlement reports interval year.",
      "Both apps of the organization bill to the same single org customer; a per-app billing member can buy, a viewer cannot (403).",
      "Every plan's annual price is ten times the monthly price (two months free).",
    ],
    details: { annual, monthly, customers, plans, byViewer: byViewer.body },
  });
});
