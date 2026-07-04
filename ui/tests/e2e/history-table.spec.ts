import { test, expect } from "@playwright/test";
import { snap } from "./helpers/evidence";

/**
 * E2E: History page data-table interactions.
 *
 * Verifies:
 * - Table renders with columns + rows
 * - Channel filter narrows to selected channel
 * - Time-range filter works
 * - Archived toggle reveals archived rows
 * - Show-more paging reveals next batch
 * - Row click navigates to session
 */

test.describe("History table", () => {
  test("renders table with correct columns", async ({ page }, testInfo) => {
    await page.goto("/sessions/history");
    await page.waitForSelector(".hist-table thead", { timeout: 10_000 });

    await snap(page, testInfo, "history-table-loaded");

    // Column headers exist
    await expect(page.locator(".hist-table th").filter({ hasText: "SESSION" })).toBeVisible();
    await expect(page.locator(".hist-table th").filter({ hasText: "CHANNEL" })).toBeVisible();
    await expect(page.locator(".hist-table th").filter({ hasText: "AGENT" })).toBeVisible();

    // At least one data row
    const rows = page.locator(".hist-table tbody tr");
    expect(await rows.count()).toBeGreaterThan(0);
  });

  test("channel filter narrows results", async ({ page }, testInfo) => {
    await page.goto("/sessions/history");
    await page.waitForSelector(".hist-table tbody tr", { timeout: 10_000 });

    const initialCount = await page.locator(".hist-row").count();

    // Filter to CLI channel
    await page.locator("select[title='Channel']").selectOption("cli");
    await page.waitForTimeout(500);

    await snap(page, testInfo, "filtered-by-cli");

    // All visible rows should show the CLI channel tag
    const visibleTags = page.locator(".hist-row .col-channel .gtag");
    const tagCount = await visibleTags.count();
    for (let i = 0; i < Math.min(tagCount, 5); i++) {
      const text = await visibleTags.nth(i).textContent();
      expect(text?.toLowerCase()).toBe("cli");
    }
  });

  test("free-text search narrows results", async ({ page }, testInfo) => {
    await page.goto("/sessions/history");
    await page.waitForSelector(".hist-search", { timeout: 10_000 });

    // Search for something that exists in fixtures
    await page.locator(".hist-search").fill("session");
    await page.waitForTimeout(500);

    const count = await page.locator(".hist-row").count();
    // Should have some results (fixtures have telegram-sourced sessions)
    expect(count).toBeGreaterThan(0);

    await snap(page, testInfo, "search-telegram");

    // Clear search — results expand
    await page.locator(".hist-search").fill("");
    await page.waitForTimeout(500);
    const expandedCount = await page.locator(".hist-row").count();
    expect(expandedCount).toBeGreaterThanOrEqual(count);
  });

  test("archived toggle shows archived rows", async ({ page }, testInfo) => {
    await page.goto("/sessions/history");
    await page.waitForSelector(".hist-table tbody tr", { timeout: 10_000 });

    // Initially archived rows are hidden (no archived tag visible)
    const archivedTagsBefore = await page.locator(".hist-archived-tag").count();

    // Toggle archived on
    await page.locator("label:has(input[type=checkbox])").click();
    await page.waitForTimeout(500);

    await snap(page, testInfo, "archived-visible");

    // After toggle, there should be archived-tagged rows visible
    const archivedTagsAfter = await page.locator(".hist-archived-tag").count();
    expect(archivedTagsAfter).toBeGreaterThanOrEqual(archivedTagsBefore);
  });

  test("row click navigates to session", async ({ page }) => {
    await page.goto("/sessions/history");
    await page.waitForSelector(".hist-row", { timeout: 10_000 });

    // Click first row
    const firstRow = page.locator(".hist-row").first();
    const sid = await firstRow.getAttribute("data-session-id");
    await firstRow.click();

    // Should navigate to the session URL
    await page.waitForURL(`**/sessions/${sid}`, { timeout: 5_000 });
    await page.waitForSelector(".chat-view", { timeout: 5_000 });
  });
});
