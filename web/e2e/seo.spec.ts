import { readFileSync } from "node:fs";
import path from "node:path";
import { test, expect } from "@playwright/test";

interface ManifestPage {
  path: string;
  canonical: string;
  title: string;
  description: string;
}

const manifest = JSON.parse(
  readFileSync(path.join(process.cwd(), "..", "marketing", "seo", "site-manifest.json"), "utf8"),
) as { siteUrl: string; pages: ManifestPage[] };

test.describe("SEO manifest matches rendered pages", () => {
  for (const entry of manifest.pages) {
    test(`${entry.path} renders the manifest title, description, and canonical`, async ({
      page,
    }) => {
      expect(entry.title.length).toBeGreaterThanOrEqual(20);
      expect(entry.title.length).toBeLessThanOrEqual(65);
      expect(entry.description.length).toBeGreaterThanOrEqual(70);
      expect(entry.description.length).toBeLessThanOrEqual(170);
      expect(entry.canonical).toBe(`${manifest.siteUrl}${entry.path}`);

      // Fetch the static HTML so the check sees what crawlers see before hydration.
      const response = await page.request.get(entry.path);
      expect(response.ok()).toBe(true);
      const html = await response.text();
      await page.setContent(html);
      await expect(page).toHaveTitle(entry.title);
      await expect(page.locator('meta[name="description"]')).toHaveAttribute(
        "content",
        entry.description,
      );
      await expect(page.locator('link[rel="canonical"]')).toHaveAttribute(
        "href",
        entry.canonical,
      );
    });
  }
});
