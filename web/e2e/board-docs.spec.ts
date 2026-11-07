// Boards link Docs (#517), as a person meets them. Alice owns team Acme, with
// chat on, a board Launch with one row, and two
// project Docs. Under the goal is a Docs strip: "+ Doc" opens a picker of the
// project's Docs by title with a filter, a pick is a chip with its written
// sections, and the chip opens the Doc's page, which says it is on the board.
// A machine's Doc (an agent links its own machine's; a person, one on a
// machine they reach) says which machine has it to a reader who reaches it,
// and is on "a machine", a plain chip, to the rest. The row's
// drawer has a Docs section with the same picker; a Doc there moves no lane.
// Bob, a viewer on a phone, sees the chips and none of the buttons. Pictures
// of both at 1280 and 390, dark, go to $TMPDIR/517-shots/ (never committed).

import { mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, live } from "./team-fixture";

const t = new TeamControl("boarddocs");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const shots = join(process.env.TMPDIR ?? tmpdir(), "517-shots");
mkdirSync(shots, { recursive: true });

let alice: Page;
let bob: Page;
let team = "";
let app = "";
let project = "";
let board = "";
let rowId = "";
let machine = "";
let bobs = "";

const PLAN = "The launch plan";
const SPIKE = "Installer spike";
const ROW = "Fix the installer";

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
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

const strip = (p: Page): Locator => p.locator("[data-board-docs]");
const chip = (where: Locator, title: string): Locator => where.locator("[data-board-doc]").filter({ hasText: title });

test("Alice links a project Doc to the board's strip, opens it, and takes it off", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice", { colorScheme: "dark", viewport: { width: 1280, height: 900 } });
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "viewer", true), team);
  bob = await t.person(browser, "bob", { colorScheme: "dark", viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  // Steps is the default at this width (#763); this spec is about the lanes.
  await bob.addInitScript(() => localStorage.setItem("arugula.board.view", "lanes"));
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  // Alice's own machine, whose Doc she links below (Bob doesn't reach it), and Bob's (she doesn't reach that).
  await t.addMachine(alice, "alices");
  await home(alice);
  await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
  machine = await alice.evaluate(() => window.__arugula.fleet.host("alices")!.id!);
  await t.addMachine(bob, "bobs");
  await home(bob);
  await expect.poll(() => live(bob, ["bobs"]), { timeout: 30_000 }).toBe(true);
  bobs = await bob.evaluate(() => window.__arugula.fleet.host("bobs")!.id!);

  await platform(alice);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  // Two Docs (one half written), an issue, and a board with the issue as its row.
  for (const [title, sections] of [
    [PLAN, [{ heading: "Why", markdown: "Because." }, { heading: "How", markdown: "Like this." }, { heading: "When", intent: "dates" }]],
    [SPIKE, [{ heading: "Findings", markdown: "It works." }]],
  ] as const) {
    expect((await api(alice, "POST", `/api/projects/${project}/docs`, { title, sections })).status).toBe(200);
  }
  const issue = (await api(alice, "POST", `/api/projects/${project}/issues`, { title: ROW, body: "", labels: [], assignees: [] })).body!.id as string;
  const made = await api(alice, "POST", `/api/projects/${project}/boards`, { title: "Launch", goal: "People can launch Acme in one click." });
  board = made.body!.board.id as string;
  const put = await api(alice, "PUT", `/api/boards/${board}/entries`, { entry: "row", pointer: { kind: "issue", id: issue } });
  rowId = put.body!.zones[0].rows[0].id as string;

  await platform(alice, `/p/${project}/boards/${board}`);
  await expect(alice.locator("[data-board-goal]")).toBeVisible({ timeout: 15_000 });
  // Under the goal, with a button to link one and no chips yet.
  await expect(strip(alice)).toBeVisible();
  await expect(strip(alice).locator("[data-board-doc]")).toHaveCount(0);
  const goalBox = (await alice.locator("[data-board-goal]").boundingBox())!;
  const stripBox = (await strip(alice).boundingBox())!;
  expect(stripBox.y).toBeGreaterThan(goalBox.y);

  await strip(alice).locator("[data-board-doc-add]").click();
  const picker = alice.locator("[data-board-doc-picker]");
  await expect(picker.locator("[data-board-doc-pick]")).toHaveCount(2);
  await picker.locator("[data-board-doc-filter]").fill("spike");
  await expect(picker.locator("[data-board-doc-pick]")).toHaveCount(1);
  await picker.locator("[data-board-doc-filter]").fill("");
  await picker.locator("[data-board-doc-pick]", { hasText: PLAN }).click();
  await expect(picker).toHaveCount(0);
  // The chip has the title and written/total sections.
  await expect(chip(strip(alice), PLAN)).toContainText("2/3");
  // A machine's Doc is linked by an agent (board_write link_doc), or by a person from a machine they reach; the platform keeps it with its title.
  const linked = await api(alice, "POST", `/api/boards/${board}/docs`, { pointer: { kind: "machine_doc", machine, doc: "00112233445566ff", title: "Perf notes" } });
  expect(linked.status).toBe(200);
  const elsewhere = await api(alice, "POST", `/api/boards/${board}/docs`, { pointer: { kind: "machine_doc", machine: bobs, doc: "00112233445566ff", title: "Not hers" } });
  expect(elsewhere.status).toBe(403);
  await alice.reload();
  await expect(chip(strip(alice), "Perf notes")).toContainText("on alices");
  await expect(strip(alice).locator("[data-board-doc]")).toHaveCount(2);
  await alice.screenshot({ path: join(shots, "strip-1280-dark.png") });

  // The chip opens the Doc's page, which says the Doc is on the board.
  await chip(strip(alice), PLAN).locator("[data-board-doc-open]").click();
  await expect(alice.locator("[data-doc-page]")).toBeVisible({ timeout: 15_000 });
  await expect(alice.locator("[data-doc-boards]")).toContainText("Launch");
  await expect(alice.locator("[data-doc-boards]")).toContainText("Docs strip");
  await alice.locator("[data-doc-board]").click();
  await expect(alice.locator("[data-board-docs]")).toBeVisible({ timeout: 15_000 });

  // The picker leaves out what is linked; the × takes a chip off.
  await strip(alice).locator("[data-board-doc-add]").click();
  await expect(alice.locator("[data-board-doc-pick]")).toHaveCount(1);
  await alice.locator("[data-board-doc-picker-close]").click();
  await chip(strip(alice), "Perf notes").locator("[data-board-doc-remove]").click();
  await expect(chip(strip(alice), "Perf notes")).toHaveCount(0);
  await expect(chip(strip(alice), PLAN)).toHaveCount(1);
});

