import { test, expect } from "@playwright/test";

test.describe("landing calls to action", () => {
  test("waitlist form is the primary call to action", async ({ page }) => {
    await page.goto("/");
    const form = page.getByRole("form", { name: "Join the waitlist" }).first();
    await expect(form).toBeVisible();
    await expect(form.getByLabel("Email address")).toBeVisible();
    await expect(form.getByRole("button", { name: "Join the waitlist" })).toBeVisible();
  });

  test("links to the GitHub repository", async ({ page }) => {
    await page.goto("/");
    const github = page.getByRole("link", { name: "View on GitHub" });
    await expect(github).toBeVisible();
    await expect(github).toHaveAttribute("href", "https://github.com/ixtable/ixtable");
  });

  test("download is visibly disabled and not a link", async ({ page }) => {
    await page.goto("/");
    const download = page.getByText(/Download: signed installers coming soon/);
    await expect(download).toBeVisible();
    await expect(download).toHaveAttribute("aria-disabled", "true");
    expect(await download.evaluate((element) => element.closest("a"))).toBeNull();
  });

  test("hero no longer sends visitors to the login page", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("main a[href='/login']")).toHaveCount(0);
    await expect(page.getByTestId("navbar-login-link")).toBeVisible();
  });
});
