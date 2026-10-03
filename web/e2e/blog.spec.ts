import { test, expect } from "@playwright/test";

test.describe("blog", () => {
  test("navbar and footer link to the blog", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator(".navbar").getByRole("link", { name: "Blog" })).toHaveAttribute(
      "href",
      "/blog",
    );
    await expect(page.locator("footer").getByRole("link", { name: "Blog" })).toBeVisible();
  });

  test("lists the first post and renders it with a reading time", async ({ page }) => {
    await page.goto("/blog");
    await page.getByRole("link", { name: "Why ixtable" }).first().click();
    await page.waitForURL("**/blog/why-ixtable");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Why ixtable");
    await expect(page.getByText(/min read/)).toBeVisible();
  });
});
