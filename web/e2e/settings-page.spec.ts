// The Settings page (Arugula #695): `#settings`, what the rail's Settings
// opens, on a daemon's own page. On control's, with the platform's half of
// the index, it's in platform.spec.ts.

import { expect, test } from "@playwright/test";
import { closeContexts, closeSettingsPage, openSettingsPage, reset, settingsRow, sheetTab } from "./helpers";

test.afterAll(closeContexts);

test("the rail's Settings is a page: every scope's rows, and back to the panes", async ({ page }) => {
  await reset(page);
  const settings = await openSettingsPage(page);
  await expect(page).toHaveURL(/#settings$/);
  await expect(page).toHaveTitle(/^Settings/);
  for (const id of ["device", "preferences", "machine", "help"]) await expect(settings.locator(`[data-settings-section="${id}"]`)).toBeVisible();

  // Each row under what it reaches.
  const machine = settings.locator('[data-settings-section="machine"]');
  await expect(machine.getByRole("button", { name: "Developer settings…" })).toBeVisible();
  await expect(machine.getByRole("button", { name: "Remove arugula.nvim" })).toBeVisible();
  await expect(settings.locator('[data-settings-section="device"]').getByRole("button", { name: "Appearance…" })).toBeVisible();
  await expect(settings.locator('[data-settings-section="preferences"]').getByRole("switch", { name: "Tips", exact: true })).toBeChecked();

  // A daemon's own page has no account, so no second way in, and no platform.
  await expect(page.locator("[data-account-menu]")).toHaveCount(0);
  await expect(settings.locator("[data-settings-index-elsewhere]")).toHaveCount(0);
  await expect(settings.locator("[data-settings-elsewhere]")).toHaveCount(0);

  // A switch says what it now is, without leaving the page.
  const tips = settings.getByRole("switch", { name: "Tips", exact: true });
  await tips.click();
  await expect(tips).not.toBeChecked();
  await tips.click();
  await expect(tips).toBeChecked();

  // A dialog opens over the page, which is still there when it shuts.
  await settingsRow(page, "Appearance…");
  await expect(page.locator("[data-appearance]")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("[data-appearance]")).toHaveCount(0);
  await expect(settings).toBeVisible();

  await closeSettingsPage(page);
  await expect(page).not.toHaveURL(/#settings/);
});

test("#settings=<section> opens at it, and a reload stays there", async ({ page }) => {
  await reset(page);
  await page.goto("/#settings=machine");
  const heading = page.locator("#machine-h");
  await expect(heading).toBeInViewport();
  await page.reload();
  await expect(heading).toBeInViewport();
  // The index keeps the page's own address, not a bare `#machine`.
  await expect(page.locator(".settings-index a", { hasText: "Help" })).toHaveAttribute("href", "#settings=help");
});

test("the command palette still finds every setting, and goes to the page", async ({ page }) => {
  await reset(page);
  await page.locator(".ws-head [data-open-palette]").click();
  const input = page.locator(".palette-search input");
  await input.fill("Developer settings");
  await expect(page.getByRole("option", { name: /Developer settings/ }).first()).toBeVisible();
  await input.fill("Go to Settings");
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-settings-page]")).toBeVisible();
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test("the sheet's Settings opens the page, with no sideways scroll", async ({ page }) => {
    await reset(page);
    await page.locator(".sheet-button").click();
    await sheetTab(page, "More");
    await page.locator(".sheet [data-open-settings]").click();
    const settings = page.locator("[data-settings-page]");
    await expect(settings.getByRole("button", { name: "Appearance…" })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await settings.locator("[data-open-panes]").click();
    await expect(settings).toHaveCount(0);
  });
});
