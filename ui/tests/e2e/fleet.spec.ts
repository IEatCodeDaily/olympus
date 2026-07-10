import { test, expect } from "@playwright/test";

const unavailable = "This surface is unavailable until its data is organization-owned.";

test.describe("Fleet view", () => {
  test("fails closed until nodes are organization-owned", async ({ page }) => {
    await page.goto("/fleet");
    await expect(page.getByText(unavailable)).toBeVisible();
    await expect(page.locator("[data-testid='fleet-grid']")).toHaveCount(0);
  });
});
