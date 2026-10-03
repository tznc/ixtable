import { expect, test, type Page } from "@playwright/test";
import { mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";

const outputDir = path.join(process.cwd(), "web-qa", ".generated");

async function capture(
  page: Page,
  name: string,
  expectations: string[],
  fullPage = true,
): Promise<void> {
  await mkdir(outputDir, { recursive: true });
  await page.locator("img").evaluateAll(async (images: HTMLImageElement[]) => {
    for (const image of images) image.loading = "eager";
    await Promise.all(
      images.map((image) =>
        image.complete
          ? image.naturalWidth > 0
            ? Promise.resolve()
            : Promise.reject(new Error(`Failed to load ${image.src}`))
          : new Promise<void>((resolve, reject) => {
              image.addEventListener("load", () => resolve(), { once: true });
              image.addEventListener(
                "error",
                () => reject(new Error(`Failed to load ${image.src}`)),
                { once: true },
              );
            }),
      ),
    );
  });
  await page.screenshot({ path: path.join(outputDir, `${name}.png`), fullPage });
  await writeFile(
    path.join(outputDir, `${name}.json`),
    `${JSON.stringify({ name, capturedAt: new Date().toISOString(), expectations }, null, 2)}\n`,
  );
}

// An empty React Flow canvas (background dots, zoom controls, minimap) scores
// under 200 dark pixels; the generated 1280x800 screenshots with four table
// nodes score about 1,700. The bound sits well clear of both, so it catches a
// blank canvas without tracking font or contrast changes in the app.
const MIN_FLOW_DARK_PIXELS = 1_000;

async function darkPixelsInFlowCanvas(page: Page, imageName: RegExp): Promise<number> {
  const image = page.getByRole("img", { name: imageName });
  await expect(image).toBeVisible();
  await image.evaluate(async (element: HTMLImageElement) => {
    element.loading = "eager";
    await element.decode();
  });
  return image.evaluate((element: HTMLImageElement) => {
    const canvas = document.createElement("canvas");
    canvas.width = element.naturalWidth;
    canvas.height = element.naturalHeight;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) throw new Error("Canvas context is unavailable.");
    context.drawImage(element, 0, 0);
    // The workspace screenshots reserve the upper-right area for React Flow.
    // Count dark pixels there so a decoded but empty canvas cannot pass QA.
    const x = Math.floor(canvas.width * 0.17),
      y = Math.floor(canvas.height * 0.17);
    const width = Math.floor(canvas.width * 0.81),
      height = Math.floor(canvas.height * 0.35);
    const pixels = context.getImageData(x, y, width, height).data;
    let dark = 0;
    for (let index = 0; index < pixels.length; index += 4) {
      if (
        pixels[index] < 180 &&
        pixels[index + 1] < 180 &&
        pixels[index + 2] < 180 &&
        pixels[index + 3] > 0
      )
        dark++;
    }
    return dark;
  });
}

