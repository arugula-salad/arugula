// The lead arranges the board (Lift 5b, design lift §3.12; Arugula #1497):
// the board's lane groups are its default panes. The lead (MCP `show` with
// a place target) and you open Docs, PRs, runs and prompts beside them or in
// tabs, from the board's place-layout record (#1496), which the stream's
// `layout` event keeps live. The lead never takes the focus, a busy person
// gets its pane quietly with a dot, a fourth agent pane closes the oldest
// untouched one, a closed default stays closed until Reset, and what waits on
// you goes to a tab of its own with a Needs-you row pointing at it. On the
// app's frame fixture, with the cloud made up here (./place-layout-fake.ts).
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { fakeLayouts, type FakeLayouts } from "./place-layout-fake";
import { reset } from "./helpers";

const P = "prj_aaaa0002";
const B = "brd_0d01";
const PLACE = `board:${B}`;

test.use({ permissions: ["local-network-access"], viewport: { width: 1440, height: 900 } });

const row = (id: string, lane: string, number: number, title: string, group: string) => ({ id, rev: 1, group, pointer: { kind: "issue", id: `iss_${number}` }, note: "", status: "none", lane, attachments: [], live: { state: "open", kind: "native", number, title }, created_at: 1, updated_at: 1 });
const pr = { id: "att_pr", role: "work", pointer: { kind: "forge_item", id: "fi_2868" }, live: { state: "checks_running", kind: "pr", repo: "arugula-salad/arugula", number: 2868, title: "Sidebar caret sits beside the crumb" } };
const rows = {
  next: [row("row_1622", "next", 1622, "Chat: polls, as cards", "zon_ux"), row("row_89", "next", 89, "A tab created while hidden", "zon_fn")],
  doing: [row("row_1448", "doing", 1448, "Row hover shows actions", "zon_ux")],
  review: [{ ...row("row_1443", "review", 1443, "Sidebar caret beside the crumb", "zon_ux"), attachments: [pr] }],
  done: [],
};
const progress = { done: 0, total: 4, proven: 0, review: 1, doing: 1 };
const VIEW = {
  board: { id: B, project: P, title: "Product improvements", goal: "", created_by: "acct_a", created_at: 1, updated_at: 1, done_note: "", closed_by: "", progress, lead: { machine: "mch_dust", machine_name: "dust", session: "claude@dust", at: 1 } },
  zones: (["next", "doing", "review", "done"] as const).map((lane) => ({ id: `lane_${lane}`, rev: 1, title: lane, kind: "lane", rows: rows[lane] })),
  groups: [
    { id: "zon_ux", rev: 1, title: "UX", progress },
    { id: "zon_fn", rev: 1, title: "Functionality", progress },
  ],
  docs: [{ id: "bdc_1", pointer: { kind: "doc", id: "pdc_design" }, title: "Design pass", sections: 1, written: 1, added_by: "acct_a", created_at: 1 }],
};
const DOC = { kind: "doc" as const, ref: "pdc_design", title: "Design pass" };
const doc = (n: number) => ({ kind: "doc" as const, ref: `pdc_${n}`, title: `Doc ${n}` });

/** The board's page, its layout served by a fake platform; `before` runs ahead of the page (a record already arranged). */
async function openBoard(page: Page, before?: (l: FakeLayouts) => void): Promise<FakeLayouts> {
  await reset(page);
  await page.route(/\/api\/projects\/prj_[0-9a-f]+\/boards$/, (r) => r.fulfill({ json: { project: P, boards: r.request().url().includes(P) ? [VIEW.board] : [] } }));
  await page.route(`**/api/boards/${B}`, (r) => r.fulfill({ json: VIEW }));
  await page.route(`**/api/projects/${P}/ci-alerts`, (r) => r.fulfill({ json: { project: P, on: false, url: "", people: [], assign: false, key_made_by: null, key_made_at: null, last_event_at: null, open: [] } }));
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => r.fulfill({ json: { project: P, groups: [] } }));
  await page.route(`**/api/projects/${P}/docs/*`, (r) => {
    const id = new URL(r.request().url()).pathname.split("/").pop()!;
    const d = { id, project: P, title: id === "pdc_design" ? "Design pass" : id, author: "acct_a", agent: null, sections: 1, written: 1, rev: 1, created_at: 1, updated_at: 1 };
    return r.fulfill({ json: { doc: d, sections: [{ id: "s1", heading: "Flat by default", intent: "", body: "A third of the width for Needs you.", rev: 1 }], assets: [], writer: true, deleter: true, link: null } });
  });
  const l = await fakeLayouts(page, PLACE);
  before?.(l);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route((u) => u.pathname.startsWith("/p/"), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(`/p/${P}/boards/${B}`);
  await expect(page.locator(`[data-board-desk="${B}"]`)).toBeVisible();
  await expect(page.locator('[data-board-default="zon_ux"]')).toBeVisible();
  return l;
}

