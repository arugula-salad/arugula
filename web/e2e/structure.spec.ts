// M3 in the browser: command marks from the shell integration, attention
// badges, and the switches that go with them.

import { devices, expect, test } from "@playwright/test";
import { active, menu, menuItem, panes, paneEl, ready, reset, tab, tabsInSession, text, type } from "./helpers";

test("finished commands get marks; a mark selects its output and runs again", async ({ page }) => {
  await reset(page);
  const pane = await active(page);
  await type(page, pane, "echo good-$((1+1))\n");
  await expect.poll(() => text(page, pane)).toContain("good-2");
  await type(page, pane, "echo bad-$((2+2)); false\n");
  await expect(paneEl(page, pane).locator(".cmd-mark.fail")).toHaveCount(1);
  await expect(paneEl(page, pane).locator(".cmd-mark.ok")).toHaveCount(1);
  await expect(paneEl(page, pane).locator(".cmd-mark.fail")).toHaveAttribute("title", /false · exit 1/);

  // Click: the command's output is selected.
  await paneEl(page, pane).locator(".cmd-mark.fail").click();
  await expect.poll(() => page.evaluate((p) => window.__arugula.selection(p).trim(), pane)).toBe("bad-4");

  // Right-click: run it again.
  await paneEl(page, pane).locator(".cmd-mark.ok").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Run again" }).click();
  await expect.poll(async () => (await text(page, pane)).match(/^good-2$/gm)?.length).toBe(2);
  await expect(paneEl(page, pane).locator(".cmd-mark.ok")).toHaveCount(2);
});

test("a background tab that wants you gets a badge until you look", async ({ page }) => {
  await reset(page);
  const [t1] = await tabsInSession(page);
  const pane = await active(page);
  // Notify (OSC 9) a moment after we've moved to another tab.
  await type(page, pane, "sleep 1; printf '\\e]9;ready for you\\a'\n");
  await page.getByTitle("New tab").click();
  await expect.poll(() => tabsInSession(page)).toHaveLength(2);
  const badge = page.locator(`[data-tab-id="${t1}"] .att.needs_input`);
  await expect(badge).toBeVisible({ timeout: 10_000 });

  // Back to it and type: answered.
  await page.locator(`[data-tab-id="${t1}"]`).click();
  await expect.poll(() => tab(page).then((t) => t.id)).toBe(t1);
  await type(page, pane, "true\n");
  await expect(badge).toBeHidden();
});

test("a right-click is the pane's menu over a program that takes the mouse; Shift sends it on", async ({ page }) => {
  await reset(page);
  const pane = await active(page);
  await type(page, pane, "printf 'mouse-on\\n\\e[?1000h\\e[?1006h'; cat -v\n");
  await expect.poll(() => text(page, pane)).toContain("mouse-on\n");
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 } });
  await expect(page.getByRole("menuitem", { name: "Split right" })).toBeVisible();
  await page.keyboard.press("Escape");
  // cat -v shows the reports it was sent: none yet, then a plain right
  // button's press and release.
  expect(await text(page, pane)).not.toContain("^[[<");
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 }, modifiers: ["Shift"] });
  await expect.poll(() => text(page, pane)).toMatch(/\^\[\[<2;\d+;\d+M\^\[\[<2;\d+;\d+m/);
  await expect(page.getByRole("menu")).toHaveCount(0);
  await page.keyboard.press("Control+c");
  await type(page, pane, "printf '\\e[?1000l\\e[?1006l'\n");
});

test("shell integration can be switched off for a pane's new shells", async ({ page }) => {
  await reset(page);
  const pane = await active(page);
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 } });
  const item = await menuItem(page, /Shell integration/);
  await expect(item).toHaveAttribute("aria-checked", "true");
  await item.click();
  await expect
    .poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.integration, pane))
    .toBe(false);
  // A split from it inherits the choice: no marks there.
  await menu(page, paneEl(page, pane), "Split right");
  await expect.poll(() => panes(page)).toHaveLength(2);
  const split = await active(page);
  await ready(page, split);
  await type(page, split, "false\n");
  await page.waitForTimeout(500);
  await expect(paneEl(page, split).locator(".cmd-mark")).toHaveCount(0);
});

// M113 (#1028): on a computer, the workspace is the only shell: each part
// there, and named for a screen reader.
test("the workspace's parts: rail, header, sessions, tabs, toolbar, footer", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await reset(page);
  await expect(page.getByRole("navigation", { name: "Places" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Where you are" })).toBeVisible();
  await expect(page.getByRole("complementary", { name: /^Sessions on / })).toBeVisible();
  await expect(page.getByRole("tablist", { name: /^Tabs in / })).toBeVisible();
  await expect(page.getByRole("toolbar", { name: "Panes" })).toBeVisible();
  await expect(page.locator("[data-status-footer]")).toBeVisible();
  // No classic bar, and no Appearance switch back to it.
  await expect(page.locator(".app > .bar, .app-bar, [data-workspace-choice]")).toHaveCount(0);
  // The header, the tab strip and the toolbar sit above the panes; the
  // footer below; the rail and the sidebar to their left.
  const main = (await page.locator(".main").boundingBox())!;
  for (const sel of [".ws-head", ".ws-tabs", "[data-pane-toolbar]"]) expect((await page.locator(sel).boundingBox())!.y).toBeLessThan(main.y);
  expect((await page.locator("[data-status-footer]").boundingBox())!.y).toBeGreaterThanOrEqual(main.y + main.height - 1);
  for (const sel of [".ws-rail", "[data-session-sidebar]"]) {
    const b = (await page.locator(sel).boundingBox())!;
    expect(b.x + b.width).toBeLessThanOrEqual(main.x + 1);
  }
});

test("the account menu offers notifications for this device", async ({ page }) => {
  await reset(page);
  await page.locator(".ws-head [data-account-menu]").click();
  await expect(page.getByRole("menuitemradio", { name: "Notify this device" })).toBeVisible();
  // The service worker that shows them is registered.
  await expect.poll(() => page.evaluate(async () => !!(await navigator.serviceWorker.getRegistration()))).toBe(true);
});

test.describe("phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the sheet lists panes that need you", async ({ page }) => {
    await reset(page);
    // M113: a phone keeps its own header; none of the workspace's chrome.
    await expect(page.locator(".phone-bar")).toBeVisible();
    await expect(page.locator(".ws-rail, .ws-head, [data-session-sidebar], [data-pane-toolbar], [data-status-footer]")).toHaveCount(0);
    const pane = await active(page);
    await page.evaluate(
      (p) => fetch(`/api/panes/${p}/attention`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ state: "needs_input" }) }),
      pane,
    );
    await expect(page.locator(".sheet-button .att.needs_input")).toBeVisible();
    await page.locator(".sheet-button").click();
    await expect(page.locator(".needs-you .sheet-item")).toHaveCount(1);
  });
});
