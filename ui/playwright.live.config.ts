import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright config for Olympus LIVE e2e tests.
 *
 * Runs against the REAL control plane (:8799 via vite proxy :5177).
 * Does NOT start a webServer — asserts the services are running in globalSetup.
 * Spends real tokens. Run via `make e2e-live` (operator/nightly only).
 */
export default defineConfig({
  testDir: "./tests/live",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 180_000, // 3 min per test — agent turns can be slow
  reporter: [
    ["list"],
    ["html", { outputFolder: "playwright-live-report", open: "never" }],
  ],
  use: {
    baseURL: "http://127.0.0.1:5177",
    trace: "retain-on-failure",
    screenshot: "on",
    video: "on",
    viewport: { width: 1280, height: 800 },
  },
  projects: [
    {
      name: "live-chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  globalSetup: require.resolve("./tests/live/global-setup.ts"),
});
