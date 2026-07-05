// Prod-parity smoke: the UI served STATICALLY from the control plane itself
// (:8799 / OLYMPUS_UI_DIST) — the exact same origin cloudflared traffic sees.
//
// This is different from the mocked e2e suite (Vite + MSW on :5188) and from
// the live smoke (Vite dev proxy on :5177). Here there is NO Vite: index.html,
// assets, and /api/* all come from the single axum server, which is what
// https://olympus.entelechia.cloud serves through the tunnel.
//
// Run with: npx playwright test --config=playwright.prod.config.ts
// Requires: olympus.service running with OLYMPUS_UI_DIST set + ui/dist built.

import { test, expect } from "@playwright/test";

const BASE = process.env.OLYMPUS_PROD_BASE ?? "http://127.0.0.1:8799";

test.describe("prod-parity (static UI from control plane)", () => {
  test("serves index.html at /", async ({ request }) => {
    const res = await request.get(`${BASE}/`);
    expect(res.status()).toBe(200);
    const body = await res.text();
    expect(body).toContain("<!doctype html>");
    expect(body).toContain('<div id="root">');
  });

  test("SPA fallback: unknown path returns index.html not 404", async ({ request }) => {
    const res = await request.get(`${BASE}/vaults/some-vault-id`);
    expect(res.status()).toBe(200);
    const body = await res.text();
    expect(body).toContain("<!doctype html>");
  });

  test("API health reachable on same origin", async ({ request }) => {
    const res = await request.get(`${BASE}/api/health`);
    expect(res.status()).toBe(200);
    const json = await res.json();
    expect(json.status).toBe("ok");
  });

  test("API rejects unauthenticated /api/sessions", async ({ request }) => {
    const res = await request.get(`${BASE}/api/sessions`);
    expect(res.status()).toBe(401);
  });

  test("origin gate: foreign Origin rejected on protected routes", async ({ request }) => {
    // /api/health is deliberately outside the gate (readiness probe); the
    // gate protects everything else.
    const res = await request.get(`${BASE}/api/sessions`, {
      headers: { Origin: "https://evil.example.com" },
    });
    expect(res.status()).toBe(403);
  });

  test("origin gate: tunnel hostname allowed", async ({ request }) => {
    const res = await request.get(`${BASE}/api/health`, {
      headers: { Origin: "https://olympus.entelechia.cloud" },
    });
    expect(res.status()).toBe(200);
  });

  test("UI boots in a real browser against the static server", async ({ page }) => {
    await page.goto(`${BASE}/`);
    // The app shell mounts (topbar with view chips)
    await expect(page.locator("#root")).not.toBeEmpty({ timeout: 15_000 });
  });

  test("static assets load with correct content-type", async ({ request }) => {
    // Pull the index, find the JS bundle path, fetch it.
    const index = await (await request.get(`${BASE}/`)).text();
    const m = index.match(/src="(\/assets\/[^"]+\.js)"/);
    expect(m, "index.html references a JS bundle").toBeTruthy();
    const res = await request.get(`${BASE}${m![1]}`);
    expect(res.status()).toBe(200);
    expect(res.headers()["content-type"]).toContain("javascript");
  });
});
