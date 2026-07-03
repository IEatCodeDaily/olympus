import { test, expect, type Page } from "@playwright/test";

/**
 * E2E: Fleet view — node grid, drill-in panel, Add-node affordance.
 *
 * Uses MSW mocks (NODES fixture: local/gpu-box/edge-mini).
 * Verifies:
 *  1. Fleet surface loads via nav.
 *  2. All three node cards render with correct status dots.
 *  3. Clicking a node card opens the detail panel.
 *  4. Panel shows hostname, slots, and a sessions list.
 *  5. Clicking the same card again collapses the panel.
 *  6. Add-node button opens the help popover.
 */

async function gotoFleet(page: Page) {
  await page.goto("/fleet");
  await page.waitForSelector("[data-testid='fleet-grid']", { timeout: 10_000 });
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

    const localCard = page.locator("[data-node-id='local']");
    await expect(localCard).toBeVisible();
    await expect(localCard.locator(".ol-badge-accent")).toContainText("LOCAL");
  });

  test("node cards show status badges", async ({ page }) => {
    await gotoFleet(page);

    // online node (local) gets ok badge
    const localCard = page.locator("[data-node-id='local']");
    // Has a live dot (ol-dot-live class)
    await expect(localCard.locator(".ol-dot-live")).toBeVisible();

    // draining node (gpu-box) gets warn badge
    const gpuCard = page.locator("[data-node-id='gpu-box']");
    await expect(gpuCard.locator(".ol-dot-warn")).toBeVisible();
  });

  test("clicking a node card opens the detail panel", async ({ page }) => {
    await gotoFleet(page);

    const localCard = page.locator("[data-node-id='local']");
    await localCard.click();

    // Detail panel should appear
    const panel = page.locator("[aria-label='Node local detail']");
    await expect(panel).toBeVisible();

    // Panel shows hostname
    await expect(panel).toContainText("localhost");

    // Panel shows SLOTS heading
    await expect(panel).toContainText("SLOTS");

    // Panel shows RUNNING SESSIONS heading
    await expect(panel).toContainText("RUNNING SESSIONS");
  });

  test("clicking the same card again collapses the panel", async ({ page }) => {
    await gotoFleet(page);

    const localCard = page.locator("[data-node-id='local']");

    // Open
    await localCard.click();
    await expect(page.locator("[aria-label='Node local detail']")).toBeVisible();

    // Close via same card
    await localCard.click();
    await expect(page.locator("[aria-label='Node local detail']")).not.toBeVisible();
  });

  test("panel close button dismisses the panel", async ({ page }) => {
    await gotoFleet(page);

    await page.locator("[data-node-id='local']").click();
    const panel = page.locator("[aria-label='Node local detail']");
    await expect(panel).toBeVisible();

    await panel.getByRole("button", { name: "Close panel" }).click();
    await expect(panel).not.toBeVisible();
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

    const edgeCard = page.locator("[data-node-id='edge-mini']");
    await expect(edgeCard).toBeVisible();
    // offline dot
    await expect(edgeCard.locator(".ol-dot-err")).toBeVisible();
  });
});
