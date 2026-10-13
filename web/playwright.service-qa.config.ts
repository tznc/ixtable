import { defineConfig, devices } from "@playwright/test";

// service-qa: real local Supabase stack only (`npm run service-qa:up --prefix web`).
// - contracts: API-level specs in e2e/service-qa/specs (Auth, PostgREST/RLS,
//   Storage, Edge Functions). No browser, no web server.
// - ui: authenticated pages of the Docusaurus production build in Chromium.
// See .claude/skills/service-qa/SKILL.md.

const argv = process.argv.slice(2);
const projectArgs = argv.flatMap((arg, index) =>
  arg.startsWith("--project=")
    ? [arg.slice(10)]
    : arg === "--project"
      ? [argv[index + 1] ?? ""]
      : [],
);
const needsWebServer = projectArgs.length === 0 || projectArgs.includes("ui");
const SITE = "http://127.0.0.1:3001";

export default defineConfig({
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? "github" : "list",
  globalSetup: "./e2e/service-qa/global-setup.ts",
  outputDir: "./test-results/service-qa",
  projects: [
    {
      name: "contracts",
      testDir: "./e2e/service-qa/specs",
      testMatch: /.*\.spec\.ts$/,
    },
    {
      name: "ui",
      testDir: "./e2e",
      testMatch: [/[\\/]e2e[\\/]service-qa\.spec\.ts$/, /[\\/]service-qa[\\/]ui[\\/].*\.spec\.ts$/],
      use: {
        ...devices["Desktop Chrome"],
        baseURL: SITE,
        trace: "on-first-retry",
        ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH
          ? { launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH } }
          : {}),
      },
    },
  ],
  webServer: needsWebServer
    ? {
        command: "npm run build && npm run serve -- --port 3001 --host 127.0.0.1",
        url: SITE,
        reuseExistingServer: !process.env.CI,
        timeout: 300_000,
        stdout: "pipe",
        stderr: "pipe",
      }
    : undefined,
});
