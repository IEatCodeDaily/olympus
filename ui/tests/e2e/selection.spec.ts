import { test, expect, type Page } from "@playwright/test";

/**
 * E2E: session-list selection + chat-view switching.
 *
 * Verifies the master-detail layout:
 * 1. Clicking a session highlights the row and loads its content in the right panel.
 * 2. Clicking a different session moves the highlight AND swaps the content.
 * 3. Clicking back to the first session restores its content and highlight.
 *
 * This is the regression test for the bug where clicking sessions in the list
 * did not update the selection highlight or the right-panel content.
 */

/** Get the Nth session row (1-indexed for readability). */
async function row(page: Page, n: number) {
  const rows = page.locator(".srow[data-session-id]");
  return rows.nth(n - 1);
}

/** Get the text of the session title shown in the row. */
async function rowTitle(page: Page, n: number): Promise<string> {
  const r = await row(page, n);
  return (await r.locator(".srow-title").textContent()) ?? "";
}

/** Get the unique session id from the row (always distinct, unlike titles). */
async function rowId(page: Page, n: number): Promise<string> {
  const r = await row(page, n);
  return (await r.getAttribute("data-session-id")) ?? "";
}

/** Get the session id currently rendered in the chat panel. */
async function chatId(page: Page): Promise<string> {
  return (await page.locator(".chat-view").getAttribute("data-session-id")) ?? "";
}

/** Get the title displayed in the chat panel header. */
async function chatTitle(page: Page): Promise<string> {
  return (await page.locator(".chat-title").first().textContent()) ?? "";
}

/** Assert exactly one row has the 'on' class and it matches the given row number. */
async function expectSelected(page: Page, n: number) {
  const selectedCount = await page.locator(".srow.on[data-session-id]").count();
  expect(selectedCount).toBe(1);

  const r = await row(page, n);
  await expect(r).toHaveClass(/.*\bon\b.*/);

  // Verify via data-session-id as well (robust against class ordering)
  const selectedId = await page
    .locator(".srow.on[data-session-id]")
    .getAttribute("data-session-id");
  const rid = await r.getAttribute("data-session-id");
  expect(selectedId).toBe(rid);
}

test.describe("Session selection and chat-view switching", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    // Wait for the session list to render (MSW mocks are synchronous but React mounts async)
    await page.waitForSelector(".srow[data-session-id]", { timeout: 10_000 });
    // Ensure we have at least 3 rows
    const count = await page.locator(".srow[data-session-id]").count();
    expect(count).toBeGreaterThanOrEqual(3);
  });

  test("clicking session 1 shows its content in the chat panel @desktop-only", async ({ page }) => {
    const title1 = await rowTitle(page, 1);

    await (await row(page, 1)).click();

    // Chat panel should appear
    await page.waitForSelector(".chat-view", { timeout: 5_000 });

    // Highlight should be on row 1
    await expectSelected(page, 1);

    // Chat panel should show the same title as row 1
    const ct = await chatTitle(page);
    expect(ct).toBe(title1);
  });

  test("clicking session 3 after session 1 switches highlight and content @desktop-only", async ({ page }) => {
    const id1 = await rowId(page, 1);
    const id3 = await rowId(page, 3);

    // Sanity: rows 1 and 3 are different sessions (ids are always unique,
    // unlike titles which can both be "Untitled").
    expect(id3).not.toBe(id1);

    // Click session 1 first
    await (await row(page, 1)).click();
    await page.waitForSelector(".chat-view");
    await expectSelected(page, 1);
    expect(await chatId(page)).toBe(id1);

    // Now click session 3
    await (await row(page, 3)).click();

    // Highlight must move to row 3
    await expectSelected(page, 3);

    // Chat panel content must change to session 3
    const shown = await chatId(page);
    expect(shown).toBe(id3);
    expect(shown).not.toBe(id1);
  });

  test("clicking session 1 → 3 → 1 round-trips correctly @desktop-only", async ({ page }) => {
    const id1 = await rowId(page, 1);
    const id3 = await rowId(page, 3);

    // Step 1: click session 1
    await (await row(page, 1)).click();
    await page.waitForSelector(".chat-view");
    await expectSelected(page, 1);
    expect(await chatId(page)).toBe(id1);

    // Step 2: click session 3
    await (await row(page, 3)).click();
    await expectSelected(page, 3);
    expect(await chatId(page)).toBe(id3);

    // Step 3: click session 1 again — must return
    await (await row(page, 1)).click();
    await expectSelected(page, 1);
    expect(await chatId(page)).toBe(id1);
  });

  test("highlight is on the correct row at every step of a multi-click sequence @desktop-only", async ({ page }) => {
    // Click rows 1, 2, 3, 2 in sequence and verify highlight + content at each step
    for (const n of [1, 2, 3, 2]) {
      const expectedId = await rowId(page, n);
      await (await row(page, n)).click();
      await expectSelected(page, n);
      expect(await chatId(page)).toBe(expectedId);
    }
  });

});

test.describe("Fork from history", () => {
  test("forking an observed session opens the managed fork", async ({ page }) => {
    // Observed sessions live in the History page now (sidebar shows only
    // pinned + 5 recent managed sessions). Open History, filter to an
    // observed channel, and open the first row.
    await page.goto("/sessions/history");
    await page.waitForSelector(".hist-row[data-session-id]", { timeout: 10_000 });

    // Filter to the cli channel (observed in fixtures)
    await page.locator("select[title='Channel']").selectOption("cli");
    await page.waitForTimeout(300);

    // Only rows that are actually observed (fixtures mark some cli rows managed)
    const rows = page.locator('.hist-row[data-managed="false"]');
    expect(await rows.count()).toBeGreaterThan(0);
    await rows.first().click();

    await page.waitForSelector(".chat-view", { timeout: 5_000 });
    const sourceId = await chatId(page);

    // Observed banner + fork affordance
    const forkButton = page.getByRole("button", { name: "Fork to continue" }).first();
    await expect(forkButton).toBeVisible({ timeout: 5_000 });
    await forkButton.click();

    // Confirmation modal → confirm
    const confirmBtn = page.getByRole("button", { name: "Fork to continue" }).last();
    await expect(confirmBtn).toBeVisible({ timeout: 3_000 });
    await confirmBtn.click();

    // The forked session is managed: composer is live, id differs from source
    await expect(page.locator(".composer-input")).toBeVisible({ timeout: 5_000 });
    const forkedId = await chatId(page);
    expect(forkedId).not.toBe(sourceId);
  });
});
