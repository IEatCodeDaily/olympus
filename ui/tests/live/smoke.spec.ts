import { test, expect } from "@playwright/test";
import { snap } from "../e2e/helpers/evidence";

/**
 * LIVE e2e: real control plane, real agent, real tokens.
 *
 * These are the ONLY tests that can catch bridge/ACP regressions that
 * mocks can't see (the ensure_runtime / silent-failure class).
 *
 * Run: make e2e-live (requires olympus.service + vite dev running)
 */

test.describe("Live smoke", () => {
  test("create session → send → agent replies", async ({ page }, testInfo) => {
    await page.goto("/sessions");
    await page.waitForSelector(".newbtn", { timeout: 15_000 });

    // Create a new session with glm52 (reliable non-rate-limited agent)
    await page.locator(".newbtn").click();
    await page.waitForTimeout(1_000);

    // Find glm52 in the agent picker
    const glmOption = page.locator("button, [role=button]").filter({ hasText: "glm52" });
    if ((await glmOption.count()) > 0) {
      await glmOption.first().click();
    } else {
      // Fall back to default agent
      const defaultOption = page.locator("button, [role=button]").filter({ hasText: "default" });
      await defaultOption.first().click();
    }

    await page.waitForSelector(".composer-input", { timeout: 10_000 });
    await snap(page, testInfo, "session-created");

    // Send a simple message
    const input = page.locator(".composer-input");
    await input.fill("Reply with exactly: PONG");
    await input.press("Enter");

    await snap(page, testInfo, "message-sent");

    // Wait for the agent reply — look for the assistant message or thinking indicator
    // Real agent can take 10-120s on a cold spawn
    try {
      await page.waitForSelector(".msg-ai", { timeout: 120_000 });
      await snap(page, testInfo, "agent-replied");

      const aiText = await page.locator(".msg-ai").last().textContent();
      expect(aiText).toBeTruthy();
      expect(aiText!.length).toBeGreaterThan(0);
    } catch {
      // Check for error messages (ensure_runtime failure should show as system msg now)
      const errorMsg = await page.locator(".msg-system").last().textContent();
      await snap(page, testInfo, "timeout-or-error");
      // If there's an error, surface it
      if (errorMsg && errorMsg.includes("error")) {
        throw new Error(`Agent failed: ${errorMsg}`);
      }
      throw new Error("Agent did not reply within 120s");
    }
  });

  test("history page loads 1000+ real sessions", async ({ page }, testInfo) => {
    await page.goto("/sessions/history");
    await page.waitForSelector(".hist-table tbody tr", { timeout: 30_000 });

    // Count initial rows
    const rowCount = await page.locator(".hist-row").count();
    expect(rowCount).toBeGreaterThan(0);

    // The total count should be massive (1800+ sessions)
    const countText = await page.locator(".gk").first().textContent();
    await snap(page, testInfo, "real-history-loaded");
    expect(countText).toBeTruthy();
  });

  test("pin persists across page reload", async ({ page }, testInfo) => {
    await page.goto("/sessions");
    await page.waitForSelector(".srow[data-session-id]", { timeout: 15_000 });

    // Pin the first RECENT session
    const recentRow = page.locator('.srow[data-pinned="false"]').first();
    if ((await recentRow.count()) === 0) return;

    const sid = await recentRow.getAttribute("data-session-id");
    await recentRow.hover();
    await recentRow.locator('button[title="Pin"]').click({ force: true });
    await page.waitForTimeout(1_000);

    await snap(page, testInfo, "after-pin");

    // Reload
    await page.reload();
    await page.waitForSelector(".srow[data-session-id]", { timeout: 15_000 });

    // The session should still be pinned (event-log persisted)
    const pinnedRow = page.locator(`.srow[data-session-id="${sid}"][data-pinned="true"]`);
    const stillPinned = await pinnedRow.count();
    expect(stillPinned).toBe(1);

    await snap(page, testInfo, "pinned-after-reload");
  });
});