const tabs = (page: Page) => page.locator("[data-board-tabs] [role=tab]");
const pane = (page: Page, title: string) => page.locator(`[data-layout-pane]`, { has: page.locator(".k-pane-title", { hasText: title }) });
/** Long enough since the last key or click that the person isn't busy. */
const idle = (page: Page) => page.waitForTimeout(4_300);

test("the board's groups are its default panes; a PR from the fold opens beside them, yours and focused, and the right column narrows to a quarter", async ({ page }, info) => {
  const l = await openBoard(page);
  await expect(tabs(page)).toHaveText(["Product improvements1"]);
  // The defaults are quiet: their own headings, no title row.
  await expect(page.locator('[data-board-default="zon_ux"] .k-pane-title')).toHaveCount(0);
  const side = async () => (await page.locator(".board-desk-side").boundingBox())!.width;
  const wide = await side();
  await page.locator('[data-board-pr="fi_2868"] .k-row-line').click();
  await expect.poll(() => l.posted).toEqual([{ op: "show", pane: { kind: "pr", ref: "arugula-salad/arugula#2868", title: "#2868 Sidebar caret sits beside the crumb", machine: null, state: null }, intent: "beside", tab: "home", in_place_of: null }]);
  const p = pane(page, "#2868");
  await expect(p).toHaveAttribute("data-pane-by", "you");
  await expect(p).toHaveAttribute("data-pane-focused", "");
  await expect(p.locator('[data-board-drawer="row_1443"]')).toBeVisible();
  // The defaults compact beside it, and show their title rows.
  await expect(page.locator('[data-board-default="zon_ux"]')).toHaveAttribute("data-pane-compact", "");
  await expect(page.locator(`[data-board-desk="${B}"]`)).toHaveAttribute("data-board-beside", "");
  const desk = (await page.locator(`[data-board-desk="${B}"]`).boundingBox())!.width;
  await expect.poll(side).toBeLessThan(wide);
  expect(await side()).toBeLessThan(desk * 0.27);
  // "changed by you · Reset", while it differs from the default.
  await expect(page.locator("[data-board-changed]")).toContainText("changed by you");
  await page.screenshot({ path: info.outputPath("board-pr-beside.png") });
  await p.hover();
  await p.locator("[data-pane-close]").click();
  await expect(page.locator("[data-layout-pane]")).toHaveCount(0);
  await expect(page.locator(`[data-board-desk="${B}"]`)).not.toHaveAttribute("data-board-beside", "");
});

test("the lead opens a pane without taking the focus or the tab; busy in the last 4 seconds, it arrives quietly with a dot", async ({ page }, info) => {
  const l = await openBoard(page);
  await idle(page);
  // Idle: it slides in, with no dot, and the keys stay where they were.
  await page.keyboard.press("/");
  const find = page.locator("[data-board-find-input] input, input[data-board-find-input]").first();
  await expect(find).toBeFocused();
  await page.evaluate(() => (document.activeElement as HTMLElement).blur());
  await idle(page);
  await l.lead(DOC);
  const d = pane(page, "Design pass");
  await expect(d).toBeVisible();
  await expect(d).toHaveAttribute("data-pane-by", "lead");
  await expect(d.locator("[data-pane-dot]")).toHaveCount(0);
  await expect(d).not.toHaveAttribute("data-pane-focused", "");
  await expect(d.locator(".k-pane-meta")).toHaveText("lead");
  await expect(page.locator("[data-board-changed]")).toContainText("changed by lead");
  // Busy: typing in the find box, then the lead's next pane comes with a dot and the box keeps the keys.
  await page.keyboard.press("/");
  await expect(find).toBeFocused();
  await page.keyboard.type("caret");
  await l.lead({ kind: "run", ref: "run_deploy", title: "Run · deploy-preview" });
  const run = pane(page, "Run · deploy-preview");
  await expect(run.locator("[data-pane-dot]")).toBeVisible();
  await expect(find).toBeFocused();
  await expect(find).toHaveValue("caret");
  await page.screenshot({ path: info.outputPath("board-busy-dot.png") });
  // Looking at it takes the dot away.
  await page.keyboard.press("Escape");
  await run.click();
  await expect(run.locator("[data-pane-dot]")).toHaveCount(0);
});