test.describe("web QA", () => {
  test("documentation screenshots show current generated assets", async ({ page }) => {
    // Drop old captures first so a failed run cannot leave stale proof behind.
    // This lives in the test, not beforeAll: fullyParallel runs beforeAll once
    // per worker, and another worker would delete this test's fresh captures.
    await Promise.all(
      ["docs-01-overview-desktop", "docs-02-data-view-desktop", "docs-03-overview-mobile"].flatMap(
        (name) => [
          rm(path.join(outputDir, `${name}.png`), { force: true }),
          rm(path.join(outputDir, `${name}.json`), { force: true }),
        ],
      ),
    );
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.goto("/docs/");
    await expect(page.getByRole("heading", { level: 1, name: "Overview" })).toBeVisible();
    await expect(page.getByRole("img", { name: /project open in Data view/i })).toBeVisible();
    await capture(page, "docs-01-overview-desktop", [
      "The overview page shows the current Data view screenshot.",
      "Documentation navigation and article content are fully visible.",
    ]);

    await page.goto("/docs/concepts/data-view");
    await expect(page.getByRole("heading", { level: 1, name: "Data view" })).toBeVisible();
    expect(await darkPixelsInFlowCanvas(page, /full Data view workspace/i)).toBeGreaterThan(
      MIN_FLOW_DARK_PIXELS,
    );
    expect(await darkPixelsInFlowCanvas(page, /new order entered/i)).toBeGreaterThan(
      MIN_FLOW_DARK_PIXELS,
    );
    const flowImage = page.getByRole("img", { name: /Customers, Products, Orders/i });
    await expect(flowImage).toBeVisible();
    await flowImage.evaluate(async (image: HTMLImageElement) => {
      image.loading = "eager";
      await image.decode();
    });
    await flowImage.scrollIntoViewIfNeeded();
    await page.waitForTimeout(100);
    await capture(
      page,
      "docs-02-data-view-desktop",
      [
        "The Data view article is positioned at the React Flow close-up.",
        "All four table nodes and their relationships are readable in the article.",
      ],
      false,
    );

    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/docs/");
    await expect(page.getByRole("heading", { level: 1, name: "Overview" })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      390,
    );
    await capture(page, "docs-03-overview-mobile", [
      "The overview article and current screenshot fit a mobile viewport.",
      "The page has no horizontal scrolling or clipped content.",
    ]);
  });

  test("landing page communicates the product clearly on desktop", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.goto("/");

    await expect(page.getByRole("heading", { level: 1 })).toHaveText(
      /Build the app\s*your business needs\./,
    );
    await expect(
      page.getByRole("button", { name: "Join the waitlist" }).first(),
    ).toBeVisible();
    await expect(page.getByLabel("Preview of the ixtable app builder")).toBeVisible();
    await expect(
      page.getByRole("heading", { name: /More capable than a spreadsheet/i }),
    ).toBeVisible();
    await expect(page.getByRole("img", { name: /DataView TableView/i })).toBeVisible();
    expect(await darkPixelsInFlowCanvas(page, /DataView TableView/i)).toBeGreaterThan(
      MIN_FLOW_DARK_PIXELS,
    );
    await expect(page.getByRole("img", { name: /production form builder/i })).toBeVisible();
    await expect(page.getByRole("img", { name: /Published inventory app/i })).toBeVisible();
    const workspacePhoto = page.getByRole("img", { name: /tidy workspace/i });
    await expect(workspacePhoto).toBeVisible();
    await expect
      .poll(() => workspacePhoto.evaluate((image: HTMLImageElement) => image.naturalWidth))
      .toBeGreaterThan(0);
    await expect(page.locator("main")).not.toHaveCSS("overflow-x", "scroll");

    await capture(page, "landing-01-desktop", [
      "The hero, waitlist form, GitHub link, disabled download note, and product preview are visible.",
      "Feature sections form a clear editorial rhythm without clipped or overlapping content.",
      "The final call to action is visible at the bottom of the page.",
    ]);
  });

  test("navbar shows the ixtable mark in light and dark themes", async ({ page }) => {
    for (const [colorScheme, file] of [
      ["light", "logo.svg"],
      ["dark", "logo-dark.svg"],
    ] as const) {
      await page.emulateMedia({ colorScheme });
      await page.setViewportSize({ width: 1440, height: 1000 });
      await page.goto("/");
      const logo = page
        .getByRole("navigation", { name: "Main" })
        .getByRole("img", { name: "ixtable" })
        .filter({ visible: true });
      await expect(logo).toHaveAttribute("src", new RegExp(`/img/${file}$`));
      await expect
        .poll(() => logo.evaluate((image: HTMLImageElement) => image.naturalWidth))
        .toBeGreaterThan(0);
      await expect(page.locator('link[rel="icon"]')).toHaveAttribute(
        "href",
        /\/img\/favicon\.svg$/,
      );
      await capture(
        page,
        `landing-03-navbar-${colorScheme}`,
        [
          `The navbar shows the ixtable "ix" tile in the ${colorScheme} theme, next to the ixtable title.`,
          "The mark is crisp, centered, and contrasts with the navbar background.",
        ],
        false,
      );
    }
  });

  test("landing page remains usable on mobile", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");

    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Join the waitlist" }).first(),
    ).toBeVisible();
    await expect(page.getByLabel("Preview of the ixtable app builder")).toBeVisible();
    const pageWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(pageWidth).toBeLessThanOrEqual(390);

    await capture(page, "landing-02-mobile", [
      "Hero copy and calls to action fit the viewport without horizontal scrolling.",
      "The product preview and feature sections collapse into a readable single column.",
      "Text remains legible and no content is visibly clipped.",
    ]);
  });

});

