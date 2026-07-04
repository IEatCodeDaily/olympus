import { chromium, expect } from "@playwright/test";

/**
 * globalSetup for live e2e: assert both the vite dev server (:5177) and the
 * control plane (:8799) are running BEFORE any test starts. Fail fast with
 * a clear message instead of cryptic timeout errors.
 */
export default async function globalSetup() {
  const api = await fetch("http://127.0.0.1:8799/api/health");
  expect(api.ok, "Control plane :8799 not healthy — is `systemctl --user status olympus` running?").toBe(true);

  const ui = await fetch("http://127.0.0.1:5177/");
  expect(ui.ok, "Vite dev server :5177 not reachable — start with `cd ui && npm run dev`").toBe(true);
}
