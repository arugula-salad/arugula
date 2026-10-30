// "Edit in nvim": a checkbox for the Editor preference, in + Pane › Editor
// and on Getting started. Ticking it sets the same preference as Settings ›
// Editor (nvim); unticking goes back to what it was before. Needs no nvim:
// only the preference is checked. + Pane's checkbox on a page that shows
// VS Code is in editor-pref.spec.ts, whose daemon has a block listener.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem, reset } from "./helpers";

test.afterAll(closeContexts);

// The app doesn't greet automation; this one pretends to be a person.
const person = () => Object.defineProperty(Navigator.prototype, "webdriver", { get: () => false });

const pref = (page: Page) => page.evaluate(() => (JSON.parse(localStorage.getItem("arugula.seen") ?? "{}") as Record<string, unknown>).editor ?? null);

/** Settings › Editor's row for `label` is checked (or not). */
async function settingsShows(page: Page, label: "VS Code" | "nvim" | "Ask each time", checked: boolean) {
  await page.locator("[data-account-menu]").first().click();
  await expect(await menuItem(page, label, { exact: true })).toHaveAttribute("aria-checked", String(checked));
  await page.keyboard.press("Escape");
}

test("+ Pane › Editor on a page that can't show VS Code: Edit in nvim is ticked and fixed", async ({ page }) => {
  await reset(page);
  await page.locator("[data-add-pane]").first().click();
  const drawer = page.locator("[data-add-pane-drawer]");
  await drawer.locator('[data-kind="editor"]').click();
  await expect(drawer.locator("[data-edit-in-nvim]")).toBeChecked();
  await expect(drawer.locator("[data-edit-in-nvim]")).toBeDisabled();
});

test("Getting started: Edit in nvim sets the same preference", async ({ browser }) => {
  const ctx = await browser.newContext();
  await ctx.addInitScript(person);
  const page = await ctx.newPage();
  await reset(page);
  const panel = page.getByRole("dialog", { name: "Getting started" });
  await expect(panel).toBeVisible();
  const box = panel.locator("[data-edit-in-nvim]");
  await expect(box).not.toBeChecked();
  await box.check();
  expect(await pref(page)).toBe("nvim");
  await panel.getByRole("button", { name: "Close" }).click();
  await settingsShows(page, "nvim", true);
});
