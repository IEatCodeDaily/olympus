import { defineConfig, devices } from "@playwright/test";

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
    baseURL: "http://127.0.0.1:5188",
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
      "VITE_USE_MOCKS=true VITE_API_BASE=http://127.0.0.1:8787 VITE_API_TOKEN=dev-mock-token node_modules/.bin/vite --port 5188 --host 127.0.0.1",
    url: "http://127.0.0.1:5188",
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
