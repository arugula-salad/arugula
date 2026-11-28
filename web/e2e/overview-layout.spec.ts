// The Overview as the chief of staff's view (Lift 9, design lift §3.12;
// Arugula #1498), v7's story: tab 1 is the title line over its default
// panes, Boards and Running. The chief of staff (MCP `show` with
// `place: overview`) opens a thread and a Doc beside them without taking
// the focus or the tab, and Boards compacts to a list; busy, its pane
// arrives quietly with a dot; a review moves to a tab of its own with a
// Needs-you row pointing at it; a fourth pane closes its oldest untouched
// one; a failing run stands in place of Boards for a while, ↺ puts Boards
// back; Reset puts tab 1 back; a closed default pane stays closed; an
// agent's prompt waits in a tab of its own, answered there; the next
// morning yesterday's panes go to the closed list; and an empty tab 1
// invites you to ask the chief of staff for something. On the
// Overview's fixture (./overview-fixture.ts), with the platform's layouts
// made up here (./place-layout-fake.ts).
import { expect, test, type Page } from "@playwright/test";
import { overview, P, staffRoomOn } from "./overview-fixture";
import { fakeLayouts, type FakeLayouts } from "./place-layout-fake";

test.use({ permissions: ["local-network-access"], viewport: { width: 1440, height: 900 } });

const PLACE = `overview:${P}`;
const doc = (n: number) => ({ kind: "doc" as const, ref: `pdc_${n}`, title: `Doc ${n}` });
const STANDUP = { kind: "doc" as const, ref: "pdc_standup", title: "Needs you on boards" };

/** The Overview, its layout served by a fake platform; `before` runs ahead of the page (a record already arranged). */
async function open(page: Page, before?: (l: FakeLayouts) => void): Promise<FakeLayouts> {
  await page.route(`**/api/projects/${P}/docs/*`, (r) => {
    const id = new URL(r.request().url()).pathname.split("/").pop()!;
    const d = { id, project: P, title: id, author: "acct_a", agent: null, sections: 1, written: 1, rev: 1, created_at: 1, updated_at: 1 };
    return r.fulfill({ json: { doc: d, sections: [{ id: "s1", heading: "Flat by default", intent: "", body: "A third of the width for Needs you.", rev: 1 }], assets: [], writer: true, deleter: true, link: null } });
  });
  const l = await fakeLayouts(page, PLACE);
  before?.(l);
  await overview(page);
  await expect(page.locator('[data-ov-default="boards"]')).toBeVisible();
  return l;
}

const tabs = (page: Page) => page.locator("[data-ov-tabs] [role=tab]");
const pane = (page: Page, title: string) => page.locator(`[data-layout-pane]`, { has: page.locator(".k-pane-title", { hasText: title }) });
const side = async (page: Page) => (await page.locator("[data-ov-side]").boundingBox())!.width;
/** Long enough since the last key or click that the person isn't busy. */
const idle = (page: Page) => page.waitForTimeout(4_300);
const forYou = (id: string, title: string, tab: string) => ({ id, kind: "for_you", group: "decide", counts: true, title, lines: [], from: { agent: "claude@dust", machine: "mch_dust" }, subject: { type: "tab", id: tab, url: `/p/${P}`, project: P, place: PLACE, tab }, since: Date.now() });
const say = (page: Page, items: unknown[]) => page.evaluate((items) => (window as unknown as { __inboxSay(i: unknown[]): void }).__inboxSay(items), items);

test("tab 1 is the title line over Boards and Running, quiet: no title rows, the right column as wide as the panel", async ({ page }, info) => {
  await open(page);
  await expect(tabs(page)).toHaveText(["Overview1"]);
  await expect(page.locator("[data-ov-title] h1")).toHaveText("Arugula");
  await expect(page.locator('[data-ov-default="boards"] [data-ov-boards]')).not.toHaveAttribute("data-compact", "");
  await expect(page.locator('[data-ov-default="running"] [data-ov-running]')).toBeVisible();
  await expect(page.locator("[data-ov-default] .k-pane-title")).toHaveCount(0);
  await expect(page.locator("[data-board-changed]")).toHaveCount(0);
  expect(Math.round(await side(page))).toBe(420);
  await page.screenshot({ path: info.outputPath("overview-layout-default.png") });
});

