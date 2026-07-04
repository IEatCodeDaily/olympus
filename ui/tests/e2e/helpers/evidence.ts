import type { Page, TestInfo } from "@playwright/test";

/**
 * Numbered, named screenshot attached to the report AND kept on disk.
 * Use at each meaningful state transition so the video + numbered
 * screenshots tell a complete story of what happened during the test.
 *
 * Usage:
 *   test("my feature", async ({ page }, testInfo) => {
 *     await page.goto("/sessions");
 *     await snap(page, testInfo, "sessions-loaded");
 *     ...
 *     await snap(page, testInfo, "after-filter");
 *   });
 */
export async function snap(page: Page, testInfo: TestInfo, name: string) {
  const privateInfo = testInfo as TestInfo & { _snapIdx?: number };
  privateInfo._snapIdx = (privateInfo._snapIdx ?? 0) + 1;
  const idx = privateInfo._snapIdx;
  const file = testInfo.outputPath(`${String(idx).padStart(2, "0")}-${name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  await testInfo.attach(name, { path: file, contentType: "image/png" });
}