test("a Doc on a row is in its drawer, moves no lane, and the Doc says which boards it is on", async () => {
  test.setTimeout(90_000);
  await platform(alice, `/p/${project}/boards/${board}`);
  const card = alice.locator("[data-board-row]").first();
  await expect(card).toBeVisible({ timeout: 15_000 });
  const laneOf = () => alice.locator("[data-board-zone]").filter({ has: alice.locator("[data-board-row]") }).getByRole("heading").first().innerText();
  const before = await laneOf();
  await card.locator("[data-board-row-menu]").click();
  await alice.getByRole("menuitem", { name: "Details" }).click();
  const drawer = alice.locator("[data-board-drawer]");
  await expect(drawer).toBeVisible();
  await drawer.locator("[data-board-drawer-doc-add]").click();
  await drawer.locator("[data-board-doc-pick]", { hasText: SPIKE }).click();
  await expect(chip(drawer.locator("[data-board-drawer-docs]"), SPIKE)).toContainText("1/1");
  // Not under Attached, and the row stays where it was.
  await expect(drawer.locator("[data-board-drawer-attachments]")).toHaveCount(0);
  expect(await laneOf()).toBe(before);
  await alice.screenshot({ path: join(shots, "drawer-1280-dark.png") });
  await alice.keyboard.press("Escape");
  // The card carries the Doc as a chip.
  await expect(chip(card, SPIKE)).toBeVisible();

  const spike = (await api(alice, "GET", `/api/projects/${project}/docs`)).body!.docs.find((d: any) => d.title === SPIKE).id as string;
  await platform(alice, `/p/${project}/docs/${spike}`);
  await expect(alice.locator("[data-doc-boards]")).toContainText("Launch");
  await expect(alice.locator("[data-doc-boards]")).toContainText("1 row");

  // Taking it off the row from the drawer.
  await platform(alice, `/p/${project}/boards/${board}`);
  await alice.locator("[data-board-row]").first().locator("[data-board-row-menu]").click();
  await alice.getByRole("menuitem", { name: "Details" }).click();
  await alice.locator("[data-board-drawer] [data-board-doc-remove]").click();
  await expect(alice.locator("[data-board-drawer] [data-board-doc]")).toHaveCount(0);
  await alice.keyboard.press("Escape");
  await expect(alice.locator("[data-board-row] [data-board-doc]")).toHaveCount(0);
});

