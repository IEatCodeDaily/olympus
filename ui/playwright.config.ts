import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright config for Olympus UI e2e tests.
 * The dev server (vite + MSW mocks) is started automatically by webServer below.
 */
export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: "list",
  use: {
    baseURL: "http://127.0.0.1:5188",
    trace: "on-first-retry",
    viewport: { width: 1280, height: 800 },
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    // Force mock mode AND the MSW origin so e2e is deterministic regardless of a
    // local .env.local (which may set VITE_API_BASE to a real backend). MSW
    // handlers intercept http://127.0.0.1:8787, so the app must fetch that origin.
    // Distinct port (5188) so it never reuses the operator's dev server on 5173.
    command:
      "VITE_USE_MOCKS=true VITE_API_BASE=http://127.0.0.1:8787 VITE_API_TOKEN=dev-mock-token node_modules/.bin/vite --port 5188 --host 127.0.0.1",
    url: "http://127.0.0.1:5188",
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
