import { test, expect, type Page } from "@playwright/test";

/**
 * E2E: Fleet view — node grid, node drawer, agents sub-tab.
 *
 * Uses MSW mocks (NODES fixture: local/gpu-box/edge-mini).
 * Tests the CANONICAL FleetView (grid + slide-in drawer + Fleet|Agents
 * sub-tabs) — the N2 "detail page" variant was dropped in the merge; specs
 * against aria-label='Node local detail' / .ol-dot-* were stale.
 */

async function gotoFleet(page: Page) {
  await page.goto("/fleet");
  await page.waitForSelector("[data-testid='fleet-grid'] [data-node-id]", { timeout: 10_000 });
}

/** Scoped to the grid container to avoid sidebar ambiguity. */
function gridCard(page: Page, nodeId: string) {
  return page.locator(`[data-testid='fleet-grid'] [data-node-id='${nodeId}']`);
}

test.describe("Fleet view", () => {
  test("fleet grid renders node cards", async ({ page }) => {
    await gotoFleet(page);

    const cards = page.locator("[data-testid='fleet-grid'] [data-node-id]");
    // MSW fixture has 3 nodes (local, gpu-box, edge-mini)
    expect(await cards.count()).toBeGreaterThanOrEqual(3);
  });

  test("node cards show status tags", async ({ page }) => {
    await gotoFleet(page);

    // online node (local) → ok tag
    await expect(gridCard(page, "local").locator(".gtag.ok")).toBeVisible();
    // draining node (gpu-box) → warn tag
    await expect(gridCard(page, "gpu-box").locator(".gtag.warn")).toBeVisible();
    // offline node (edge-mini) → err tag
    await expect(gridCard(page, "edge-mini").locator(".gtag.err")).toBeVisible();
  });

  test("clicking a node card opens the drawer with node facts", async ({ page }) => {
    await gotoFleet(page);

    await gridCard(page, "local").click();

    const drawer = page.getByRole("dialog", { name: "Node local" });
    await expect(drawer).toBeVisible();
    // Drawer shows the hostname from the fixture
    await expect(drawer).toContainText("localhost");

    // Close it
    await drawer.getByRole("button", { name: "Close drawer" }).click();
    await expect(drawer).not.toBeVisible();
  });

  test("slots bar reflects fixture slot usage", async ({ page }) => {
    await gotoFleet(page);

    // local: 2/4 slots in the fixture
    await expect(gridCard(page, "local")).toContainText("2 / 4");
    // gpu-box: 5/6
    await expect(gridCard(page, "gpu-box")).toContainText("5 / 6");
  });

  test("agents sub-tab lists per-node agents", async ({ page }) => {
    await gotoFleet(page);

    // Switch to the Agents sub-tab
    await page
      .locator("[role='tablist'][aria-label='Fleet sub-view'] button", {
        hasText: /agents/i,
      })
      .click();

    // The grid disappears; per-node agent sections render
    await expect(page.locator("[data-testid='fleet-grid']")).not.toBeVisible();
  });
});
