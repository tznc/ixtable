import { test, expect } from "../fixture";
import { captureOutcome } from "../record";
import { loginFromHome, openCloudApp } from "../../helpers";
import { seedPublishedApp } from "../../cloud-fixtures";

test("owner grants an org member the Viewer role and the viewer sees a read-only console", async ({
  page,
  browser,
  baseURL,
  cloud,
}) => {
  const { owner, app } = await seedPublishedApp(cloud);
  const viewer = await cloud.user();
  const { error } = await cloud.admin
    .from("org_members")
    .insert({ org_id: app.org_id, user_id: viewer.user.id, role: "member" });
  if (error) throw new Error(`join org: ${error.message}`);

  await loginFromHome(page, owner.email, owner.password);
  await openCloudApp(page, app.name);
  await page.getByRole("tab", { name: "Access" }).click();
  const form = page.getByRole("form", { name: "Grant app access" });
  await form.getByLabel("Organization member").selectOption({ label: viewer.email });
  await form.getByLabel("App role").selectOption("viewer");
  await form.getByRole("button", { name: "Grant access" }).click();
  const row = page.getByRole("row").filter({ hasText: viewer.email });
  await expect(row).toContainText("Reads the console; changes nothing");
  await expect(row.getByLabel(`App role for ${viewer.email}`)).toHaveValue("viewer");
  await page.evaluate(() => window.scrollTo(0, 0));
  await captureOutcome(page, "ui-app-access-01-owner-grant", {
    expectations: [
      "The Access tab lists the owner as Developer/Owner and the new member with the Viewer role.",
      "The Grant access form is still shown for the owner.",
    ],
  });

  const context = await browser.newContext({ baseURL });
  const viewerPage = await context.newPage();
  try {
    await loginFromHome(viewerPage, viewer.email, viewer.password);
    await openCloudApp(viewerPage, app.name);
    const tabs = viewerPage.getByRole("tablist", { name: "App sections" });
    await expect(tabs.getByRole("tab", { name: "Access" })).toBeVisible();
    for (const name of ["Billing", "Settings", "Backups", "Credentials"]) {
      await expect(tabs.getByRole("tab", { name: name })).toHaveCount(0);
    }
    await tabs.getByRole("tab", { name: "Runtime users" }).click();
    await expect(viewerPage.getByRole("form", { name: "Invite runtime user" })).toHaveCount(0);
    await expect(viewerPage.getByRole("button", { name: /Invite|Revoke|Remove/ })).toHaveCount(0);
    await captureOutcome(viewerPage, "ui-app-access-02-viewer-readonly", {
      expectations: [
        "Signed in as the viewer, the app tabs are Overview, Runtime users, Roles, Versions, Installations, Audit history and Access only.",
        "The Runtime users tab lists users with no invite form and no mutating buttons.",
      ],
    });
  } finally {
    await context.close();
  }
});

test("billing tab and pricing page show monthly prices only", async ({ page, cloud }) => {
  const { owner, app } = await seedPublishedApp(cloud);
  await loginFromHome(page, owner.email, owner.password);
  await openCloudApp(page, app.name);
  await page.getByRole("tab", { name: "Billing" }).click();
  await expect(page.getByText(/\$19(\.00)? per month/).first()).toBeVisible();
  await expect(page.getByText(/per year|Annual/)).toHaveCount(0);
  await expect(page.getByText("Loading invoices...")).toHaveCount(0);
  await captureOutcome(page, "ui-app-access-03-billing-monthly", {
    expectations: [
      "The Billing tab shows the Team plan and monthly plan cards (Starter $19 per month).",
      "There is no billing interval toggle and no annual price.",
    ],
  });

  await page.goto("/pricing");
  const starter = page.getByRole("region", { name: "Starter plan" });
  await expect(starter).toContainText(/\$19(\.00)? per month/);
  await expect(page.getByTestId("pricing-terms")).toHaveText(
    "Billed monthly, per app. No free trial.",
  );
  await expect(page.getByText(/per year|Annual/)).toHaveCount(0);
  await captureOutcome(page, "ui-app-access-04-pricing-monthly", {
    expectations: [
      "The Pricing page says billing is monthly, per app, with no free trial.",
      "Starter, Team and Business cards show monthly prices and no annual option.",
    ],
  });
});
