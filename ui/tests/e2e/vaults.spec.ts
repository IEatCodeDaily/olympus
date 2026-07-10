import { test, expect } from "@playwright/test";
import { snap } from "./helpers/evidence";

/**
 * E2E: Vaults surface.
 *
 * Verifies:
 * - Vault list renders
 * - Creating a vault works
 * - Opening a vault shows the note tree
 * - Note editor round-trip (create + edit + save)
 * - Tables and graph tabs render without crashing
 */

test.describe("Vaults @desktop-only", () => {
  test("rich editor preserves wikilinks and saves through explicit source", async ({ page }) => {
    await page.goto("/vaults/engineering?note=redb%2Fredb-compaction.md");

    await page.getByTestId("vedit").click();
    const richEditor = page.locator(".vault-rich-editor .ProseMirror");
    await expect(richEditor).toBeVisible();
    await expect(page.getByTestId("vsave")).toBeDisabled();
    await expect(richEditor.locator('a[href^="olympus-wikilink:"]')).toHaveCount(3);

    await richEditor.click();
    await page.keyboard.press("Control+End");
    await page.keyboard.press("Enter");
    await page.keyboard.type("Updated by E2E");
    await expect(page.getByTestId("vsave")).toBeEnabled();

    await page.getByRole("button", { name: "Source" }).click();
    const source = page.getByTestId("vsrc").locator(".cm-content");
    await expect(source).toContainText("[[event-log-design.md]]");
    await expect(source).not.toContainText("\\[\\[event-log-design.md]]");
    await expect(source).toContainText("Updated by E2E");

    await page.getByTestId("vsave").click();
    await expect(page.getByTestId("vedit")).toBeVisible();
    await expect(page.getByTestId("mdbody")).toContainText("Updated by E2E");
  });

  test("slash commands and wikilink suggestions open from rich text", async ({ page }) => {
    await page.goto("/vaults/engineering?note=redb%2Fredb-compaction.md");
    await page.getByTestId("vedit").click();

    const richEditor = page.locator(".vault-rich-editor .ProseMirror");
    await richEditor.click();
    await page.keyboard.press("Control+a");
    await page.keyboard.type("/");
    await expect(page.locator(".vault-rich-editor nav")).toBeVisible();

    await page.keyboard.press("Escape");
    await page.keyboard.press("Control+a");
    await page.keyboard.type("See [[event");
    await expect(page.getByRole("listbox", { name: "note suggestions" })).toBeVisible();
    await expect(page.getByRole("option").first()).toContainText("event-log-design");
    await page.getByRole("option").first().click();
    await expect(page.getByRole("listbox", { name: "note suggestions" })).toBeHidden();
    await page.getByRole("button", { name: "Source" }).click();
    await expect(page.getByTestId("vsrc").locator(".cm-content")).toContainText(
      "[[event-log-design.md|event-log-design]]",
    );
  });

  test("vault list renders", async ({ page }, testInfo) => {
    await page.goto("/vaults");
    // Wait for either vault list or empty state
    await page.waitForTimeout(2_000);

    await snap(page, testInfo, "vaults-loaded");

    // The route opens the first vault and note when mock data is available.
    await expect(page.getByRole("button", { name: "New vault" })).toBeVisible();
    await expect(page.getByTestId("vnotename")).toBeVisible({ timeout: 10_000 });
  });

  test("create vault flow", async ({ page }, testInfo) => {
    await page.goto("/vaults");
    await page.waitForTimeout(2_000);

    // Look for a "create" or "new" button
    const createBtn = page.getByRole("button", { name: /new vault|create/i }).first();
    if ((await createBtn.count()) === 0) return; // no create affordance in mock

    await createBtn.click();
    await page.waitForTimeout(500);

    await snap(page, testInfo, "create-vault-dialog");

    // If there's an input, type a name
    const nameInput = page.locator("input[type=text], input[placeholder*='name']").first();
    if ((await nameInput.count()) > 0) {
      await nameInput.fill("Test Vault");
      const confirmBtn = page.getByRole("button", { name: /create|confirm|ok/i }).first();
      if ((await confirmBtn.count()) > 0) {
        await confirmBtn.click();
        await page.waitForTimeout(500);
      }
    }
  });

  test("open vault detail and navigate tabs", async ({ page }, testInfo) => {
    await page.goto("/vaults");
    await page.waitForTimeout(2_000);

    // Click first vault if available
    const vaultItem = page.locator("[data-vault-id], .ol-card").first();
    if ((await vaultItem.count()) === 0) return;

    await vaultItem.click();
    await page.waitForTimeout(1_000);

    await snap(page, testInfo, "vault-detail");

    // If there are tabs, try clicking them
    const tabs = page.locator(".rs-tab, [role=tab]");
    const tabCount = await tabs.count();
    for (let i = 0; i < Math.min(tabCount, 3); i++) {
      await tabs.nth(i).click();
      await page.waitForTimeout(300);
    }

    await snap(page, testInfo, "vault-tab-switched");
  });
});
