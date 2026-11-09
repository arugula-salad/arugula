// A board's Steps view (Arugula #763, #786): the sequence chart. Alice owns team Acme, with a board of
// two phases: "Phase A" holds Design (link: before Build, relates, duplicate,
// discovered), nine fillers beside it in step 1 (so step 1 folds after 8), and
// Build (step 2, which blocks Ship); "Phase B" holds Ship (step 3) and the
// rows Design is linked to. Hovering a row draws its links, each kind its own
// way, with its ends on the bars' boxes through an unfold and a resize; an
// agent on a row shows live; a row is dragged inside its step; and a second
// client that read the group before someone else moved it is told. At 390 px
// Steps is the default, a tap draws the arrows, and the drawer's ↑ ↓ move a
// row. A local control (team-fixture.ts).

import { mkdirSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("steps");

test.use({ viewport: { width: 1280, height: 900 } });
test.describe.configure({ mode: "serial" });

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = "test-results/786-shots";

async function shot(p: Page, name: string) {
  mkdirSync(SHOTS, { recursive: true });
  const path = `${SHOTS}/${name}.png`;
  await p.screenshot({ path, animations: "disabled", fullPage: true });
  console.log(`screenshot: ${path}`);
}

async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
    },
    [method, path, body] as const,
  );
}

let app = "";
let project = "";
let board = "";
let phaseA = "";
let alice: Page;
/** Row ids by title. */
const rowOf: Record<string, string> = {};
const LONG = "Rework the whole importer so a row with a very long title still leaves room for its agent";
const FILLERS = Array.from({ length: 9 }, (_, i) => `Filler ${i + 1}`);

/** The block an agent on Design reports; the board is read with it in place of a daemon's report (a spec can't sign as a machine). */
let agent: { state: string } = { state: "working" };

/** A row's line. */
const bar = (p: Page, title: string) => p.locator(`[data-step-row="${rowOf[title]}"]`);

/** The ids of the lines of a phase's step, in the order drawn. */
const column = (p: Page, phase: string, step: number) =>
  p.locator(`[data-steps-phase="${phase}"] [data-step-row][data-step="${step}"]`).evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.stepRow!));

const grip = (p: Page) => p.locator("[data-steps-grip]");
const gripWidth = async (p: Page) => Number(await grip(p).getAttribute("aria-valuenow"));

/** The width of each column of the header (✓, then the steps), and the title column's. */
const columns = (p: Page) => p.locator("[data-steps-col]").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().width));

/** Drag the handle by `dx` pixels with the mouse. */
async function pull(p: Page, dx: number) {
  const b = (await grip(p).boundingBox())!;
  const [x, y] = [b.x + b.width / 2, b.y + 40];
  await p.mouse.move(x, y);
  await p.mouse.down();
  await p.mouse.move(x + dx / 2, y, { steps: 5 });
  await p.mouse.move(x + dx, y, { steps: 5 });
  await p.mouse.up();
}

/** Each drawn arrow's ends against the boxes of the bars it joins, in the chart's own coordinates. */
async function ends(p: Page) {
  return p.evaluate(() => {
    const chart = document.querySelector("[data-steps]")!;
    const base = chart.getBoundingClientRect();
    const box = (id: string) => {
      const r = chart.querySelector(`[data-step-bar="${id}"]`)!.getBoundingClientRect();
      return { left: r.left - base.left, right: r.right - base.left, cy: (r.top + r.bottom) / 2 - base.top };
    };
    return [...chart.querySelectorAll<SVGPathElement>("path[data-link-kind]")].map((el) => {
      const g = el.parentElement as unknown as SVGGElement;
      const n = el.getAttribute("d")!.match(/-?\d+(?:\.\d+)?/g)!.map(Number);
      const a = box(g.dataset.linkFrom!);
      const b = box(g.dataset.linkTo!);
      // Out of the right edge into the left of a bar further right, the other way round for one further left, else round the right.
      const [x1, x2] = b.left >= a.right ? [a.right, b.left] : b.right <= a.left ? [a.left, b.right] : [a.right, b.right];
      return { kind: el.dataset.linkKind!, from: g.dataset.linkFrom!, to: g.dataset.linkTo!, got: [n[0], n[1], n.at(-2)!, n.at(-1)!], want: [x1, a.cy, x2, b.cy] };
    });
  });
}