const GUIDE_PAGES = [
  { path: "/docs/guides/queries", title: "Queries" },
  { path: "/docs/guides/runtime-forms", title: "Runtime forms" },
  { path: "/docs/guides/reports", title: "Reports" },
  { path: "/docs/guides/dashboards", title: "Dashboards" },
  { path: "/docs/guides/automation", title: "Automation" },
  { path: "/docs/guides/roles", title: "Roles and permissions" },
  { path: "/docs/guides/migrations", title: "Migrations" },
  { path: "/docs/guides/runtime-bundles", title: "Runtime bundles" },
  { path: "/docs/reference/expressions", title: "Expressions" },
] as const;

test.describe("web QA: guides", () => {
  test("every guide is in the sidebar and fits desktop and mobile", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.goto("/docs/");
    const sidebar = page.getByRole("navigation", { name: "Docs sidebar" });
    for (const category of ["Guides", "Reference"]) {
      const toggle = sidebar.getByRole("button", { name: category, exact: true });
      if ((await toggle.getAttribute("aria-expanded")) === "false") await toggle.click();
    }
    for (const guide of GUIDE_PAGES) {
      await expect(sidebar.getByRole("link", { name: guide.title, exact: true })).toBeVisible();
    }

    await sidebar.getByRole("link", { name: "Roles and permissions", exact: true }).click();
    await expect(
      page.getByRole("heading", { level: 1, name: "Roles and permissions" }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { level: 2, name: "PostgreSQL limitation" }),
    ).toBeVisible();
    await capture(page, "guides-01-roles-desktop", [
      "The Roles and permissions page is open with the Guides category expanded in the sidebar.",
      "The PostgreSQL limitation section and the role tables render without clipping.",
    ]);

    await page.goto("/docs/reference/expressions");
    await capture(page, "guides-02-expressions-desktop", [
      "The expressions reference shows its operator, function, and format tables.",
      "Code samples and tables are readable and none overflow the article column.",
    ]);

    await page.setViewportSize({ width: 390, height: 844 });
    for (const guide of GUIDE_PAGES) {
      await page.goto(guide.path);
      await expect(page.getByRole("heading", { level: 1, name: guide.title })).toBeVisible();
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth),
        `${guide.path} scrolls horizontally`,
      ).toBeLessThanOrEqual(390);
    }
    await page.goto("/docs/guides/automation");
    await capture(page, "guides-03-automation-mobile", [
      "The automation guide fits a phone width with no horizontal page scroll.",
      "Wide tables scroll inside their own box instead of widening the page.",
    ]);
  });
});

test.describe("web QA: account pages", () => {
  test("pricing and log in pages fit a phone screen", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    await page.getByRole("button", { name: "Toggle navigation bar" }).click();
    await page
      .getByRole("navigation", { name: "Main" })
      .getByRole("link", { name: "Pricing" })
      .click();
    await expect(page.getByRole("heading", { level: 1, name: "Pricing" })).toBeVisible();
    const starter = await page.getByRole("region", { name: "Starter plan" }).boundingBox();
    expect(starter?.width ?? 0).toBeLessThanOrEqual(390);
    await capture(page, "cloud-01-pricing-mobile", [
      "The three plan cards stack in one column and each names its runtime user allowance.",
      "The page has no horizontal scrolling or clipped text.",
    ]);
    await page.getByRole("button", { name: "Toggle navigation bar" }).click();
    await page
      .getByRole("navigation", { name: "Main" })
      .getByRole("link", { name: "Log in" })
      .click();
    await expect(page.getByRole("button", { name: "Continue with Microsoft" })).toBeVisible();
    await capture(page, "cloud-02-login-mobile", [
      "The log in form, the Google and Microsoft buttons, and the not-configured note fit the phone width.",
      "Form controls show labels and are large enough to tap.",
    ]);
  });
});
