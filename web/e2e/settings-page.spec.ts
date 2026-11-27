// The Settings page (Arugula #695): `#settings`, what the rail's Settings
// opens, on a daemon's own page, which has every section. It draws the
// app's list (Lift 7): a kit Row a section, opening in place at
// `#settings=<id>`; how it looks is Look's, not a section. On control's
// page, which has the app's Settings, it keeps Phones and browsers alone,
// and the rest of its old links land in the app (Arugula #1110): the last
// test, with control made up on the suite's daemon and the app a stand-in.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, closeSettingsPage, openSettingsPage, reset, setting, sheetTab } from "./helpers";

test.afterAll(closeContexts);

const row = (page: Page, id: string) => page.locator(`[data-settings-row="${id}"]`);
const opened = (page: Page, id: string) => row(page, id).locator("button[aria-expanded]").first();

test("the rail's Settings is a page: a row a section, each opening in place, and back to the panes", async ({ page }) => {
  await reset(page);
  const settings = await openSettingsPage(page);
  await expect(page).toHaveURL(/#settings$/);
  await expect(page).toHaveTitle(/^Settings/);
  for (const id of ["push", "preferences", "machine"]) await expect(row(page, id)).toBeVisible();
  // Help is Go to…'s, and how it looks is Look's: neither is a section here (Lift 7).
  await expect(settings.locator('[data-settings-row="help"], [data-settings-row="appearance"]')).toHaveCount(0);
  await expect(settings.locator("[data-appearance-controls]")).toHaveCount(0);
  // Every row starts closed; a click opens it at its address, and another closes it.
  await expect(settings.locator('[data-settings-section="machine"]')).toBeHidden();
  await opened(page, "machine").click();
  await expect(page).toHaveURL(/#settings=machine$/);
  const machine = settings.locator('[data-settings-section="machine"]');
  await expect(machine.getByRole("button", { name: "Developer settings…" })).toBeVisible();
  await expect(machine.getByRole("button", { name: "Remove arugula.nvim" })).toBeVisible();
  await opened(page, "machine").click();
  await expect(page).toHaveURL(/#settings$/);
  await expect(machine).toBeHidden();

  // A daemon's own page has no account, so no second way in.
  await expect(page.locator("[data-account-menu]")).toHaveCount(0);
  // …and no app, so nothing moves.
  await expect(settings.locator("[data-settings-in-app]")).toHaveCount(0);

  // A switch says what it now is, without leaving the page.
  const tips = await setting(page, "switch", "Tips");
  await expect(opened(page, "preferences")).toHaveAttribute("aria-expanded", "true");
  await tips.click();
  await expect(tips).not.toBeChecked();
  await tips.click();
  await expect(tips).toBeChecked();

  await closeSettingsPage(page);
  await expect(page).not.toHaveURL(/#settings/);
});

test("#settings=<section> opens at it, and a reload stays there", async ({ page }) => {
  await reset(page);
  await page.goto("/#settings=machine");
  await expect(opened(page, "machine")).toHaveAttribute("aria-expanded", "true");
  await expect(row(page, "machine")).toBeInViewport();
  await page.reload();
  await expect(opened(page, "machine")).toHaveAttribute("aria-expanded", "true");
  // Appearance is Look's now: its old link opens Preferences.
  await page.goto("/#settings=appearance");
  await page.reload();
  await expect(opened(page, "preferences")).toHaveAttribute("aria-expanded", "true");
});

test("This device and All your devices are one section, Phones and browsers, and their old links land on it", async ({ page }) => {
  await reset(page);
  for (const old of ["device", "all-devices"]) {
    await page.goto(`/#settings=${old}`);
    await expect(row(page, "push").locator("[data-row-title]")).toHaveText("Phones and browsers");
    await expect(opened(page, "push")).toHaveAttribute("aria-expanded", "true");
  }
});

test("Help is in Go to…, not in Settings", async ({ page }) => {
  await reset(page);
  await page.locator("[data-open-palette]").click();
  const palette = page.getByRole("dialog", { name: "Go to…" });
  const filter = palette.getByRole("combobox", { name: "Go to" });
  for (const label of ["Getting started", "What's new", "nvim in Arugula"]) {
    await filter.fill(label);
    await expect(palette.locator(`[data-command="Help/${label}"]`)).toBeVisible();
    await expect(palette.locator(`[data-command="Settings/${label}"]`)).toHaveCount(0);
  }
});

// Arugula #1094: who reached the machine through Arugula cloud, from the
// daemon's own log, under This machine, with the remote-input switch's state.
test("Remote sessions lists who came through Arugula cloud, one row each", async ({ page }) => {
  await reset(page);
  const now = Date.now();
  await page.route("**/api/remote-sessions", (r) =>
    r.fulfill({
      json: {
        remote_input: false,
        sessions: [
          { account: "a2", name: "lee", role: "viewer", path: "relay", by: "token", opened_ms: now - 60_000, open: true },
          { account: "a1", name: "sam", role: "editor", path: "relay", by: "token", opened_ms: now - 7_200_000, closed_ms: now - 6_480_000, open: false },
          { account: "a1", name: "sam", role: "owner", path: "direct", by: "token", opened_ms: now - 90_000_000, open: false },
        ],
      },
    }),
  );
  await page.goto("/#settings=remote-sessions");
  const section = page.locator('[data-settings-section="remote-sessions"]');
  await expect(row(page, "remote-sessions").locator("[data-row-title]")).toHaveText("Remote sessions 3");
  const rows = section.locator("[data-remote-session]");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toHaveAttribute("data-remote-session", "open");
  await expect(rows.nth(0)).toContainText("lee");
  await expect(rows.nth(0)).toContainText("Open");
  await expect(rows.nth(1)).toContainText("for 12m");
  await expect(rows.nth(2)).toContainText("until the daemon stopped");
  await expect(section.locator("[data-remote-input]")).toHaveAttribute("data-remote-input", "off");
});

test("a daemon without the log's route shows no Remote sessions, and no error", async ({ page }) => {
  await reset(page);
  await page.route("**/api/remote-sessions", (r) => r.fulfill({ status: 404, json: { error: "no such route" } }));
  await page.goto("/#settings=machine");
  await expect(page.locator('[data-settings-section="machine"]')).toBeVisible();
  await expect(row(page, "remote-sessions")).toHaveCount(0);
  await expect(page.locator("[data-settings-page] .error")).toHaveCount(0);
});

// Go to… goes to the page; each setting is the page's, not a command
// (Lift 3 trimmed them, #1479).
test("Go to… goes to Settings", async ({ page }) => {
  await reset(page);
  await page.locator("[data-open-palette]").click();
  const input = page.getByRole("combobox", { name: "Go to" });
  await input.fill("Settings");
  await expect(page.locator('[data-command="Places/settings"]')).toBeVisible();
  await input.fill("Developer settings");
  await expect(page.locator('[data-command="Settings/Developer settings…"]')).toHaveCount(0);
  await input.fill("Settings");
  await page.locator('[data-command="Places/settings"]').click();
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
    await expect(settings.locator('[data-settings-row="preferences"]')).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await settings.locator("[data-open-panes]").click();
    await expect(settings).toHaveCount(0);
  });
});

// Arugula #1110: on control's page, an old link to a section that's in the
// app's Settings now lands on it, at once, as do control's Machines and
// Shared Docs dialogs' (`#devices`, `#shared-docs`); Phones and browsers stays. A
// machine's own sections wait for the machine shown (settings-machine.spec.ts).
test("on control's page, old #settings links land on the app's Settings, but Phones and browsers stays", async ({ page }) => {
  const APP = "http://app.test";
  await page.route(/\/control\.json$/, (r) => r.fulfill({ contentType: "application/json", body: JSON.stringify({ control: true, url: new URL(r.request().url()).origin, platform: APP }) }));
  await page.route(`${APP}/**`, (r) => r.fulfill({ contentType: "text/html", body: "<!doctype html><title>the app</title><p data-app-stand-in>the app</p>" }));
  for (const [hash, path] of [
    ["#settings", "/settings"],
    ["#settings=appearance", "/settings/you/preferences"],
    ["#settings=preferences", "/settings/you/preferences"],
    ["#settings=account", "/settings/you/security"],
    // Control's Machines… and Shared Docs… dialogs are the app's too.
    ["#devices", "/settings/team/personal/machines"],
    ["#shared-docs", "/docs?shared=1"],
  ]) {
    await page.goto(`/${hash}`);
    await expect(page).toHaveURL(`${APP}${path}`);
    await expect(page.locator("[data-app-stand-in]")).toBeVisible();
  }
  for (const hash of ["#settings=push", "#settings=device"]) {
    await page.goto(`/${hash}`);
    // Long enough for a redirect to have happened.
    await page.waitForTimeout(500);
    expect(new URL(page.url()).origin, hash).not.toBe(APP);
  }
});
