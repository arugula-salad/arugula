// M110 (#1025): the pane toolbar's presets and Focus.
// Each rebuilds the tab's split tree in the daemon (`arrange`, `focus`,
// M109): the same panes, the same processes and their scrollback, only
// placed anew; focus gives back exactly the layout it hid.

import { expect, test, type Page } from "@playwright/test";
import type { Node, PaneId } from "../src/proto";
import { active, closeContexts, paneEl, panes, ready, reset, run, tab, text } from "./helpers";

test.afterAll(closeContexts);
test.use({ viewport: { width: 1440, height: 900 } });

async function newWorkspace(page: Page) {
  await reset(page);
  await expect(page.locator(".ws-rail")).toBeVisible();
}

/** Every pane of the shown tab, in reading order (also the ones a focused
 * tab hides). */
const allPanes = (page: Page) =>
  page.evaluate(() => {
    const out: number[] = [];
    const walk = (n: Node) => (n.type === "pane" ? out.push(n.pane) : n.children.forEach((c) => walk(c.node)));
    walk(window.__arugula.client.tabView()!.root);
    return out;
  });

/** A tab of `n` shells, each having printed a marker of its own. */
async function shells(page: Page, n: number): Promise<Map<PaneId, string>> {
  const marks = new Map<PaneId, string>();
  const first = await active(page);
  await run(page, first, "echo mark-$((100+1))", "mark-101");
  marks.set(first, "mark-101");
  for (let i = 2; i <= n; i++) {
    const before = await panes(page);
    const from = before[before.length - 1];
    await page.evaluate(([pane, edge]) => window.__arugula.client.intent({ op: "split", pane, edge }), [from, i % 2 ? "bottom" : "right"] as const);
    await expect.poll(() => panes(page)).toHaveLength(i);
    const made = (await panes(page)).find((p) => !before.includes(p))!;
    await ready(page, made);
    await run(page, made, `echo mark-$((100+${i}))`, `mark-${100 + i}`);
    marks.set(made, `mark-${100 + i}`);
  }
  return marks;
}

async function kept(page: Page, marks: Map<PaneId, string>) {
  expect((await allPanes(page)).slice().sort()).toEqual([...marks.keys()].sort());
  for (const [id, mark] of marks) expect(await text(page, id)).toContain(mark);
}

/** The shape of a tree: a pane's id, or a split's direction and children. */
type Shape = number | { dir: string; of: Shape[] };
const shape = (page: Page) =>
  page.evaluate(() => {
    const walk = (n: Node): unknown => (n.type === "pane" ? n.pane : { dir: n.dir, of: n.children.map((c) => walk(c.node)) });
    return walk(window.__arugula.client.tabView()!.root) as Shape;
  });

test("every preset and Focus keep the panes and what they printed", async ({ page }) => {
  await newWorkspace(page);
  const marks = await shells(page, 6);
  const ids = await allPanes(page);
  const toolbar = page.locator("[data-pane-toolbar]");
  await expect(toolbar.locator("[data-pane-count]")).toHaveAttribute("data-pane-count", "6");

  // Columns: one row of six.
  await toolbar.locator('[data-preset="columns"]').click();
  await expect.poll(() => shape(page)).toEqual({ dir: "row", of: ids });
  await kept(page, marks);

  // Grid: ⌈√6⌉ = 3 columns, filled row by row.
  await toolbar.locator('[data-preset="grid"]').click();
  await expect.poll(() => shape(page)).toEqual({ dir: "column", of: [{ dir: "row", of: ids.slice(0, 3) }, { dir: "row", of: ids.slice(3) }] });
  await kept(page, marks);

  // Main + stack: the active pane large, the rest stacked in reading order.
  const main = ids[3];
  await page.evaluate((p) => window.__arugula.client.setActive(p), main);
  await toolbar.locator('[data-preset="main_stack"]').click();
  await expect.poll(() => shape(page)).toEqual({ dir: "row", of: [main, { dir: "column", of: ids.filter((p) => p !== main) }] });
  await kept(page, marks);
  // The terminals agree with what's drawn (the arrangement resized them).
  await run(page, main, "echo after-$((6*7))", "after-42");

  // Focus fills the tab with the active pane, for everyone; a second
  // click gives back the very same tree.
  const before = await shape(page);
  await toolbar.locator("[data-pane-focus]").click();
  await expect.poll(async () => (await tab(page)).focus).toBe(main);
  await expect.poll(() => panes(page)).toEqual([main]);
  await expect(toolbar.locator("[data-pane-focus]")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("[data-pane-header]")).toHaveCount(0);
  await kept(page, marks);
  await toolbar.locator("[data-pane-focus]").click();
  await expect.poll(async () => (await tab(page)).focus).toBeNull();
  expect(await shape(page)).toEqual(before);
  await expect.poll(() => panes(page)).toHaveLength(6);
  await kept(page, marks);

  // The header's expand is the same operation, and Ctrl+Shift+Enter too.
  const other = ids[1];
  await page.locator(`[data-pane-expand="${other}"]`).click();
  await expect.poll(async () => (await tab(page)).focus).toBe(other);
  await paneEl(page, other).click({ position: { x: 40, y: 40 } });
  await page.keyboard.press("Control+Shift+Enter");
  await expect.poll(async () => (await tab(page)).focus).toBeNull();
  expect(await shape(page)).toEqual(before);
  await kept(page, marks);
});

test("the pane count opens a switcher that goes to a pane", async ({ page }) => {
  await newWorkspace(page);
  const marks = await shells(page, 4);
  const ids = [...marks.keys()];
  await page.locator("[data-pane-count]").click();
  const items = page.locator(".menu [role=menuitemradio]");
  await expect(items).toHaveCount(4);
  await items.nth(2).click();
  await expect.poll(() => active(page)).toBe((await allPanes(page))[2]);
  expect(ids).toContain(await active(page));
});

test("+ Pane says where before it opens a shell, then gives it the keys", async ({ page }) => {
  await newWorkspace(page);
  const first = await active(page);
  await run(page, first, "cd /tmp && echo here-$((3+4))", "here-7");
  await page.keyboard.press("Control+Shift+E");
  const drawer = page.locator("[data-add-pane-drawer]");
  await expect(drawer).toBeVisible();
  await drawer.locator('[data-place="down"]').click();
  await expect(drawer.locator("[data-add-pane-destination]")).toContainText("Shell on");
  await expect(drawer.locator("[data-add-pane-destination]")).toContainText("in /tmp, below pane 1");
  await drawer.locator("[data-add-pane-create]").click();
  await expect(drawer).toBeHidden();
  await expect.poll(() => panes(page)).toHaveLength(2);
  const made = (await panes(page)).find((p) => p !== first)!;
  await expect.poll(() => active(page)).toBe(made);
  const t = await tab(page);
  const rects = Object.fromEntries(t.layout.panes);
  expect(rects[made].y).toBeGreaterThan(rects[first].y);
  await ready(page, made);
  // The keys are the new pane's: typing goes there without a click.
  await page.keyboard.type("pwd\n", { delay: 2 });
  await expect.poll(() => text(page, made)).toMatch(/\n\/tmp\s*\n/);
});