test("a fourth agent pane closes the oldest the lead opened and you haven't touched; yours stay", async ({ page }) => {
  const l = await openBoard(page);
  await l.lead(doc(1));
  await l.lead(doc(2));
  await l.lead(doc(3));
  // You maximise Doc 1 (f, then esc): it's yours now.
  await pane(page, "Doc 1").click();
  await page.keyboard.press("f");
  await expect(page.locator("[data-panes-columns]")).toHaveAttribute("data-panes-columns", "1");
  await expect(page.locator('[data-board-default="zon_ux"]')).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-board-default="zon_ux"]')).toBeVisible();
  await expect.poll(() => l.posted.map((c) => c.op)).toContain("maximise");
  await l.lead(doc(4));
  await expect(page.locator("[data-layout-pane] .k-pane-title")).toHaveText(["Doc 1", "Doc 3", "Doc 4"]);
  // What was closed is on the ⋯'s closed list.
  await page.locator("[data-board-more]").click();
  await expect(page.locator(`[role=menu] [data-board-reopen="${l.layout.closed[0]!.id}"]`)).toHaveText("Reopen Doc 2");
});

test("at 1440 three opened panes stack rather than shrink the board below its width", async ({ page }, info) => {
  const l = await openBoard(page);
  for (const n of [1, 2, 3]) await l.lead(doc(n));
  await expect(page.locator("[data-layout-pane]")).toHaveCount(3);
  // The board and two beside it: not four columns.
  await expect(page.locator("[data-panes-columns]")).toHaveAttribute("data-panes-columns", "3");
  for (const c of await page.locator("[data-panes-col]").all()) expect((await c.boundingBox())!.width).toBeGreaterThanOrEqual(279);
  // The third stacks under the second.
  await expect(page.locator('[data-panes-col="2"] [data-layout-pane]')).toHaveCount(2);
  await page.mouse.move(5, 5);
  await expect(pane(page, "Doc 3").locator(".k-pane-title")).toBeVisible();
  await page.screenshot({ path: info.outputPath("board-three-panes-1440.png"), animations: "disabled" });
});

test("a default pane you close stays closed while the lead opens more, until Reset; Reset sends what was opened to the closed list", async ({ page }) => {
  const l = await openBoard(page);
  await l.lead(DOC);
  const ux = page.locator('[data-board-default="zon_ux"]');
  await ux.hover();
  await ux.locator("[data-pane-close]").click();
  await expect(ux).toHaveCount(0);
  await l.lead(doc(2));
  await expect(page.locator("[data-layout-pane]")).toHaveCount(2);
  await expect(ux).toHaveCount(0);
  await page.locator("[data-board-reset]").click();
  await expect(ux).toBeVisible();
  await expect(page.locator("[data-layout-pane]")).toHaveCount(0);
  await expect(page.locator("[data-board-changed]")).toHaveCount(0);
  // And Reopen brings one back from the closed list.
  await page.locator("[data-board-more]").click();
  await page.locator(`[role=menu] [data-board-reopen]`, { hasText: "Design pass" }).click();
  await expect(pane(page, "Design pass")).toBeVisible();
});

test("the lead shows a run in place of a group for a while; ↺ puts the group back", async ({ page }) => {
  const l = await openBoard(page);
  await l.lead({ kind: "run", ref: "run_main", title: "Run · main" }, "beside", "zon_ux");
  const run = pane(page, "Run · main");
  await expect(run.locator(".k-pane-meta")).toHaveText("in place of UX · lead");
  await expect(page.locator('[data-board-default="zon_ux"]')).toHaveCount(0);
  await run.hover();
  await run.locator('[data-pane-restore="zon_ux"]').click();
  await expect(page.locator('[data-board-default="zon_ux"]')).toBeVisible();
});

