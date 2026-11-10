// Starred tabs and panes (#854): Star tab in a tab's menu and Star pane in
// a pane's put a ★ on the tab and the pane's header, and list them in the
// Sessions sidebar's Starred group, below Needs you, oldest first. A row
// jumps there, from over a page too; unstarring takes it out. A star is
// the daemon's (an option on the tab or pane), so a second client sees it
// at once. The viewer's half is in access.spec.ts, the phone's in
// stars-phone.spec.ts.

import { expect, test, type Page } from "@playwright/test";
import { active, menuItem, open, panes, ready, reset, tab } from "./helpers";

test.use({ viewport: { width: 1440, height: 900 } });

const tabEl = (page: Page, id: number) => page.locator(`.ws-tabs [data-tab-id="${id}"]`);
const header = (page: Page, pane: number) => page.locator(`[data-pane-header="${pane}"]`);
const starred = (page: Page) => page.locator("[data-starred]");
const starredKeys = (page: Page) => starred(page).evaluateAll((els) => els.map((e) => e.getAttribute("data-starred")));

async function pick(page: Page, name: string) {
  await (await menuItem(page, name, { exact: true })).click();
}

/** A tab with two panes, and a second tab: [tab, its second pane, the other tab]. */
async function twoTabs(page: Page): Promise<[number, number, number]> {
  await reset(page);
  const [first] = await panes(page);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), first);
  await expect.poll(() => panes(page).then((p) => p.length)).toBe(2);
  const [, second] = await panes(page);
  const a = (await tab(page)).id;
  await page.evaluate(() => {
    const c = window.__arugula.client;
    c.intent({ op: "new_tab", session: c.session!, from_pane: null });
  });
  const tabs = () => page.evaluate(() => window.__arugula.client.state!.tabs.map((t) => t.id));
  await expect.poll(() => tabs().then((t) => t.length)).toBe(2);
  const b = (await tabs()).find((t) => t !== a)!;
  await page.evaluate((t) => window.__arugula.client.selectTab(t), a);
  await ready(page, second);
  return [a, second, b];
}

test("star a tab and a pane from their menus, jump to them from Starred, unstar them", async ({ page }) => {
  const [a, pane, b] = await twoTabs(page);
  await expect(page.locator('[data-session-group="starred"]')).toHaveCount(0);

  await tabEl(page, a).click({ button: "right" });
  await pick(page, "Star tab");
  await expect(tabEl(page, a).locator("[data-tab-star]")).toBeVisible();
  await expect(tabEl(page, b).locator("[data-tab-star]")).toHaveCount(0);

  await header(page, pane).click({ button: "right" });
  await pick(page, "Star pane");
  await expect(header(page, pane).locator("[data-pane-star]")).toBeVisible();

  // Starred, right below Needs you (or first, when nothing needs you), in
  // the order they were starred.
  const group = page.locator('[data-session-group="starred"]');
  await expect(group).toContainText("Starred");
  await expect.poll(() => starredKeys(page)).toEqual([`tab:${a}`, `pane:${pane}`]);
  const groups = await page.locator("[data-session-group]").evaluateAll((els) => els.map((e) => e.getAttribute("data-session-group")));
  expect(groups.indexOf("starred")).toBe(groups[0] === "needs" ? 1 : 0);
  // Under the name, where it is: the session.
  const session = await page.evaluate(() => {
    const c = window.__arugula.client;
    return c.state!.sessions.find((s) => s.id === c.session)!.name;
  });
  await expect(page.locator(`[data-starred="pane:${pane}"] .ws-session-sub`)).toContainText(session);

  // A click lands on it.
  await page.evaluate((t) => window.__arugula.client.selectTab(t), b);
  await page.locator(`[data-starred="pane:${pane}"] .ws-session-main`).click();
  await expect.poll(() => tab(page).then((t) => t.id)).toBe(a);
  await expect.poll(() => active(page)).toBe(pane);
  await expect(page.locator(`[data-starred="pane:${pane}"] .ws-session-main`)).toHaveAttribute("aria-current", "true");
  await page.evaluate((t) => window.__arugula.client.selectTab(t), b);
  await page.locator(`[data-starred="tab:${a}"] .ws-session-main`).click();
  await expect.poll(() => tab(page).then((t) => t.id)).toBe(a);

  // Unstarred, it leaves; with nothing starred the group goes.
  await tabEl(page, a).click({ button: "right" });
  await pick(page, "Unstar tab");
  await expect(tabEl(page, a).locator("[data-tab-star]")).toHaveCount(0);
  await expect.poll(() => starredKeys(page)).toEqual([`pane:${pane}`]);
  await header(page, pane).click({ button: "right" });
  await pick(page, "Unstar pane");
  await expect(header(page, pane).locator("[data-pane-star]")).toHaveCount(0);
  await expect(group).toHaveCount(0);
});

test("a starred row lands on its tab from over the overview", async ({ page }) => {
  const [a, , b] = await twoTabs(page);
  await page.evaluate((t) => window.__arugula.client.intent({ op: "set_option", scope: { kind: "tab", id: t }, name: "star", value: String(Date.now()) }), b);
  await expect(starred(page)).toHaveCount(1);
  await page.evaluate(() => (location.hash = "overview"));
  await expect(page.locator("[data-overview]")).toBeVisible();
  await page.locator(`[data-starred="tab:${b}"] .ws-session-main`).click();
  await expect(page.locator("[data-overview]")).toHaveCount(0);
  await expect.poll(() => tab(page).then((t) => t.id)).toBe(b);
  expect(b).not.toBe(a);
});

test("a second client sees a star appear, and go when its tab closes", async ({ page, browser }) => {
  const [, , b] = await twoTabs(page);
  const other = await (await browser.newContext({ viewport: { width: 1440, height: 900 } })).newPage();
  await open(other);
  await expect(other.locator("[data-starred]")).toHaveCount(0);
  await tabEl(page, b).click({ button: "right" });
  await pick(page, "Star tab");
  await expect(other.locator(`[data-starred="tab:${b}"]`)).toBeVisible();
  await expect(other.locator(`.ws-tabs [data-tab-id="${b}"] [data-tab-star]`)).toBeVisible();
  await page.evaluate((t) => window.__arugula.client.intent({ op: "close_tab", tab: t }), b);
  await expect(other.locator("[data-starred]")).toHaveCount(0);
  await other.context().close();
});

test("starred jumps come first among the palette's Go to", async ({ page }) => {
  const [, pane] = await twoTabs(page);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "set_option", scope: { kind: "pane", id: p }, name: "star", value: "1" }), pane);
  await expect(starred(page)).toHaveCount(1);
  await page.locator(`[data-pane="${pane}"]`).click({ position: { x: 40, y: 40 } });
  await page.keyboard.press("Control+Shift+KeyP");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();
  await palette.getByRole("combobox", { name: "Command" }).fill("go to");
  const rows = page.locator('.palette-row[data-command^="Go/"]');
  await expect(rows.first()).toHaveAttribute("data-command", `Go/pane ${pane}`);
  await expect(rows.first().locator(".star-mark")).toBeVisible();
});
