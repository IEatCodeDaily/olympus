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
    const noteSuggestions = page.getByRole("listbox", { name: "note suggestions" });
    await expect(noteSuggestions).toBeVisible();
    await expect(noteSuggestions.getByRole("option").first()).toContainText("event-log-design");
    await noteSuggestions.getByRole("option").first().click();
    await expect(noteSuggestions).toBeHidden();
    await page.getByRole("button", { name: "Source" }).click();
    await expect(page.getByTestId("vsrc").locator(".cm-content")).toContainText(
      "[[event-log-design.md|event-log-design]]",
    );
  });

  test("sidebar follows the vault workflow and folder index opens", async ({ page }, testInfo) => {
    await page.goto("/vaults");
    await expect(page.getByRole("button", { name: /Vault Engineering/ })).toBeVisible();
    await expect(page.getByRole("button", { name: "New Note" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Graph View" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Table View" })).toBeVisible();
    await expect(page.getByRole("tree", { name: "Vault files" })).toBeVisible();
    await snap(page, testInfo, "vaults-loaded");

    await page.getByRole("treeitem").filter({ hasText: "redb" }).first().getByRole("button").first().click();
    await expect(page.getByTestId("vnotename")).toContainText("index.md");
    await expect(page.getByRole("tab", { name: "redb" })).toBeVisible();
    await expect(page).toHaveURL(/note=redb%2Findex\.md/);
  });

  test("creates a configured GitHub vault", async ({ page }, testInfo) => {
    await page.goto("/vaults");
    await page.getByRole("button", { name: /Vault Engineering/ }).click();
    await page.getByRole("menuitem", { name: "Create vault…" }).click();
    await snap(page, testInfo, "create-vault-dialog");
    const dialog = page.getByRole("dialog", { name: "Create vault" });
    await dialog.getByLabel("Vault name").fill("Test Vault");
    await dialog.getByRole("textbox", { name: "Repository", exact: true }).fill("IEatCodeDaily/test-vault");
    await dialog.getByRole("button", { name: "Create vault" }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByRole("button", { name: /Vault Test Vault/ })).toBeVisible();
    await expect(page.getByRole("button", { name: "New Note" })).toBeVisible();
  });

  test("creates a note, opens views as tabs, and changes layout", async ({ page }, testInfo) => {
    await page.goto("/vaults/engineering");
    await page.getByRole("button", { name: "New Note" }).click();
    const dialog = page.getByRole("dialog", { name: "New note" });
    await dialog.getByLabel("Title").fill("Workspace note");
    await dialog.getByLabel("File path").fill("workspace-note.md");
    await dialog.getByRole("button", { name: "Create note" }).click();
    await expect(page.getByRole("tab", { name: /Workspace note/ })).toBeVisible();
    await expect(page.getByTestId("vnotename")).toContainText("workspace-note.md");
    await expect(page.getByTestId("mdbody")).toContainText("Workspace note");

    await page.getByRole("button", { name: "New Note" }).click();
    const duplicateDialog = page.getByRole("dialog", { name: "New note" });
    await duplicateDialog.getByLabel("Title").fill("Replacement");
    await duplicateDialog.getByLabel("File path").fill("workspace-note.md");
    await duplicateDialog.getByRole("button", { name: "Create note" }).click();
    await expect(duplicateDialog.getByRole("alert")).toContainText("note already exists");
    await duplicateDialog.getByRole("button", { name: "Cancel" }).click();

    await page.getByRole("button", { name: "Graph View" }).click();
    await page.getByRole("button", { name: "Table View" }).click();
    await expect(page.getByRole("tab", { name: "Graph View" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Table View" })).toBeVisible();
    await page.getByRole("button", { name: "Two columns" }).click();
    await expect(page.locator(".vault-pane")).toHaveCount(2);
    await snap(page, testInfo, "vault-tabs-and-columns");

    const file = page.getByRole("treeitem").filter({ hasText: "workspace-note.md" });
    await file.click({ button: "right" });
    await expect(page.getByRole("menuitem", { name: "Details" })).toBeVisible();
  });
});
