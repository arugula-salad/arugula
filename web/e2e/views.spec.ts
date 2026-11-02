// #503: a View shows panes it doesn't own. Two panes from different tabs both
// stream in one, closing it leaves the tabs alone, and a pane closed elsewhere
// says so in its leaf.

import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, menuItem, open, panes, ready, reset, tabsInSession } from "./helpers";

test.afterAll(closeContexts);
test.describe.configure({ mode: "serial" });

const leaves = (page: Page) => page.locator("[data-view-leaf]");

/** What a leaf's terminal shows, through the View's own connection. */
const leafText = (page: Page, pane: number) =>
  page.evaluate((pane) => {
    const w = window.__arugula as unknown as { viewLinks: { client(m: string): { panes: Map<number, { view: { text(): string } }> } | undefined }; hosts: { current: string; find(n: string): { id?: string } | undefined } };
    const h = w.hosts.current;
    return w.viewLinks.client(w.hosts.find(h)?.id ?? h)?.panes.get(pane)?.view.text() ?? "";
  }, pane);

/** Print into a pane through the page's main client: it isn't drawn while a
 * View is shown, so this proves the leaf streams by itself. */
const print = (page: Page, pane: number, word: string) =>
  page.evaluate(([pane, word]) => window.__arugula.client.input(pane as number, new TextEncoder().encode(`echo ${word}\r`)), [pane, word] as const);

async function addToNewView(page: Page, pane: number) {
  await page.locator(`[data-pane-more="${pane}"]`).click();
  await (await menuItem(page, "New view")).click();
}

