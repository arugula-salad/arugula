// A board's Steps view (Arugula #763). Alice owns team Acme, with a board of
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

const SHOTS = `${process.env.TMPDIR ?? "/tmp"}/763-shots`;

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
const FILLERS = Array.from({ length: 9 }, (_, i) => `Filler ${i + 1}`);

/** The block an agent on Design reports; the board is read with it in place of a daemon's report (a spec can't sign as a machine). */
let agent: { state: string } = { state: "working" };

const bar = (p: Page, title: string) => p.locator(`[data-step-row="${rowOf[title]}"]`);

/** The ids of step 1 of Phase A in the order drawn. */
const column = (p: Page, phase: string, step: number) =>
  p.locator(`[data-steps-phase="${phase}"] [data-steps-col="${step}"] [data-step-row]`).evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.stepRow!));

/** Each drawn arrow's ends against the boxes of the bars it joins, in the chart's own coordinates. */
async function ends(p: Page) {
  return p.evaluate(() => {
    const chart = document.querySelector("[data-steps]")!;
    const base = chart.getBoundingClientRect();
    const box = (id: string) => {
      const r = chart.querySelector(`[data-step-row="${id}"]`)!.getBoundingClientRect();
      return { left: r.left - base.left + chart.scrollLeft, right: r.right - base.left + chart.scrollLeft, cy: (r.top + r.bottom) / 2 - base.top + chart.scrollTop };
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
  const titles: [string, string][] = [
    ["Design", phaseA],
    ...FILLERS.map((f): [string, string] => [f, phaseA]),
    ["Build", phaseA],
    ["Ship", phaseB],
    ["Docs", phaseB],
    ["Old duplicate", phaseB],
    ["Found in testing", phaseB],
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
    if (design) design.attachments = [{ id: "att_0000000000000000000000000000aa01", role: "work", pointer: { kind: "block", machine: "mch_0123", pane: "640" }, block: { state: agent.state, title: "API lane", branch: "feat/api", head: "abc1234", pushed: false, uncommitted: 1 } }];
    await route.fulfill({ response: r, json: j });
  });

  await alice.goto(`${app}/p/${project}/boards/${board}`);
  // Lanes is the default at this width; the switch is the board's own.
  await expect(alice.locator('[data-board-view="lanes"]')).toHaveAttribute("aria-pressed", "true");
  await alice.locator('[data-board-view="steps"]').click();
  await expect(alice.locator("[data-steps]")).toBeVisible();
  await expect(alice.locator("[data-board-zone]")).toHaveCount(0);

  // Phases and steps: Phase A's step 1 shows eight of ten behind "+2 more"; Build is step 2 and Ship step 3.
  await expect(alice.locator('[data-steps-phase] h2')).toHaveText(["Phase A", "Phase B"]);
  expect(await column(alice, phaseA, 1)).toHaveLength(8);
  await expect(alice.locator(`[data-steps-more="${phaseA}"]`)).toHaveText("+2 more");
  await expect(bar(alice, "Build")).toHaveAttribute("data-step", "2");
  await expect(bar(alice, "Ship")).toHaveAttribute("data-step", "3");

  // The agent is live on Design's bar: working, then asking, with no reload.
  await expect(bar(alice, "Design").locator('[data-board-block="working"]')).toBeVisible();
  await alice.evaluate(() => ((window as any).__kept = 1));
  agent = { state: "needs_input" };
  const designRev = (await api(alice, "GET", `/api/boards/${board}`)).body!.zones.flatMap((z: any) => z.rows).find((x: any) => x.id === rowOf["Design"]).rev;
  // The platform tells the board's readers when a block's state changes (put_block_state); a spec can't sign as a machine, so a write on the row stands in for that tell.
  expect((await api(alice, "PUT", `/api/boards/${board}/entries`, { id: rowOf["Design"], rev: designRev, note: "asking" })).status).toBe(200);
  await expect(bar(alice, "Design").locator('[data-board-block="needs_input"]')).toBeVisible();
  expect(await alice.evaluate(() => (window as any).__kept)).toBe(1);

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
  // Steps stack as sections: a column per step that has rows.
  await expect(phone.locator(`[data-steps-phase="${phaseA}"] [data-steps-col]`)).toHaveCount(2);
  expect(await phone.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);

  // A tap draws the row's arrows; another tap opens its drawer.
  await phone.mouse.move(2, 2);
  await expect(phone.locator("path[data-link-kind]")).toHaveCount(0);
  await bar(phone, "Design").click();
  await onTheBars(phone, ["before", "relates", "duplicate_of", "discovered_from"]);
  await shot(phone, "390-links");
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
