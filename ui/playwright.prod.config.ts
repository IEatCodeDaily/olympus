// Playwright config for the PROD-PARITY tier: tests against the control
// plane's own static UI serving on :8799 (same origin cloudflared sees).
// No webServer — olympus.service must already be running with
// OLYMPUS_UI_DIST set. Cheap and fast; safe to run any time.

import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/prod",
  timeout: 30_000,
  retries: 0,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: process.env.OLYMPUS_PROD_BASE ?? "http://127.0.0.1:8799",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },
  projects: [
    {
      name: "prod-chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
