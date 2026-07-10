import { test, expect } from "@playwright/test";
const unavailable = "This surface is unavailable until its data is organization-owned.";

test.describe("Vaults @desktop-only", () => {
  test("fails closed until vaults are organization-owned", async ({ page }) => {
    await page.goto("/vaults");
    await expect(page.getByText(unavailable)).toBeVisible();
    await expect(page.getByRole("button", { name: /new vault|create/i })).toHaveCount(0);
  });
});
