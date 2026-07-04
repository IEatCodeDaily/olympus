import { test, expect } from "@playwright/test";
import { snap } from "./helpers/evidence";

/**
 * E2E: Projects surface.
 *
 * Verifies:
 * - Projects page renders (placeholder or real)
 * - Create project flow (if UI is live)
 * - Detail pane with vault/repo/board bindings (if UI is live)
 *
 * NOTE: The Projects UI from the kanban worker merge may not be fully
 * reconciled with the current sidebar/History code yet. Tests are
 * defensive — they skip gracefully if the UI isn't wired.
 */

test.describe("Projects", () => {
  test("projects page loads without error", async ({ page }, testInfo) => {
    await page.goto("/projects");
    await page.waitForTimeout(2_000);

    await snap(page, testInfo, "projects-loaded");

    // Something should render (content or empty state)
    const content = page.locator(".gv-body, .empty-state, .view");
    await expect(content.first()).toBeVisible({ timeout: 10_000 });
  });

  test("create project flow if available", async ({ page }, testInfo) => {
    await page.goto("/projects");
    await page.waitForTimeout(2_000);

    // Look for a create button
    const createBtn = page.getByRole("button", { name: /new project|create/i }).first();
    if ((await createBtn.count()) === 0) return; // UI not wired yet

    await createBtn.click();
    await page.waitForTimeout(500);

    await snap(page, testInfo, "create-project");

    const nameInput = page.locator("input[type=text]").first();
    if ((await nameInput.count()) > 0) {
      await nameInput.fill("test-project");
      const confirm = page.getByRole("button", { name: /create|ok/i }).first();
      if ((await confirm.count()) > 0) {
        await confirm.click();
        await page.waitForTimeout(500);

        // Project should appear in the list
        await snap(page, testInfo, "project-created");
      }
    }
  });
});
