// #139: the command palette in Chrome, on a desktop and a phone. The
// checks are in palette-steps.ts, shared with palette.webkit.spec.ts.

import { devices, expect, test } from "@playwright/test";
import { desktop, docs, phone } from "./palette-steps";
import { active, paneEl, reset } from "./helpers";

test("the chord opens the palette from a terminal; it runs every pane-menu action", async ({ page }) => {
  await desktop(page);
});

test("Docs opens the Docs page, and a Doc that was closed opens by its title", async ({ page }) => {
  await docs(page);
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });
  test("a full-height sheet from the sheet's Commands button", async ({ page }) => {
    await phone(page);
  });
});

// M111 (#1026): the palette jumps to this machine's sessions, tabs and
// panes, a pane in another session included; Escape gives the focus back.
test("a jump to a pane in another session, and Escape gives the focus back", async ({ page }) => {
  await reset(page);
  const home = await page.evaluate(() => window.__arugula.client.session!);
  const first = await active(page);
  await page.evaluate(() => window.__arugula.client.intent({ op: "new_session", name: "jumps", from_pane: null }));
  let sid: number | undefined;
  await expect.poll(async () => (sid = await page.evaluate(() => window.__arugula.client.state!.sessions.find((s) => s.name === "jumps")?.id))).not.toBeUndefined();
  const one = await page.evaluate((s) => {
    const c = window.__arugula.client;
    const tab = c.tabView(c.state!.sessions.find((x) => x.id === s)!.tabs[0])!;
    return tab.layout.panes[0][0];
  }, sid!);
  // Two panes in it, so the jump picks one of them, not just the session.
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), one);
  let target: number | undefined;
  await expect
    .poll(async () => (target = await page.evaluate((p) => window.__arugula.client.tabOfPane(p)!.layout.panes.map(([id]) => id).find((id) => id !== p), one)))
    .not.toBeUndefined();
  // Back in the first session, from its terminal.
  await page.evaluate((s) => window.__arugula.client.selectSession(s), home);
  await expect.poll(() => active(page)).toBe(first);
  await paneEl(page, first).click({ position: { x: 60, y: 60 } });

  const mod = process.platform === "darwin" ? "Meta" : "Control";
  const palette = page.getByRole("dialog", { name: "Command palette" });
  const filter = palette.getByRole("combobox", { name: "Command" });
  await page.keyboard.press(`${mod}+Shift+KeyP`);
  await filter.fill("jumps");
  const key = `Go/pane ${target}`;
  const row = palette.locator(`[data-command="${key}"]`);
  await expect(row).toBeVisible();
  const at = await palette.getByRole("option").evaluateAll((els, k) => els.findIndex((e) => (e as HTMLElement).dataset.command === k), key);
  for (let i = 0; i < at; i++) await filter.press("ArrowDown");
  await expect(row).toHaveAttribute("aria-selected", "true");
  await filter.press("Enter");
  await expect(palette).toBeHidden();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.session)).toBe(sid);
  await expect.poll(() => active(page)).toBe(target);
  // The keys are the pane's.
  await expect.poll(() => paneEl(page, target!).evaluate((el) => el.contains(document.activeElement))).toBe(true);

  // Opened and dismissed: the focus is back where it was.
  await page.keyboard.press(`${mod}+Shift+KeyP`);
  await expect(filter).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(palette).toBeHidden();
  await expect.poll(() => paneEl(page, target!).evaluate((el) => el.contains(document.activeElement))).toBe(true);
});
