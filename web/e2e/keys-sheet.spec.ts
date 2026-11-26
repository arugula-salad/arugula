// Lift 3 (#1479): one key table (shared/keys.ts). `?` lists every binding
// the page has, each with a key; the footer's chips come from the same
// table; a plain key is never taken from a terminal; `g m` goes to Machines.

import { expect, test } from "@playwright/test";
import { active, openSettingsPage, paneEl, reset } from "./helpers";

declare global {
  interface Window {
    __arugulaKeys: () => string[];
  }
}

test("`?` lists every binding in the table, each with its key", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await reset(page);
  const footer = page.locator("[data-status-footer]");
  await expect(footer).toContainText("Machines");
  // The workspace's keys are the footer's chips.
  await expect(footer.locator('[data-chip="pane.jump"]')).toBeVisible();

  await footer.locator("[data-open-keys]").click();
  const sheet = page.locator("[data-keys-sheet]");
  await expect(sheet).toBeVisible();
  const listed = await sheet.locator("[data-binding]").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.binding!));
  const table = await page.evaluate(() => window.__arugulaKeys());
  expect([...listed].sort()).toEqual([...table].sort());
  // No binding is unreachable: each shows a key.
  for (const id of listed) await expect(sheet.locator(`[data-binding="${id}"] kbd`).first()).not.toBeEmpty();
  await expect(sheet.locator('[data-binding="go.machines"]')).toContainText("g");

  // `?` again, or Escape, closes it.
  await page.keyboard.press("?");
  await expect(sheet).toHaveCount(0);
  await footer.locator("[data-open-keys]").click();
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
});

test("a plain key typed in a terminal stays the terminal's", async ({ page }) => {
  await reset(page);
  const pane = await active(page);
  await paneEl(page, pane).click({ position: { x: 40, y: 40 } });
  await page.keyboard.press("?");
  await page.waitForTimeout(300);
  await expect(page.locator("[data-keys-sheet]")).toHaveCount(0);
});

test("`g m` goes back to Machines from Settings", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await reset(page);
  const settings = await openSettingsPage(page);
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press("g");
  await page.keyboard.press("m");
  await expect(settings).toHaveCount(0);
});
