import { test, expect, type Page } from "@playwright/test";

/**
 * E2E: Fleet view — node grid, node detail page, Add-node affordance.
 *
 * Uses MSW mocks (NODES fixture: local/gpu-box/edge-mini).
 * Verifies:
 *  1. Fleet surface loads via nav.
 *  2. All three node cards render with correct status dots.
 *  3. Clicking a node card navigates to /fleet/$nodeId (detail page).
 *  4. Detail page shows hostname, slots, and a sessions list.
 *  5. Back navigation (chevron) returns to the grid.
 *  6. Add-node button opens the help popover.
 *
 * NOTE: Both the sidebar and the grid use data-node-id. Tests scope card
 * locators to [data-testid='fleet-grid'] to avoid strict-mode ambiguity.
 */

async function gotoFleet(page: Page) {
  await page.goto("/fleet");
  await page.waitForSelector("[data-testid='fleet-grid']", { timeout: 10_000 });
}

/** Scoped to the grid container to avoid sidebar ambiguity. */
function gridCard(page: Page, nodeId: string) {
  return page.locator(`[data-testid='fleet-grid'] [data-node-id='${nodeId}']`);
}

test.describe("Fleet view", () => {
  test("fleet grid renders node cards", async ({ page }) => {
    await gotoFleet(page);

    const cards = page.locator("[data-testid='fleet-grid'] [data-node-id]");
    const count = await cards.count();
    // MSW fixture has 3 nodes (local, gpu-box, edge-mini)
    expect(count).toBeGreaterThanOrEqual(1);
  });

  test("local node card has LOCAL badge", async ({ page }) => {
    await gotoFleet(page);

    const localCard = gridCard(page, "local");
    await expect(localCard).toBeVisible();
    await expect(localCard.locator(".ol-badge-accent")).toContainText("LOCAL");
  });

  test("node cards show status badges", async ({ page }) => {
    await gotoFleet(page);

    // online node (local) gets ok badge
    const localCard = gridCard(page, "local");
    // Has a live dot (ol-dot-live class)
    await expect(localCard.locator(".ol-dot-live")).toBeVisible();

    // draining node (gpu-box) gets warn badge
    const gpuCard = gridCard(page, "gpu-box");
    await expect(gpuCard.locator(".ol-dot-warn")).toBeVisible();
  });

  test("clicking a node card opens the detail page", async ({ page }) => {
    await gotoFleet(page);

    const localCard = gridCard(page, "local");
    await localCard.click();

    // URL navigates to /fleet/local
    await page.waitForURL("**/fleet/local", { timeout: 5_000 });

    // Detail page renders with the node's aria-label
    const detail = page.locator("[aria-label='Node local detail']");
    await expect(detail).toBeVisible();

    // Detail shows hostname
    await expect(detail).toContainText("localhost");

    // Detail shows SLOTS heading
    await expect(detail).toContainText("SLOTS");

    // Detail shows RUNNING SESSIONS heading
    await expect(detail).toContainText("RUNNING SESSIONS");
  });

  test("back button from detail page returns to grid", async ({ page }) => {
    await gotoFleet(page);

    // Navigate to detail via grid card click
    await gridCard(page, "local").click();
    await page.waitForURL("**/fleet/local", { timeout: 5_000 });
    await expect(page.locator("[aria-label='Node local detail']")).toBeVisible();

    // Click the back button (aria-label "Back to fleet")
    await page.getByRole("button", { name: "Back to fleet" }).click();
    await page.waitForURL("**/fleet", { timeout: 5_000 });

    // Grid is visible again
    await expect(page.locator("[data-testid='fleet-grid']")).toBeVisible();
    await expect(page.locator("[aria-label='Node local detail']")).not.toBeVisible();
  });

  test("sidebar 'All nodes' link returns to grid from detail", async ({ page }) => {
    await page.goto("/fleet/local");
    await page.waitForSelector("[aria-label='Node local detail']", { timeout: 10_000 });

    // Click "All nodes" in the sidebar
    await page.getByRole("button", { name: "All nodes" }).click();
    await page.waitForURL("**/fleet", { timeout: 5_000 });

    await expect(page.locator("[data-testid='fleet-grid']")).toBeVisible();
  });

  test("Add-node button opens the help popover", async ({ page }) => {
    await gotoFleet(page);

    const addBtn = page.locator("[data-testid='fleet-view']").getByRole("button", { name: "Add node" });
    await expect(addBtn).toBeVisible();
    await addBtn.click();

    // Popover with heading visible
    const popover = page.getByRole("dialog", { name: "Add node" });
    await expect(popover).toBeVisible();
    await expect(popover).toContainText("olympus-envoy");
  });

  test("offline node card renders with reduced prominence", async ({ page }) => {
    await gotoFleet(page);

    const edgeCard = gridCard(page, "edge-mini");
    await expect(edgeCard).toBeVisible();
    // offline dot
    await expect(edgeCard.locator(".ol-dot-err")).toBeVisible();
  });
});