test("Bob, a viewer on a phone, sees the Docs and none of the buttons", async () => {
  test.setTimeout(90_000);
  // Alice links one again for him to see.
  expect((await api(alice, "POST", `/api/boards/${board}/docs`, { row: rowId, pointer: { kind: "machine_doc", machine, doc: "00112233445566ff", title: "Perf notes" } })).status).toBe(200);
  await platform(bob, `/p/${project}/boards/${board}`);
  await expect(strip(bob)).toBeVisible({ timeout: 15_000 });
  await expect(chip(strip(bob), PLAN)).toBeVisible();
  await expect(strip(bob).locator("[data-board-doc-add], [data-board-doc-remove]")).toHaveCount(0);
  await bob.screenshot({ path: join(shots, "strip-390-dark.png") });
  await bob.locator("[data-board-row]").first().locator("[data-board-row-menu]").click();
  await bob.getByRole("menuitem", { name: "Details" }).click();
  const drawer = bob.locator("[data-board-drawer]");
  // Alice's machine isn't one Bob reaches: the Doc is on "a machine", a plain chip with nothing to open.
  const perf = chip(drawer.locator("[data-board-drawer-docs]"), "Perf notes");
  await expect(perf).toBeVisible();
  await expect(perf).toContainText("on a machine");
  await expect(perf.locator("[data-board-doc-open]")).toHaveCount(0);
  // Bob's board page has no sideways scroll.
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
  await expect(drawer.locator("[data-board-drawer-doc-add], [data-board-doc-remove]")).toHaveCount(0);
  await bob.screenshot({ path: join(shots, "drawer-390-dark.png") });
  // A viewer's write is refused by the platform.
  const refused = await api(bob, "POST", `/api/boards/${board}/docs`, { pointer: { kind: "doc", id: "pdc_00000000000000000000000000000000" } });
  expect(refused.status).toBe(403);
});

// Native #673: a machine Doc whose copy sits in the board's project is read
// from the copy: the chip has the copy's title and counts and opens the
// copy's page while the machine is away. The platform's side is tested in
// Rust (`a_machine_docs_chip_reads_its_copy_only_in_the_copys_project`, and
// `a_reader_not_told_the_machine_still_reads_a_copy_in_the_boards_project`);
// here the board's answer is given a `copy`, as a platform with it answers.
test("a machine Doc's chip opens its copy in the project", async () => {
  test.setTimeout(90_000);
  const plan = (await api(alice, "GET", `/api/projects/${project}/docs`)).body!.docs.find((d: any) => d.title === PLAN).id as string;
  const linked = await api(alice, "POST", `/api/boards/${board}/docs`, { pointer: { kind: "machine_doc", machine, doc: "00112233445566aa", title: "Bench notes" } });
  expect(linked.status).toBe(200);
  const entry = linked.body!.docs.find((d: any) => d.title === "Bench notes").id as string;
  const withCopy = async (route: import("@playwright/test").Route) => {
    const res = await route.fetch();
    const v = (await res.json()) as { docs: Record<string, unknown>[] };
    const docs = v.docs.map((d) => (d.id === entry ? { ...d, title: "Bench notes, v2", sections: 2, written: 1, copy: plan } : d));
    await route.fulfill({ response: res, json: { ...v, docs } });
  };
  await alice.route(`**/api/boards/${board}`, withCopy);
  try {
    await platform(alice, `/p/${project}/boards/${board}`);
    const c = chip(strip(alice), "Bench notes, v2");
    await expect(c).toBeVisible({ timeout: 15_000 });
    await expect(c).toContainText("1/2 on");
    await expect(c.locator("[data-board-doc-open]")).toHaveAttribute("data-board-doc-copy", plan);
    await c.locator("[data-board-doc-open]").click();
    await expect(alice.locator("[data-doc-page]")).toBeVisible({ timeout: 15_000 });
    await expect(alice.locator("[data-doc-page] .doc-title")).toHaveText(PLAN);
  } finally {
    await alice.unroute(`**/api/boards/${board}`, withCopy);
  }
  // Without a copy (an older platform, or none yet), the chip says where it is.
  await platform(alice, `/p/${project}/boards/${board}`);
  await expect(chip(strip(alice), "Bench notes")).toContainText("on alices");
  await expect(chip(strip(alice), "Bench notes").locator("[data-board-doc-copy]")).toHaveCount(0);
  expect((await api(alice, "DELETE", `/api/boards/${board}/docs/${entry}`)).status).toBe(200);
});