test("the chief of staff opens a Doc beside Boards without taking the focus; Boards compacts, the side narrows, the header says who changed it", async ({ page }, info) => {
  const l = await open(page);
  await idle(page);
  const before = await page.evaluate(() => document.activeElement?.outerHTML.slice(0, 80));
  await l.lead(doc(1));
  const d = pane(page, "Doc 1");
  await expect(d).toBeVisible();
  await expect(d).toHaveAttribute("data-pane-by", "chief of staff");
  await expect(d.locator(".k-pane-meta")).toHaveText("chief of staff");
  await expect(d.locator("[data-pane-dot]")).toHaveCount(0);
  await expect(d).not.toHaveAttribute("data-pane-focused", "");
  expect(await page.evaluate(() => document.activeElement?.outerHTML.slice(0, 80))).toBe(before);
  await expect(page.locator("[data-ov-boards]")).toHaveAttribute("data-compact", "");
  await expect(page.locator("[data-board-changed]")).toContainText("changed by chief of staff");
  await expect(tabs(page)).toHaveCount(1);
  expect(await side(page)).toBeLessThan(420);
  await page.screenshot({ path: info.outputPath("overview-layout-doc.png") });
});

test("busy in the last 4 seconds, the chief of staff's pane arrives quietly with a dot; looking at it takes the dot", async ({ page }) => {
  const l = await open(page);
  await page.locator("[data-ov-title] h1").click();
  await l.lead(doc(2));
  const d = pane(page, "Doc 2");
  await expect(d.locator("[data-pane-dot]")).toBeVisible();
  await d.click();
  await expect(d.locator("[data-pane-dot]")).toHaveCount(0);
});

