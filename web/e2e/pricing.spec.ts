import { test, expect } from "@playwright/test";
import catalog from "../src/data/plans.json";

test.describe("pricing", () => {
  test("navbar links to the pricing page", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("navigation").getByRole("link", { name: "Pricing" }).first().click();
    await page.waitForURL("**/pricing");
    await expect(page.getByRole("heading", { level: 1, name: "Pricing" })).toBeVisible();
  });

  test("signed-out visitors see the static plan catalog, billed monthly", async ({ page }) => {
    await page.goto("/pricing");
    await expect(page.getByTestId("pricing-terms")).toHaveText(
      "Billed monthly, per app. No free trial.",
    );
    for (const plan of catalog.plans) {
      const card = page.getByRole("region", { name: `${plan.name} plan` });
      await expect(card).toContainText(`$${plan.price_cents / 100}`);
      await expect(card).toContainText(`${plan.runtime_user_allowance} runtime users`);
      await expect(card).toContainText(`${plan.storage_gb} GB archive storage`);
    }
    await expect(page.locator("main")).not.toContainText(/annual|per year|months free/i);
  });

  test("lists what Cloud is not", async ({ page }) => {
    await page.goto("/pricing");
    const main = page.locator("main");
    await expect(main.getByRole("heading", { name: "What ixtable Cloud is not" })).toBeVisible();
    await expect(main).toContainText("does not host your PostgreSQL database");
  });
});