test("two panes from two tabs in one View, both streaming, closing it closes nothing", async ({ page }) => {
  await reset(page);
  const first = await active(page);
  await page.evaluate(() => {
    const c = window.__arugula.client;
    c.intent({ op: "new_tab", session: c.session!, name: null });
  });
  await expect.poll(() => tabsInSession(page).then((t) => t.length)).toBe(2);
  await expect.poll(() => active(page)).not.toBe(first);
  const second = await active(page);
  await ready(page, second);

  // No form: the menu makes the View and opens it.
  await addToNewView(page, second);
  await expect(page).toHaveURL(/#view=[a-z0-9]+$/);
  await expect(leaves(page)).toHaveCount(1);
  const viewTab = page.locator("[data-view-tab]");
  await expect(viewTab).toHaveCount(1);

  // The other tab's pane joins it, from its own menu.
  await page.locator(".tab").first().click();
  await expect(page).not.toHaveURL(/#view=/);
  await expect.poll(() => active(page)).toBe(first);
  await page.locator(`[data-pane-more="${first}"]`).click();
  // The View is named from what's in it, and listed above New view.
  await page.locator(".menu .menu-parent", { hasText: "Add to view" }).click();
  await page.getByRole("menuitem", { name: "New view" }).waitFor();
  await page.getByRole("menuitem").filter({ hasText: (await viewTab.innerText()).replace("⧉", "").trim() }).first().click();
  await viewTab.click();
  await expect(leaves(page)).toHaveCount(2);

  // Both stream, though neither tab is on screen.
  await print(page, first, "from-first-tab");
  await print(page, second, "from-second-tab");
  await expect.poll(() => leafText(page, first)).toContain("from-first-tab");
  await expect.poll(() => leafText(page, second)).toContain("from-second-tab");

  // Typing in a leaf reaches its pane.
  await page.locator(`[data-view-leaf$=".${first}"] .xterm`).click();
  await page.keyboard.type("echo typed-in-leaf\n");
  await expect.poll(() => leafText(page, first)).toContain("typed-in-leaf");

  // Reloaded, it's still there: it's kept.
  await page.reload();
  await expect(leaves(page)).toHaveCount(2);

  await expect.poll(() => page.evaluate(() => window.__arugula.client.state !== null)).toBe(true);
  const tabsBefore = await page.evaluate(() => window.__arugula.client.state!.tabs.length);
  await viewTab.click({ button: "right" });
  await (await menuItem(page, "Close view")).click();
  await expect(viewTab).toHaveCount(0);
  await expect(page).not.toHaveURL(/#view=/);
  expect(await page.evaluate(() => window.__arugula.client.state!.tabs.length)).toBe(tabsBefore);
  expect(await page.evaluate(() => window.__arugula.client.state!.panes.map((p) => p.id).sort())).toEqual([first, second].sort());
});

test("a View stays through a reload, and the rail's Panes leaves it", async ({ page }) => {
  await reset(page);
  const pane = await active(page);
  await ready(page, pane);
  await addToNewView(page, pane);
  await expect(leaves(page)).toHaveCount(1);
  // The first tab of a page opened at a View doesn't close it.
  await page.reload();
  await expect(leaves(page)).toHaveCount(1);
  await expect(page).toHaveURL(/#view=/);
  await page.locator("[data-open-panes]").first().click();
  await expect(page).not.toHaveURL(/#view=/);
  await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
  await page.locator("[data-view-tab]").click({ button: "right" });
  await (await menuItem(page, "Close view")).click();
});

test("a block carried or sent home doesn't resize its terminal", async ({ page }) => {
  await reset(page);
  const pane = await active(page);
  await ready(page, pane);
  await addToNewView(page, pane);
  await expect(leaves(page)).toHaveCount(1);
  const cols = () => page.evaluate((p) => window.__arugula.client.tabOfPane(p)?.cols ?? 0, pane);
  await expect.poll(cols).toBeGreaterThan(60);

  // Carried around for longer than a size takes to settle: still the place's.
  const placed = await cols();
  const head = (await leaves(page).locator(".view-leaf-head").boundingBox())!;
  await page.mouse.move(head.x + 60, head.y + 10);
  await page.mouse.down();
  await page.mouse.move(head.x + 200, head.y + 200, { steps: 6 });
  await page.waitForTimeout(600);
  expect(await cols()).toBe(placed);

  // Off the edge and home: the terminal keeps the size it had.
  await page.mouse.move(2, 2, { steps: 6 });
  await page.waitForTimeout(300);
  await page.mouse.up();
  await expect(leaves(page)).toHaveCount(0, { timeout: 10_000 });
  await page.waitForTimeout(400);
  expect(await cols()).toBeGreaterThan(40);
});

test("a pane closed elsewhere shows where it was, and Remove takes the leaf", async ({ page }) => {
  await reset(page);
  const keep = await active(page);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), keep);
  await expect.poll(() => panes(page).then((p) => p.length)).toBe(2);
  const other = (await panes(page)).find((p) => p !== keep)!;
  await ready(page, other);
  await addToNewView(page, other);
  await expect(leaves(page)).toHaveCount(1);

  await page.evaluate((p) => window.__arugula.client.intent({ op: "close_pane", pane: p }), other);
  const gone = page.locator("[data-view-gone]");
  await expect(gone).toContainText(`%${other} has closed on`, { timeout: 15_000 });
  await gone.getByRole("button", { name: "Remove" }).click();
  await expect(leaves(page)).toHaveCount(0);
  // Nothing's left of it: the View went with its last leaf.
  await expect(page.locator("[data-view-tab]")).toHaveCount(0);
});

test("a View doesn't take the size from the tab it came from for good", async ({ page }) => {
  await open(page);
  const pane = await active(page);
  await ready(page, pane);
  const sizeBefore = await page.evaluate(() => {
    const t = window.__arugula.client.tabView()!;
    return [t.cols, t.rows];
  });
  await addToNewView(page, pane);
  await expect(leaves(page)).toHaveCount(1);
  // This page's own size goes to the leaf...
  await expect.poll(() => page.locator("[data-sized-elsewhere]").count()).toBe(0);
  // ...and showing the tab again takes it back with no button to press.
  await page.locator(".tab").first().click();
  await expect(page).not.toHaveURL(/#view=/);
  await expect.poll(() => page.evaluate(() => {
    const t = window.__arugula.client.tabView()!;
    return t.owner === window.__arugula.client.clientId;
  })).toBe(true);
  await expect(page.locator("[data-sized-elsewhere]")).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => {
    const t = window.__arugula.client.tabView()!;
    return [t.cols, t.rows];
  })).toEqual(sizeBefore);
});