test("for_you: a prompt opens a tab of its own that never becomes current, with a Needs-you row pointing at it; answered, both go", async ({ page }, info) => {
  const l = await openBoard(page);
  await page.locator('[data-board-default="zon_ux"]').click();
  const r = await l.lead({ kind: "prompt", ref: "%7", title: "Allow git clone", machine: "dust" }, "for_you");
  const item = { id: "fy_1", kind: "for_you", group: "decide", counts: true, title: "Allow git clone", lines: [], from: { agent: "claude@dust", machine: "mch_dust" }, subject: { type: "tab", id: r.tab, url: `/p/${P}/boards/${B}`, project: P, place: PLACE, tab: r.tab }, since: Date.now() };
  await page.evaluate((items) => (window as unknown as { __inboxSay(i: unknown[]): void }).__inboxSay(items), [item]);
  await expect(tabs(page)).toHaveCount(2);
  await expect(tabs(page).nth(1)).toHaveAttribute("aria-selected", "false");
  await expect(tabs(page).nth(1).locator("[data-tab-dot]")).toBeVisible();
  await expect(page.locator('[data-board-default="zon_ux"]')).toBeVisible();
  const row = page.locator('[data-board-for-you]');
  await expect(row).toContainText("Allow git clone");
  await expect(row).toContainText("tab 2");
  await page.screenshot({ path: info.outputPath("board-for-you.png") });
  // You go there from the row: the prompt pane, answered in place.
  await row.locator("[data-board-item-tab]").click();
  await expect(tabs(page).nth(1)).toHaveAttribute("aria-selected", "true");
  const p = page.locator('[data-pane-kind="prompt"]');
  await expect(p).toBeVisible();
  await expect(p.locator(".k-mark")).toBeVisible();
  // Answered (on its machine): the platform closes the pane and its tab, and the row goes.
  l.layout.tabs = l.layout.tabs.filter((t) => t.id !== r.tab);
  l.layout.rev++;
  await l.say();
  await page.evaluate(() => (window as unknown as { __inboxSay(i: unknown[]): void }).__inboxSay([]));
  await expect(tabs(page)).toHaveCount(1);
  await expect(tabs(page).first()).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("[data-board-for-you]")).toHaveCount(0);
});

test("keys: 1–6 tabs, [ ] focus panes, w closes one, f maximises and esc comes back, W closes the tab", async ({ page }) => {
  const l = await openBoard(page);
  await l.lead(DOC);
  await l.lead({ kind: "run", ref: "run_deploy", title: "Run · deploy-preview" }, "for_you");
  await page.locator("body").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("]");
  await expect(page.locator('[data-board-default="zon_ux"]')).toHaveAttribute("data-pane-focused", "");
  await page.keyboard.press("[");
  await expect(pane(page, "Design pass")).toHaveAttribute("data-pane-focused", "");
  await page.keyboard.press("f");
  await expect(page.locator("[data-panes-columns]")).toHaveAttribute("data-panes-columns", "1");
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-board-default="zon_ux"]')).toBeVisible();
  await page.keyboard.press("w");
  await expect(pane(page, "Design pass")).toHaveCount(0);
  await expect.poll(() => l.posted.at(-1)).toMatchObject({ op: "close" });
  await page.keyboard.press("2");
  await expect(tabs(page).nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(pane(page, "Run · deploy-preview")).toBeVisible();
  await page.keyboard.press("Shift+W");
  await expect(tabs(page)).toHaveCount(1);
  await expect.poll(() => l.posted.at(-1)).toMatchObject({ op: "close_tab" });
  // The keys are on the ? sheet.
  const ids = await page.evaluate(() => (window as unknown as { __arugulaKeys(): string[] }).__arugulaKeys());
  for (const k of ["board.tab", "board.pane.next", "board.pane.prev", "board.pane.close", "board.tab.close", "board.pane.max", "board.pane.back"]) expect(ids).toContain(k);
});

test("the layout follows across reloads, and a moved pane is a tab of its own", async ({ page }) => {
  const l = await openBoard(page);
  await l.lead(DOC);
  const d = pane(page, "Design pass");
  await d.hover();
  await d.locator("[data-pane-move]").click();
  await expect(tabs(page)).toHaveCount(2);
  await expect(tabs(page).first()).toHaveAttribute("aria-selected", "true");
  await page.reload();
  await expect(tabs(page)).toHaveCount(2);
  await expect(tabs(page).nth(1)).toContainText("Design pass");
  await expect(page.locator("[data-layout-pane]")).toHaveCount(0);
  await page.keyboard.press("2");
  await expect(pane(page, "Design pass")).toBeVisible();
  expect(l.layout.tabs[1]!.panes[0]!.touched).toBe(true);
});

test("a linked Doc opens from + pane as yours, and its ⋯ takes it off the board (Arugula #1524)", async ({ page }) => {
  const gone: string[] = [];
  await page.route(`**/api/boards/${B}/docs/bdc_1`, (r) => (gone.push(r.request().method()), r.fulfill({ json: { ...VIEW, docs: [] } })));
  await openBoard(page);
  await page.locator("[data-board-add-pane-menu]").click();
  await page.locator('[role=menu] [data-board-add-pane="bdc_1"]').click();
  const d = pane(page, "Design pass");
  await expect(d).toHaveAttribute("data-pane-by", "you");
  await d.locator("[data-doc-more]").click();
  await page.locator('[role=menu] [data-board-doc-remove="bdc_1"]').click();
  await expect.poll(() => gone).toEqual(["DELETE"]);
  await expect(d).toHaveCount(0);
});
