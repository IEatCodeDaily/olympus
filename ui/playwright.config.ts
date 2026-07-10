import { defineConfig, devices } from "@playwright/test";

const e2ePort = Number(process.env.OLYMPUS_E2E_PORT ?? "5188");
const e2eOrigin = `http://127.0.0.1:${e2ePort}`;

/**
 * Playwright config for Olympus UI e2e tests.
 * The dev server (vite + MSW mocks) is started automatically by webServer below.
 *
 * Evidence policy: video + screenshots on EVERY test (not just failures), so
 * every run produces a complete visual record. The evidence-bundle.sh script
 * post-processes these into test-evidence/<ts>/ (mp4 + contact sheet + report).
 */
export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  outputDir: "test-results",
  reporter: [
    ["list"],
    ["html", { outputFolder: "playwright-report", open: "never" }],
    ["json", { outputFile: "test-results/results.json" }],
  ],
  use: {
    baseURL: e2eOrigin,
    trace: "retain-on-failure",
    screenshot: "on",
    video: "on",
    viewport: { width: 1280, height: 800 },
  },
  projects: [
    {
      name: "chromium-desktop",
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "mobile-chrome",
      use: { ...devices["Pixel 7"] },
      grepInvert: /@desktop-only/,
    },
  ],
  webServer: {
    command:
      `VITE_USE_MOCKS=true VITE_API_BASE=http://127.0.0.1:8787 node_modules/.bin/vite --port ${e2ePort} --host 127.0.0.1`,
    url: e2eOrigin,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
