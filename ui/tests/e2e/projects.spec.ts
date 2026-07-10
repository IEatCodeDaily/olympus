import { test, expect } from "@playwright/test";
const unavailable = "This surface is unavailable until its data is organization-owned.";

test.describe("Projects", () => {
  test("fails closed until projects are organization-owned", async ({ page }) => {
    await page.goto("/projects");
    await expect(page.getByText(unavailable)).toBeVisible();
    await expect(page.getByRole("button", { name: /new project|create/i })).toHaveCount(0);
  });
});
