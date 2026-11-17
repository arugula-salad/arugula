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
  for (const id of ["push", "appearance", "preferences", "machine"]) await expect(settings.locator(`[data-settings-section="${id}"]`)).toBeVisible();
  // Help is the command palette's, not a section here (Arugula #1108).
  await expect(settings.locator('[data-settings-section="help"]')).toHaveCount(0);

  // Each row under what it reaches.
  const machine = settings.locator('[data-settings-section="machine"]');
  await expect(machine.getByRole("button", { name: "Developer settings…" })).toBeVisible();
  await expect(machine.getByRole("button", { name: "Remove arugula.nvim" })).toBeVisible();
  await expect(settings.locator('[data-settings-section="appearance"]').getByRole("button", { name: "Appearance…" })).toBeVisible();
  await expect(settings.getByRole("button", { name: "Appearance…" })).toHaveCount(1);
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
  await expect(page.locator(".settings-index a", { hasText: "Preferences" })).toHaveAttribute("href", "#settings=preferences");
});

test("This device and All your devices are one section, Phones and browsers, and their old links land on it", async ({ page }) => {
  await reset(page);
  for (const old of ["device", "all-devices"]) {
    await page.goto(`/#settings=${old}`);
    await expect(page.locator("#push-h")).toHaveText("Phones and browsers");
    await expect(page.locator("#push-h")).toBeInViewport();
  }
});

test("Help is in the command palette, not in Settings", async ({ page }) => {
  await reset(page);
  await page.locator(".ws-head [data-open-palette]").click();
  const palette = page.getByRole("dialog", { name: "Command palette" });
  const filter = palette.getByRole("combobox", { name: "Command" });
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
