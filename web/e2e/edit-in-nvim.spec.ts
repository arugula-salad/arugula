// "Edit in nvim": a checkbox for the Editor preference, in + Pane › Editor
// (Getting started had it until Lift 7). Ticking it sets the same preference as Settings ›
// Editor (nvim); unticking goes back to what it was before. Needs no nvim:
// only the preference is checked. + Pane's checkbox on a page that shows
// VS Code is in editor-pref.spec.ts, whose daemon has a block listener.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem, reset, openSettingsPage, closeSettingsPage, paletteCommand, setting } from "./helpers";

test.afterAll(closeContexts);

const pref = (page: Page) => page.evaluate(() => (JSON.parse(localStorage.getItem("arugula.seen") ?? "{}") as Record<string, unknown>).editor ?? null);

/** Settings › Editor's row for `label` is checked (or not). */
async function settingsShows(page: Page, label: "VS Code" | "nvim" | "Ask each time", checked: boolean) {
  await expect((await setting(page, "radio", label))).toBeChecked({ checked });
  await closeSettingsPage(page);
}

test("+ Pane › Editor on a page that can't show VS Code: Edit in nvim is ticked and fixed", async ({ page }) => {
  await reset(page);
  await page.locator("[data-add-pane]").first().click();
  const drawer = page.locator("[data-add-pane-drawer]");
  await drawer.locator('[data-kind="editor"]').click();
  await expect(drawer.locator("[data-edit-in-nvim]")).toBeChecked();
  await expect(drawer.locator("[data-edit-in-nvim]")).toBeDisabled();
});

