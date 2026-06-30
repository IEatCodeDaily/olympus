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

// Navigation uses Playwright's configured baseURL (see playwright.config.ts),
// so the e2e server port can change without editing every test.

/** Get the Nth session row (1-indexed for readability). */
async function row(page: Page, n: number) {
  const rows = page.locator(".session-row");
  return rows.nth(n - 1);
}

/** Get the text of the session title shown in the row. */
async function rowTitle(page: Page, n: number): Promise<string> {
  const r = await row(page, n);
  return (await r.locator(".row-title").textContent()) ?? "";
}

/** Get the unique session id from the row (always distinct, unlike titles). */
async function rowId(page: Page, n: number): Promise<string> {
  const r = await row(page, n);
  return (await r.getAttribute("data-session-id")) ?? "";
}

/** Get the session id currently rendered in the chat panel header. */
async function chatId(page: Page): Promise<string> {
  return (await page.locator(".chat-view").getAttribute("data-session-id")) ?? "";
}

/** Get the title displayed in the chat panel header. */
async function chatTitle(page: Page): Promise<string> {
  return (await page.locator(".chat-title span").first().textContent()) ?? "";
}

/** Assert exactly one row has the 'selected' class and it matches the given row number. */
async function expectSelected(page: Page, n: number) {
  const selectedCount = await page.locator(".session-row.selected").count();
  expect(selectedCount).toBe(1);

  const r = await row(page, n);
  await expect(r).toHaveClass(/.*\bselected\b.*/);

  // Verify via data-session-id as well (robust against class ordering)
  const selectedId = await page
    .locator(".session-row.selected")
    .getAttribute("data-session-id");
  const rowId = await r.getAttribute("data-session-id");
  expect(selectedId).toBe(rowId);
}

test.describe("Session selection and chat-view switching", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    // Wait for the session list to render (MSW mocks are synchronous but React mounts async)
    await page.waitForSelector(".session-row", { timeout: 10_000 });
    // Ensure we have at least 3 rows
    const count = await page.locator(".session-row").count();
    expect(count).toBeGreaterThanOrEqual(3);
  });

  test("clicking session 1 shows its content in the chat panel", async ({ page }) => {
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

  test("clicking session 3 after session 1 switches highlight and content", async ({ page }) => {
    const id1 = await rowId(page, 1);
    const id3 = await rowId(page, 3);

    // Sanity: rows 1 and 3 are different sessions (ids are always unique,
    // unlike titles which can both be "(no title)").
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

  test("clicking session 1 → 3 → 1 round-trips correctly", async ({ page }) => {
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

  test("highlight is on the correct row at every step of a multi-click sequence", async ({ page }) => {
    // Click rows 1, 2, 3, 2 in sequence and verify highlight + content at each step
    for (const n of [1, 2, 3, 2]) {
      const expectedId = await rowId(page, n);
      await (await row(page, n)).click();
      await expectSelected(page, n);
      expect(await chatId(page)).toBe(expectedId);
    }
  });

  test("forking an observed session opens the managed fork", async ({ page }) => {
    let sourceId = "";
    let forkButton = page.getByRole("button", { name: "Fork to continue" });

    for (let n = 1; n <= 6; n += 1) {
      await (await row(page, n)).click();
      await page.waitForSelector(".chat-view", { timeout: 5_000 });
      if ((await forkButton.count()) > 0) {
        sourceId = await chatId(page);
        break;
      }
    }

    expect(sourceId).not.toBe("");
    await expect(forkButton).toBeEnabled();
    await forkButton.click();

    // A forked session is managed (steerable): the composer input is present,
    // and it shows a managed status badge — "running" (active) when freshly
    // forked within the recency window, or "idle" otherwise.
    await expect(page.locator(".composer-input")).toBeVisible();
    await expect(
      page.locator(".chat-live-badge, .chat-managed-badge")
    ).toBeVisible();
    const forkedId = await chatId(page);
    expect(forkedId).not.toBe(sourceId);
    await expect(page.locator(".chat-view")).toHaveAttribute("data-session-id", forkedId);
  });
});
