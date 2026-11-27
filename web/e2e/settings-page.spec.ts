// The Settings page (Arugula #695): `#settings`, what the rail's Settings
// opens, on a daemon's own page, which has every section. On control's
// page, which has the app's Settings, it keeps Phones and browsers alone,
// and the rest of its old links land in the app (Arugula #1110): the last
// test, with control made up on the suite's daemon and the app a stand-in.

import { expect, test } from "@playwright/test";
import { closeContexts, closeSettingsPage, openSettingsPage, reset, sheetTab } from "./helpers";

test.afterAll(closeContexts);

test("the rail's Settings is a page: every scope's rows, and back to the panes", async ({ page }) => {
  await reset(page);
  const settings = await openSettingsPage(page);
  await expect(page).toHaveURL(/#settings$/);
  await expect(page).toHaveTitle(/^Settings/);
  for (const id of ["push", "appearance", "machine"]) await expect(settings.locator(`[data-settings-section="${id}"]`)).toBeVisible();
  // Help is Go to…'s, not a section here (Arugula #1108).
  await expect(settings.locator('[data-settings-section="help"]')).toHaveCount(0);

  // Each row under what it reaches.
  const machine = settings.locator('[data-settings-section="machine"]');
  await expect(machine.getByRole("button", { name: "Developer settings…" })).toBeVisible();
  await expect(machine.getByRole("button", { name: "Remove arugula.nvim" })).toBeVisible();
  // Appearance's controls in place (Arugula #1318), Preferences a part of it.
  const look = settings.locator('[data-settings-section="appearance"]');
  await expect(look.locator("[data-appearance-controls]")).toBeVisible();
  await expect(settings.getByRole("button", { name: "Appearance…" })).toHaveCount(0);
  await expect(look.locator('[data-settings-part="preferences"]').getByRole("switch", { name: "Tips", exact: true })).toBeChecked();

  // A daemon's own page has no account, so no second way in.
  await expect(page.locator("[data-account-menu]")).toHaveCount(0);
  // …and no app, so nothing moves.
  await expect(settings.locator("[data-settings-in-app]")).toHaveCount(0);

  // A switch says what it now is, without leaving the page.
  const tips = settings.getByRole("switch", { name: "Tips", exact: true });
  await tips.click();
  await expect(tips).not.toBeChecked();
  await tips.click();
  await expect(tips).toBeChecked();

  // A theme applies at once, and Reset goes back to nothing picked.
  await look.locator('[data-theme-choice="light"]').click();
  await expect(page.locator("html")).toHaveAttribute("data-scheme", "light");
  await look.getByRole("button", { name: "Reset", exact: true }).click();
  await expect(page.locator("html")).not.toHaveAttribute("data-scheme", "light");
  await expect(page.locator("[data-appearance]")).toHaveCount(0);

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
  await expect(page.locator(".settings-index a", { hasText: "Appearance" })).toHaveAttribute("href", "#settings=appearance");
  // Preferences is part of Appearance now: its old link opens there.
  await expect(page.locator(".settings-index a", { hasText: "Preferences" })).toHaveCount(0);
  // A fresh load (a hash change alone doesn't reopen the page at a section).
  await page.goto("/#settings=preferences");
  await page.reload();
  await expect(page.locator("#appearance-h")).toBeInViewport();
});

test("This device and All your devices are one section, Phones and browsers, and their old links land on it", async ({ page }) => {
  await reset(page);
  for (const old of ["device", "all-devices"]) {
    await page.goto(`/#settings=${old}`);
    await expect(page.locator("#push-h")).toHaveText("Phones and browsers");
    await expect(page.locator("#push-h")).toBeInViewport();
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
  await expect(section.locator("h2")).toHaveText("Remote sessions3");
  const rows = section.locator("[data-remote-session]");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toHaveAttribute("data-remote-session", "open");
  await expect(rows.nth(0)).toContainText("lee");
  await expect(rows.nth(0)).toContainText("Open");
  await expect(rows.nth(1)).toContainText("for 12m");
  await expect(rows.nth(2)).toContainText("until the daemon stopped");
  await expect(section.locator("[data-remote-input]")).toHaveAttribute("data-remote-input", "off");
  // The index lists it with the page's other sections.
  await expect(page.locator(".settings-index a", { hasText: "Remote sessions" })).toHaveAttribute("href", "#settings=remote-sessions");
});

test("a daemon without the log's route shows no Remote sessions, and no error", async ({ page }) => {
  await reset(page);
  await page.route("**/api/remote-sessions", (r) => r.fulfill({ status: 404, json: { error: "no such route" } }));
  await page.goto("/#settings=machine");
  await expect(page.locator('[data-settings-section="machine"]')).toBeVisible();
  await expect(page.locator('[data-settings-section="remote-sessions"]')).toHaveCount(0);
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
    await expect(settings.locator("[data-appearance-controls]")).toBeVisible();
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
    ["#settings=appearance", "/settings/you/appearance"],
    ["#settings=preferences", "/settings/you/appearance"],
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
