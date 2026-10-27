// What's new (shell/ui/whats-new.tsx): after an update, a small panel with
// the headlines of the releases since the last one shown, once; Try it
// opens the thing; the account menu's What's new and the update chip's
// open it again. On a daemon's own page "shown" is this browser's
// (localStorage `arugula.seen`, key `whats-new`); the account's is
// control's `/api/me/prefs` (crates/control/src/prefs.rs, its own tests).
// Which headlines there are is the release notes' (docs/releases), so
// these read them from the page.

import { expect, test, type Page } from "@playwright/test";
import { menuItem, open, reset } from "./helpers";

const panel = (page: Page) => page.locator("[data-whats-new]");
const shown = (page: Page) => page.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}")["whats-new"] ?? null);

/** As after an update from an old version: it opens by itself only for a
 * browser that isn't automation's. */
async function updatedFrom(page: Page, version: string) {
  await page.addInitScript((v) => {
    Object.defineProperty(navigator, "webdriver", { get: () => false });
    if (!sessionStorage.getItem("e2e.whats-new")) {
      sessionStorage.setItem("e2e.whats-new", "1");
      localStorage.setItem("arugula.seen", JSON.stringify({ "whats-new": v }));
    }
  }, version);
}

test("after an update: the panel opens once, with the new headlines", async ({ page }) => {
  await updatedFrom(page, "0.0.1");
  await reset(page);
  await expect(panel(page)).toBeVisible();
  const newest = await panel(page).locator("[data-whats-new-release]").first().getAttribute("data-whats-new-release");
  expect(newest).toMatch(/^\d+\.\d+\.\d+$/);
  await expect(panel(page).locator("[data-whats-new-notes]").first()).toHaveAttribute("href", `https://github.com/arugula-salad/arugula/releases/tag/v${newest}`);
  // Seen when shown: the pref moved on to this build.
  await expect.poll(() => shown(page)).toBe(newest);
  // A daemon's own page isn't signed in to control: what needs it is left out.
  await expect(panel(page).locator('[data-whats-new-item="project-docs"], [data-whats-new-item="board-pin"]')).toHaveCount(0);

  await page.reload();
  await open(page);
  await page.waitForTimeout(1000);
  await expect(panel(page)).toHaveCount(0);
});

test("the account menu's What's new; Try it opens the thing and closes it", async ({ page }) => {
  await reset(page);
  await expect(panel(page)).toHaveCount(0);
  await page.locator("[data-account-menu]").first().click();
  await (await menuItem(page, /^What's new/)).click();
  await expect(panel(page)).toBeVisible();
  const tries = panel(page).locator("[data-whats-new-try]");
  if ((await tries.count()) === 0) return;
  // The pane menu's headline opens the command palette.
  const palette = panel(page).locator('[data-whats-new-item="pane-menu-shorter"] [data-whats-new-try]');
  if (await palette.count()) {
    await palette.click();
    await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
    await page.keyboard.press("Escape");
  } else await tries.first().click();
  await expect(panel(page)).toHaveCount(0);
});

test("the update chip's What's new opens the panel, with the new release's notes", async ({ page }) => {
  await page.route("**/api/update", (r) =>
    r.fulfill({
      json: { current: "0.16.0", latest: "0.17.0", newer: true, enabled: true, kind: "script", command: "true", url: "https://github.com/arugula-salad/arugula/releases/tag/v0.17.0" },
    }),
  );
  await reset(page);
  await page.locator("[data-update-chip]").click();
  await page.locator("[data-update-whats-new]").click();
  await expect(page.getByRole("dialog", { name: "Update Arugula" })).toHaveCount(0);
  await expect(panel(page)).toContainText("Arugula 0.17.0 is out.");
  await expect(panel(page).getByRole("link", { name: "Its notes ↗" })).toHaveAttribute("href", /\/tag\/v0\.17\.0$/);
  await panel(page).locator("[data-whats-new-close]").click();
  await expect(panel(page)).toHaveCount(0);
});