/** Every drawn arrow ends within a pixel and a half of the bars it joins. */
async function onTheBars(p: Page, kinds: string[]) {
  await expect
    .poll(async () => {
      const found = await ends(p);
      const off = found.filter((f) => f.got.some((v, i) => Math.abs(v - f.want[i]) > 1.5));
      return { kinds: found.map((f) => f.kind).sort(), off };
    })
    .toEqual({ kinds: [...kinds].sort(), off: [] });
}

test("a board's Steps view draws a row's links by kind, shows agents live and is reordered by dragging; a stale reorder is told", async ({ browser }) => {
  test.setTimeout(300_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.emulateMedia({ colorScheme: "dark" });
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await alice.goto(`${app}/`);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  board = (await api(alice, "POST", `/api/projects/${project}/boards`, { title: "Launch" })).body!.board.id as string;
  phaseA = (await api(alice, "PUT", `/api/boards/${board}/entries`, { entry: "zone", title: "Phase A" })).body!.groups[0].id as string;
  const phaseB = (await api(alice, "PUT", `/api/boards/${board}/entries`, { entry: "zone", title: "Phase B" })).body!.groups[1].id as string;
  const phaseC = (await api(alice, "PUT", `/api/boards/${board}/entries`, { entry: "zone", title: "Phase C" })).body!.groups[2].id as string;
  const titles: [string, string][] = [
    ["Design", phaseA],
    ...FILLERS.map((f): [string, string] => [f, phaseA]),
    ["Build", phaseA],
    ["Ship", phaseB],
    ["Docs", phaseB],
    ["Old duplicate", phaseB],
    ["Found in testing", phaseB],
    [LONG, phaseB],
    ["Already shipped", phaseC],
  ];
  const issue: Record<string, string> = {};
  for (const [title, group] of titles) {
    const i = await api(alice, "POST", `/api/projects/${project}/issues`, { title });
    issue[title] = i.body!.id;
    const v = await api(alice, "PUT", `/api/boards/${board}/entries`, { entry: "row", pointer: { kind: "issue", id: i.body!.id } });
    expect(v.status).toBe(200);
    const row = v.body!.zones.flatMap((z: any) => z.rows).find((r: any) => r.pointer.id === i.body!.id).id as string;
    rowOf[title] = row;
    expect((await api(alice, "POST", `/api/boards/${board}/entries/${row}/move`, { zone: group })).status).toBe(200);
  }
  const link = async (from: string, to: string, kind: string) => expect((await api(alice, "POST", `/api/projects/${project}/relations`, { from: issue[from], to: issue[to], kind })).status).toBe(200);
  await link("Design", "Build", "before");
  await link("Build", "Ship", "blocks");
  await link("Design", "Docs", "relates");
  await link("Design", "Old duplicate", "duplicate_of");
  await link("Design", "Found in testing", "discovered_from");

  // The board is read with an agent on Design; what it says is `agent`'s.
  await alice.route(/\/api\/boards\/brd_[0-9a-f]+$/, async (route) => {
    const r = await route.fetch();
    const j = await r.json();
    const design = j.zones.flatMap((z: any) => z.rows).find((x: any) => x.id === rowOf["Design"]);
    if (design) design.marks = [{ kind: "stalled", since: Date.now() - 3 * 3_600_000 }, { kind: "blocked_by", number: 5, title: "Pick a name" }];
    if (design) design.attachments = [{ id: "att_0000000000000000000000000000aa01", role: "work", pointer: { kind: "block", machine: "mch_0123", pane: "640" }, block: { state: agent.state, title: "API lane", branch: "feat/api", head: "abc1234", pushed: false, uncommitted: 1 } }];
    // A long title with an agent whose own title is long too; and a phase whose only row is done.
    const long = j.zones.flatMap((z: any) => z.rows).find((x: any) => x.id === rowOf[LONG]);
    if (long) long.attachments = [{ id: "att_0000000000000000000000000000aa02", role: "work", pointer: { kind: "block", machine: "mch_0123", pane: "641" }, block: { state: "idle", title: "Importer lane for the long one", branch: "feat/import", head: "def5678", pushed: false, uncommitted: 0 } }];
    const shipped = j.zones.flatMap((z: any) => z.rows).find((x: any) => x.id === rowOf["Already shipped"]);
    if (shipped) shipped.lane = "done";
    await route.fulfill({ response: r, json: j });
  });

  await alice.goto(`${app}/p/${project}/boards/${board}`);
  // Lanes is the default at this width; the switch is the board's own.
  await expect(alice.locator('[data-board-view="lanes"]')).toHaveAttribute("aria-pressed", "true");
  await alice.locator('[data-board-view="steps"]').click();
  await expect(alice.locator("[data-steps]")).toBeVisible();
  await expect(alice.locator("[data-board-zone]")).toHaveCount(0);

  // Phases and steps: Phase A's step 1 shows eight of ten behind "+2 more"; Build is step 2 and Ship step 3.
  await expect(alice.locator('[data-steps-phase] h2')).toHaveText(["Phase A", "Phase B", "Phase C"]);
  expect(await column(alice, phaseA, 1)).toHaveLength(8);
  // A line carries the glyph, the number and the title, and no agent or marks.
  await expect(bar(alice, "Design").locator("[data-step-agent], [data-board-mark]")).toHaveCount(0);
  await expect(alice.locator(`[data-steps-more="${phaseA}"]`)).toHaveText("+2 more");
  await expect(bar(alice, "Build")).toHaveAttribute("data-step", "2");
  await expect(bar(alice, "Ship")).toHaveAttribute("data-step", "3");

  // The agent is live in Design's drawer, with its marks: working, then asking, with no reload.
  await bar(alice, "Design").click();
  const drawer = alice.locator(`[data-board-drawer="${rowOf["Design"]}"]`);
  await expect(drawer.locator('[data-board-block="working"]')).toBeVisible();
  await expect(drawer.locator('[data-board-mark="stalled"]')).toHaveText("stalled 3h");
  await expect(drawer.locator('[data-board-mark="blocked_by"]')).toHaveText("blocked by #5");
  await alice.evaluate(() => ((window as any).__kept = 1));
  agent = { state: "needs_input" };
  const designRev = (await api(alice, "GET", `/api/boards/${board}`)).body!.zones.flatMap((z: any) => z.rows).find((x: any) => x.id === rowOf["Design"]).rev;
  // The platform tells the board's readers when a block's state changes (put_block_state); a spec can't sign as a machine, so a write on the row stands in for that tell.
  expect((await api(alice, "PUT", `/api/boards/${board}/entries`, { id: rowOf["Design"], rev: designRev, note: "asking" })).status).toBe(200);
  await expect(drawer.locator('[data-board-block="needs_input"]')).toBeVisible();
  expect(await alice.evaluate(() => (window as any).__kept)).toBe(1);
  await alice.locator("[data-board-drawer-close]").click();
  await alice.mouse.move(2, 2);

  // No arrows until a row is hovered; then its links, one look to a kind, each end on a bar.
  await expect(alice.locator("path[data-link-kind]")).toHaveCount(0);
  await bar(alice, "Design").hover();
  await onTheBars(alice, ["before", "relates", "duplicate_of", "discovered_from"]);
  const dash = (kind: string) => alice.locator(`path[data-link-kind="${kind}"]`).evaluate((e) => getComputedStyle(e).strokeDasharray + "|" + getComputedStyle(e).strokeWidth + "|" + e.getAttribute("marker-end"));
  const looks = await Promise.all(["before", "relates", "duplicate_of", "discovered_from"].map(dash));
  expect(new Set(looks).size).toBe(4);
  expect(looks[0]).toMatch(/^none\|.*\|url\(#/); // before: solid, with a head
  expect(looks[1]).not.toMatch(/^none/); // relates: dotted
  expect(looks[1]).toMatch(/\|null$/); // and no head
  expect(looks[3]).not.toMatch(/^none/); // discovered_from: dashed, with a head
  await expect(alice.locator('path[data-link-kind="duplicate_of"] + .step-link-gap')).toHaveCount(1);
  await shot(alice, "1280-links");
  await bar(alice, "Build").hover();
  await onTheBars(alice, ["before", "blocks"]);
  expect(Number(await alice.locator('path[data-link-kind="blocks"]').evaluate((e) => getComputedStyle(e).strokeWidth.replace("px", "")))).toBeGreaterThan(2);

  // "+2 more" unfolds step 1, and Phase B moves down with it: Build's arrow to Ship follows. A resize does the same.
  const before = await bar(alice, "Ship").boundingBox();
  await alice.locator(`[data-steps-more="${phaseA}"]`).click();
  await expect(alice.locator(`[data-steps-more="${phaseA}"]`)).toHaveCount(0);
  await bar(alice, "Build").hover();
  expect((await bar(alice, "Ship").boundingBox())!.y).toBeGreaterThan(before!.y + 20);
  await onTheBars(alice, ["before", "blocks"]);
  // The pointer leaves the bar when the layout moves, so each new layout is hovered again and its arrows are held to its bars.
  await alice.setViewportSize({ width: 1000, height: 900 });
  await alice.mouse.move(5, 5);
  await bar(alice, "Build").hover();
  await onTheBars(alice, ["before", "blocks"]);
  await alice.setViewportSize({ width: 1280, height: 900 });
  await alice.mouse.move(5, 5);
  await bar(alice, "Build").hover();
  await onTheBars(alice, ["before", "blocks"]);
  await alice.mouse.move(5, 5);
  await expect(alice.locator("path[data-link-kind]")).toHaveCount(0);

  // The view is remembered for this viewer.
  await alice.reload();
  await expect(alice.locator("[data-steps]")).toBeVisible();

  // A row is dragged inside its step: Filler 2 goes before Design, and stays there.
  const startOrder = await column(alice, phaseA, 1);
  expect(startOrder[0]).toBe(rowOf["Design"]);
  await bar(alice, "Filler 2").dragTo(bar(alice, "Design"), { targetPosition: { x: 40, y: 3 } });
  await expect.poll(() => column(alice, phaseA, 1).then((c) => c[0])).toBe(rowOf["Filler 2"]);
  await alice.reload();
  await expect.poll(() => column(alice, phaseA, 1).then((c) => c[0])).toBe(rowOf["Filler 2"]);
  const server = async () => ((await api(alice, "GET", `/api/boards/${board}`)).body!.order as string[]).filter((id) => Object.values(rowOf).includes(id)).slice(0, 3);
  expect((await server())[0]).toBe(rowOf["Filler 2"]);

  // Two clients: the second read the group before the first moved a row, and is told. Its reorder is not made and not tried again.
  const stale = (await api(alice, "GET", `/api/boards/${board}`)).body!;
  const grouped = new Set(stale.zones.flatMap((z: any) => z.rows).filter((r: any) => r.group === phaseA).map((r: any) => r.id));
  const read = (stale.order as string[]).filter((id) => grouped.has(id));
  const second = await alice.context().newPage();
  await second.goto(`${app}/p/${project}/boards/${board}`);
  await expect(second.locator("[data-steps]")).toBeVisible();
  // The first client moves Filler 3 to the top, as another client would.
  const moved = await api(alice, "POST", `/api/boards/${board}/entries/${rowOf["Filler 3"]}/move`, { before: rowOf["Filler 2"], seen: read });
  expect(moved.status).toBe(200);
  // The second still has the old order (its reads are held), moves Filler 4 and is refused with the group as it is now.
  await second.route(/\/api\/boards\/brd_[0-9a-f]+$/, (route) => route.fulfill({ json: stale }));
  await second.reload();
  await expect(bar(second, "Filler 4")).toBeVisible();
  const refused = second.waitForResponse((r) => r.url().includes("/move") && r.status() === 409);
  await bar(second, "Filler 4").dragTo(bar(second, "Design"), { targetPosition: { x: 40, y: 3 } });
  const body = await (await refused).json();
  expect(body.reason).toBe("stale_order");
  expect(body.rows[0]).toBe(rowOf["Filler 3"]);
  await expect(bar(second, "Filler 4").locator("[data-step-moved-first]")).toHaveText("moved first by someone else");
  await shot(second, "1280-told");
  expect((await server())[0]).toBe(rowOf["Filler 3"]);
  expect(await server()).not.toContain(rowOf["Filler 4"]);
  await second.close();
});

test("at 390 px Steps is the default, a tap draws the arrows, and the drawer's arrows move a row", async () => {
  test.setTimeout(240_000);
  // Alice's own page at a phone's width, with the view she picked forgotten.
  const phone = alice;
  await phone.setViewportSize({ width: 390, height: 844 });
  await phone.evaluate(() => localStorage.clear());
  await phone.goto(`${app}/p/${project}/boards/${board}`);
  await expect(phone.locator("[data-steps]")).toBeVisible();
  await expect(phone.locator('[data-board-view="steps"]')).toHaveAttribute("aria-pressed", "true");
  await expect(phone.locator("[data-board-zone]")).toHaveCount(0);
  // One chart: ✓ and three steps, with the title column at the phone's default; a phase's done rows are counted, not drawn.
  await expect(phone.locator("[data-steps-col]")).toHaveCount(4);
  expect(await gripWidth(phone)).toBe(150);
  await expect(phone.locator('[data-step-row][data-lane="done"]')).toHaveCount(0);
  await expect(phone.locator("[data-steps-done]")).toHaveText("1 done");
  expect(await phone.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);

  // A tap draws the row's arrows; another tap opens its drawer.
  await phone.mouse.move(2, 2);
  await expect(phone.locator("path[data-link-kind]")).toHaveCount(0);
  await bar(phone, "Design").click();
  await onTheBars(phone, ["before", "relates", "duplicate_of", "discovered_from"]);
  await shot(phone, "390-links");
  // The held row stays held while the title column is widened by keys, and its arrows follow the bars (no box the chart watches changes size).
  await grip(phone).focus();
  await phone.mouse.move(2, 2);
  for (let i = 0; i < 2; i++) await phone.keyboard.press("ArrowRight");
  expect(await gripWidth(phone)).toBe(182);
  await onTheBars(phone, ["before", "relates", "duplicate_of", "discovered_from"]);
  await shot(phone, "390-links-widened");
  // A double-click puts it back, and the arrows follow again.
  await grip(phone).dblclick();
  expect(await gripWidth(phone)).toBe(150);
  expect(await phone.evaluate(() => localStorage.getItem("arugula.board.steps.title"))).toBeNull();
  await onTheBars(phone, ["before", "relates", "duplicate_of", "discovered_from"]);
  await bar(phone, "Design").click();
  await expect(phone.locator(`[data-board-drawer="${rowOf["Design"]}"]`)).toBeVisible();
  // The drawer lists its links by kind, in words.
  await expect(phone.locator("[data-board-relation]")).toHaveCount(4);
  await phone.locator("[data-board-drawer-close]").click();

  // ↑ ↓ in the drawer: the first filler in step 1 goes down one place.
  const order = await column(phone, phaseA, 1);
  await phone.locator(`[data-step-row="${order[0]}"]`).click();
  await phone.locator(`[data-step-row="${order[0]}"]`).click();
  await expect(phone.locator('[data-board-step-move="up"]')).toBeDisabled();
  await shot(phone, "390-drawer");
  await phone.locator('[data-board-step-move="down"]').click();
  await expect.poll(() => column(phone, phaseA, 1).then((c) => c.slice(0, 2))).toEqual([order[1], order[0]]);
  await phone.locator("[data-board-drawer-close]").click();

  // Lanes is one tap away, and the pick is kept.
  await phone.locator('[data-board-view="lanes"]').click();
  await phone.reload();
  await expect(phone.locator("[data-board-zone]").first()).toBeVisible();
});

/** The right edge of a line, and of the furthest thing inside it (its title cell's parts, its bar): none crosses it. */
async function insideLine(p: Page, title: string) {
  return bar(p, title).evaluate((line) => ({
    line: line.getBoundingClientRect().right,
    inside: Math.max(...[...line.querySelectorAll("*")].map((e) => e.getBoundingClientRect().right)),
  }));
}

test("a line carries only its glyph, number and title, and nothing crosses its right edge; a phase with nothing open has only done lines", async () => {
  test.setTimeout(240_000);
  const page = alice;
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`${app}/p/${project}/boards/${board}`);
    await page.locator('[data-board-view="steps"]').click();
    await expect(bar(page, LONG)).toBeVisible();
    for (const title of [LONG, "Design", "Build"]) {
      const { line, inside } = await insideLine(page, title);
      expect(inside, `${width}px, ${title}: ${inside} against the line's ${line}`).toBeLessThanOrEqual(line + 0.5);
    }
    // The long title gave way with an ellipsis, and its line has no agent chip.
    expect(await bar(page, LONG).locator("[data-step-title]").evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(true);
    await expect(page.locator("[data-step-agent]")).toHaveCount(0);
    await shot(page, `${width}-lines`);
  }

  // The agent on the long row is in its drawer.
  await page.setViewportSize({ width: 1280, height: 900 });
  await bar(page, LONG).click();
  const drawer = page.locator(`[data-board-drawer="${rowOf[LONG]}"]`);
  await expect(drawer.locator("[data-board-drawer-attachments] [data-board-block]")).toBeVisible();
  await page.locator("[data-board-drawer-close]").click();

  // Phase C has one row, done: its head says so. On a phone no line is drawn under it; on a desktop its line has the bar in ✓.
  await page.setViewportSize({ width: 390, height: 900 });
  const c = page.locator("[data-steps-phase]", { has: page.locator("h2", { hasText: "Phase C" }) });
  await expect(c.locator("[data-steps-done]")).toHaveText("1 done");
  await expect(c.locator("[data-step-row]")).toHaveCount(0);
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(c.locator("[data-step-row]")).toHaveCount(1);
  await expect(c.locator("[data-step-bar]")).toHaveAttribute("data-bar", "done");
});

test("at 1280 px each open row's bar is in its step's column and a done row's is in ✓", async () => {
  test.setTimeout(240_000);
  const page = alice;
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(`${app}/p/${project}/boards/${board}`);
  await expect(page.locator("[data-steps]")).toBeVisible();
  await page.locator(`[data-steps-more="${phaseA}"]`).click();
  const found = await page.evaluate(() => {
    const cols = [...document.querySelectorAll<HTMLElement>("[data-steps-col]")].map((e) => e.getBoundingClientRect());
    return [...document.querySelectorAll<HTMLElement>("[data-step-row]")].map((line) => {
      const bar = line.querySelector("[data-step-bar]")!.getBoundingClientRect();
      const col = cols[Number(line.dataset.step)];
      return { id: line.dataset.stepRow!, lane: line.dataset.lane!, step: Number(line.dataset.step), left: bar.left - col.left, right: col.right - bar.right, top: bar.top, bottom: bar.bottom, line: line.getBoundingClientRect() };
    });
  });
  // Every bar fills its column, inset 3px.
  expect(found.length).toBeGreaterThan(12);
  for (const f of found) {
    expect(f.left, `${f.id}: left inset`).toBeCloseTo(3, 0);
    expect(f.right, `${f.id}: right inset`).toBeCloseTo(3, 0);
    expect(f.top).toBeGreaterThanOrEqual(f.line.top);
    expect(f.bottom).toBeLessThanOrEqual(f.line.bottom);
  }
  const by = (title: string) => found.find((f) => f.id === rowOf[title])!;
  expect([by("Design").step, by("Build").step, by("Ship").step]).toEqual([1, 2, 3]);
  // The done row is in ✓ (column 0), and only a done row is.
  expect(by("Already shipped")).toMatchObject({ lane: "done", step: 0 });
  expect(found.filter((f) => f.step === 0).map((f) => f.lane)).toEqual(["done"]);
  expect(found.filter((f) => f.lane !== "done").every((f) => f.step >= 1)).toBe(true);
  await expect(page.locator('[data-step-bar][data-bar="done"]')).toHaveCount(1);
  // The longest open chain is ringed.
  const chain = ((await api(page, "GET", `/api/boards/${board}`)).body!.chain ?? []) as string[];
  for (const id of chain) await expect(page.locator(`[data-step-bar="${id}"]`)).toHaveAttribute("data-crit", "");
  await shot(page, "1280-chart");
});

test("dragging the title column's handle by 200 px narrows every step column equally and the width is kept; keys, the clamp and a double-click", async () => {
  test.setTimeout(240_000);
  const page = alice;
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(`${app}/p/${project}/boards/${board}`);
  await expect(page.locator("[data-steps]")).toBeVisible();
  await page.evaluate(() => localStorage.removeItem("arugula.board.steps.title"));
  await page.reload();
  await expect(grip(page)).toHaveAttribute("role", "separator");
  await expect(grip(page)).toHaveAttribute("aria-orientation", "vertical");
  expect(await gripWidth(page)).toBe(260);

  const before = await columns(page);
  expect(before).toHaveLength(4);
  await pull(page, 200);
  expect(await gripWidth(page)).toBe(460);
  const after = await columns(page);
  // The four columns share the rest evenly: each is a quarter of the 200 px narrower, and they stay equal.
  for (let i = 0; i < after.length; i++) expect(Math.abs(before[i] - after[i] - 200 / after.length), `column ${i}`).toBeLessThanOrEqual(1);
  expect(Math.max(...after) - Math.min(...after)).toBeLessThanOrEqual(1);
  expect(await page.evaluate(() => localStorage.getItem("arugula.board.steps.title"))).toBe("460");
  // The arrows follow the bars after a resize, with a row hovered.
  await bar(page, "Build").hover();
  await onTheBars(page, ["before", "blocks"]);
  await shot(page, "1280-resized-links");
  await page.mouse.move(2, 2);

  // It is still there after a reload.
  await page.reload();
  await expect(grip(page)).toBeVisible();
  expect(await gripWidth(page)).toBe(460);
  expect(Math.round((await page.locator(".steps-lab").first().boundingBox())!.width)).toBe(460);

  // The most it takes is 60% of the chart, the least 160 px.
  const chartWidth = await page.locator("[data-steps-chart]").evaluate((e) => e.clientWidth);
  await pull(page, 2000);
  expect(await gripWidth(page)).toBe(Math.floor(chartWidth * 0.6));
  expect(Number(await grip(page).getAttribute("aria-valuemax"))).toBe(Math.floor(chartWidth * 0.6));
  await pull(page, -3000);
  expect(await gripWidth(page)).toBe(160);
  expect(Number(await grip(page).getAttribute("aria-valuemin"))).toBe(160);
  expect(await page.evaluate(() => localStorage.getItem("arugula.board.steps.title"))).toBe("160");

  // A double-click puts it back, and the browser forgets the width.
  await grip(page).dblclick();
  expect(await gripWidth(page)).toBe(260);
  expect(await page.evaluate(() => localStorage.getItem("arugula.board.steps.title"))).toBeNull();
});

test("the handle is moved by the arrow keys, 16 px a press", async () => {
  test.setTimeout(120_000);
  const page = alice;
  await page.evaluate(() => localStorage.removeItem("arugula.board.steps.title"));
  await page.reload();
  await expect(grip(page)).toBeVisible();
  await grip(page).focus();
  expect(await gripWidth(page)).toBe(260);
  for (let i = 0; i < 3; i++) await page.keyboard.press("ArrowRight");
  expect(await gripWidth(page)).toBe(308);
  await page.keyboard.press("ArrowLeft");
  expect(await gripWidth(page)).toBe(292);
  expect(await page.evaluate(() => localStorage.getItem("arugula.board.steps.title"))).toBe("292");
  await page.evaluate(() => localStorage.removeItem("arugula.board.steps.title"));
});

test("at 390 px the page doesn't scroll sideways, with the title column at its widest too", async () => {
  test.setTimeout(120_000);
  const page = alice;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`${app}/p/${project}/boards/${board}`);
  await expect(page.locator("[data-steps]")).toBeVisible();
  const fits = () => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
  expect(await fits()).toBe(true);
  await grip(page).focus();
  for (let i = 0; i < 30; i++) await page.keyboard.press("ArrowRight");
  expect(await gripWidth(page)).toBe(Number(await grip(page).getAttribute("aria-valuemax")));
  expect(await fits()).toBe(true);
  for (let i = 0; i < 30; i++) await page.keyboard.press("ArrowLeft");
  expect(await gripWidth(page)).toBe(120);
  expect(await fits()).toBe(true);
  await shot(page, "390-narrowest");
  await grip(page).dblclick();
});
