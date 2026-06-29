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
    // Force mock mode so e2e is deterministic regardless of a local .env.local
    // that may point the dev server at a real backend. Use a distinct port so it
    // never reuses a real-backend dev server the operator is running on 5173.
    command: "VITE_USE_MOCKS=true node_modules/.bin/vite --port 5188 --host 127.0.0.1",
    url: "http://127.0.0.1:5188",
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