test("a review moves to a tab of its own, never current, with a Needs-you row that opens it; tab 1 goes back to its default", async ({ page }, info) => {
  const l = await open(page);
  const r = await l.lead(STANDUP, "for_you");
  await say(page, [forYou("fy_review", "Review: Needs you on boards", r.tab)]);
  await expect(tabs(page)).toHaveCount(2);
  await expect(tabs(page).nth(0)).toHaveAttribute("aria-selected", "true");
  await expect(tabs(page).nth(1).locator("[data-tab-dot]")).toBeVisible();
  await expect(page.locator("[data-ov-boards]")).not.toHaveAttribute("data-compact", "");
  const row = page.locator('[data-ov-needs] [data-need-item="fy_review"]');
  await expect(row).toContainText("Review: Needs you on boards");
  await expect(row).toContainText("tab 2");
  await page.screenshot({ path: info.outputPath("overview-layout-review-tab.png") });
  await row.locator(".k-row-line").click();
  await row.getByRole("button", { name: /Open/ }).click();
  await expect(tabs(page).nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(pane(page, "Needs you on boards")).toBeVisible();
  await expect(page.locator("[data-ov-title]")).toHaveCount(0);
});

test("a fourth pane closes the oldest the chief of staff opened and you haven't touched", async ({ page }) => {
  const l = await open(page);
  for (const n of [1, 2, 3]) await l.lead(doc(n));
  await expect(page.locator("[data-layout-pane]")).toHaveCount(3);
  await l.lead(doc(4));
  await expect(pane(page, "Doc 4")).toBeVisible();
  await expect(pane(page, "Doc 1")).toHaveCount(0);
  await expect(page.locator("[data-layout-pane]")).toHaveCount(3);
});

test("a failing run stands in place of Boards for a while; ↺ Boards puts Boards back", async ({ page }, info) => {
  const l = await open(page);
  await l.lead({ kind: "run", ref: "run_red", title: "Run · main red" }, "beside", "boards");
  const run = pane(page, "Run · main red");
  await expect(run.locator(".k-pane-meta")).toHaveText("in place of Boards · chief of staff");
  await expect(page.locator('[data-ov-default="boards"]')).toHaveCount(0);
  await expect(page.locator('[data-ov-default="running"]')).toBeVisible();
  await page.screenshot({ path: info.outputPath("overview-layout-in-place-of-boards.png") });
  await run.hover();
  await run.locator('[data-pane-restore="boards"]').click();
  await expect(page.locator('[data-ov-default="boards"]')).toBeVisible();
  await expect(run).toHaveCount(0);
});

test("a closed default pane stays closed, and the layout follows a reload; Reset puts tab 1 back and lists what was opened to reopen", async ({ page }) => {
  const l = await open(page);
  await l.lead(doc(1));
  await expect(pane(page, "Doc 1")).toBeVisible();
  const running = page.locator('[data-ov-default="running"]');
  await running.hover();
  await running.locator("[data-pane-close]").click();
  await expect(running).toHaveCount(0);
  await page.reload();
  await expect(page.locator('[data-ov-default="boards"]')).toBeVisible();
  await expect(pane(page, "Doc 1")).toBeVisible();
  await expect(page.locator('[data-ov-default="running"]')).toHaveCount(0);
  await page.locator("[data-board-reset]").click();
  await expect(page.locator('[data-ov-default="running"]')).toBeVisible();
  await expect(pane(page, "Doc 1")).toHaveCount(0);
  await expect(page.locator("[data-board-changed]")).toHaveCount(0);
  await page.locator("[data-ov-more]").click();
  await page.locator("[data-board-reopen]").first().click();
  await expect(pane(page, "Doc 1")).toBeVisible();
});

test("a blocked agent's prompt waits in a tab of its own while you're away, and you open it from Needs you; answered, the tab goes", async ({ page }, info) => {
  const l = await open(page);
  const r = await l.lead({ kind: "prompt", ref: "%7", title: "Allow git clone", machine: "dust" }, "for_you");
  await say(page, [forYou("fy_prompt", "Allow git clone", r.tab)]);
  await expect(tabs(page)).toHaveCount(2);
  await expect(tabs(page).nth(0)).toHaveAttribute("aria-selected", "true");
  const row = page.locator('[data-ov-needs] [data-need-item="fy_prompt"]');
  await expect(row).toContainText("tab 2");
  await page.keyboard.press("2");
  await expect(tabs(page).nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-pane-kind="prompt"]')).toBeVisible();
  await page.screenshot({ path: info.outputPath("overview-layout-prompt.png") });
  // Answered on its machine: the platform closes the pane and its tab, and the row goes.
  l.layout.tabs = l.layout.tabs.filter((t) => t.id !== r.tab);
  l.layout.rev++;
  await l.say();
  await say(page, []);
  await expect(tabs(page)).toHaveCount(1);
  await expect(tabs(page).first()).toHaveAttribute("aria-selected", "true");
  await expect(row).toHaveCount(0);
});

test("the next morning, what the chief of staff opened before today and you haven't touched goes to the closed list; a prompt's tab and today's pane stay; the strip offers them back", async ({ page }, info) => {
  const yesterday = new Date(new Date().setHours(0, 0, 0, 0)).getTime() - 3 * 3_600_000;
  const cos = { account: "acct_a", machine: "mch_dust" };
  const p = (id: string, kind: "doc" | "pr" | "prompt", title: string, at: number) => ({ id, kind, ref: kind === "prompt" ? "%7" : id, title, machine: kind === "prompt" ? "dust" : null, state: null, opened_by: cos, opened_at: at, touched: false, in_place_of: null, size: null, offline: false });
  const tab = (id: string, pane: ReturnType<typeof p>, forYou: boolean) => ({ id, title: pane.title, opened_by: cos, opened_at: yesterday, for_you: forYou, panes: [pane] });
  const l = await open(page, (l) => {
    l.layout.tabs[0]!.panes = [p("pan_old", "doc", "Yesterday's thread", yesterday), p("pan_standup", "doc", "Standup", Date.now())];
    l.layout.tabs.push(tab("tab_review", p("pan_review", "doc", "Review: Needs you on boards", yesterday), false), tab("tab_prompt", p("pan_prompt", "prompt", "Allow git clone", yesterday), true));
    l.layout.changed = { by: cos, at: yesterday };
    l.layout.rev = 3;
  });
  await expect.poll(() => l.posted).toEqual([
    { op: "close", pane: "pan_old" },
    { op: "close_tab", tab: "tab_review" },
  ]);
  await expect(tabs(page)).toHaveCount(2);
  await expect(pane(page, "Standup")).toBeVisible();
  await expect(pane(page, "Yesterday's thread")).toHaveCount(0);
  const overnight = page.locator("[data-ov-overnight]");
  await expect(overnight).toHaveText("↺ 2 closed overnight");
  await page.screenshot({ path: info.outputPath("overview-layout-next-morning.png") });
  // Once a day: a reload tidies nothing more.
  await page.reload();
  await expect(pane(page, "Standup")).toBeVisible();
  expect(l.posted).toHaveLength(2);
  await expect(page.locator("[data-ov-overnight]")).toHaveCount(0);
});

test("while tab 1 has nothing beside Boards and Running, an invitation's asks fill the chief of staff's composer; it goes once something opens", async ({ page }, info) => {
  await page.route(`**/api/projects/${P}/docs/*`, (r) => r.fulfill({ status: 404, json: { error: "not found" } }));
  const l = await fakeLayouts(page, PLACE);
  await staffRoomOn(page, { machine: "m_suite", machine_name: "suite", session: "Chief of staff · Arugula" });
  const invite = page.locator("[data-ov-invite]");
  await expect(invite).toBeVisible();
  await expect(invite.locator("[data-ov-ask]")).toHaveCount(4);
  await page.screenshot({ path: info.outputPath("overview-layout-invite.png") });
  await invite.locator('[data-ov-ask="Show me what merged today"]').click();
  const composer = page.locator("[data-kit=panel] textarea");
  await expect(composer).toHaveValue("Show me what merged today");
  await expect(composer).toBeFocused();
  expect(l.posted).toEqual([]);
  await l.lead(doc(1));
  await expect(pane(page, "Doc 1")).toBeVisible();
  await expect(invite).toHaveCount(0);
});
