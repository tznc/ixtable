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

test("billing tab and pricing page show annual prices", async ({ page, cloud }) => {
  const { owner, app } = await seedPublishedApp(cloud);
  await loginFromHome(page, owner.email, owner.password);
  await openCloudApp(page, app.name);
  await page.getByRole("tab", { name: "Billing" }).click();
  await page.getByText("Annual (2 months free)").first().click();
  await expect(page.getByText(/\$190(\.00)? per year/).first()).toBeVisible();
  await expect(page.getByText("2 months free", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("Loading invoices...")).toHaveCount(0);
  await captureOutcome(page, "ui-app-access-03-billing-annual", {
    expectations: [
      "The Billing tab shows the Team plan, with the interval toggle on Annual.",
      "Plan cards show annual prices (Starter $190 per year) and 2 months free.",
    ],
  });

  await page.goto("/pricing");
  await page.getByText("Annual (2 months free)").first().click();
  const starter = page.getByRole("region", { name: "Starter plan" });
  await expect(starter).toContainText(/\$190(\.00)? per year/);
  await expect(starter).toContainText("2 months free");
  // Let the toggle's colour transition settle before the capture.
  await page.waitForTimeout(600);
  await captureOutcome(page, "ui-app-access-04-pricing-annual", {
    expectations: [
      "The Pricing page shows the Annual toggle selected.",
      "Starter, Team and Business cards show annual prices and 2 months free.",
    ],
  });
});
