// The project's Overview (Arugula inversion, L6), in the app's frame over
// the suite's daemon (app-frame-page.tsx): the project's boards with only
// their cards in Doing and Review drawn, Next and Done as counts; a card's
// tile holding the session on it (an agent block on the card, with the
// terminal in its session), and Not on a card last; a card opening the
// board's drawer, its address and the path in the bar, Esc and the path
// climbing back; a session opening on its machine's page; a question in a
// card's session pulsing its tile and answered in place on Needs you; the
// card's drawer drawing its session live with its question (#987's, as on
// the board); the hub's sessions by card adding a cloud workspace's
// session; and on a phone, one list with Needs you on top. The platform's boards and the
// hub's sessions are made up here; the panes are the daemon's own.
import { fileURLToPath } from "node:url";
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

const P = "prj_aaaa0002";
const B = "brd_e2e0launch";
const LATER = "brd_e2e1later";

const fakeAgent = fileURLToPath(new URL("../../crates/daemon/tests/fake_acp.py", import.meta.url));
const hookInput = fileURLToPath(new URL("../../crates/daemon/tests/fixtures/s13-hook-ask.json", import.meta.url));
const cli = fileURLToPath(new URL("../../target/debug/arugula", import.meta.url));

// A page a route fulfils has no address of its own, so Chrome counts it as
// public and won't let it reach the daemon on loopback without this.
test.use({ permissions: ["local-network-access"] });

type Lane = "next" | "doing" | "review" | "done";
const LANES: Lane[] = ["next", "doing", "review", "done"];
const board = (id: string, title: string) => ({ id, project: P, title, created_by: "", created_at: 1, updated_at: 1, goal: "", done_note: "", progress: { done: 0, total: 0, proven: 0, review: 0, doing: 0 } });
/** A board's view as the platform answers it: a row is `[id, lane, number, title]`, its card the native issue `iss_<number>`. */
function view(id: string, title: string, rows: [string, Lane, number, string][], attached: Record<string, unknown[]> = {}) {
  const row = ([rid, lane, n, t]: [string, Lane, number, string]) => ({
    id: rid, rev: 1, pointer: { kind: "issue", id: `iss_${n}` }, note: "", status: "none", lane, attachments: attached[rid] ?? [],
    live: { state: "open", kind: "native", number: n, title: t }, created_at: 1, updated_at: n,
  });
  return { board: board(id, title), zones: LANES.map((l) => ({ id: `lane_${l}`, rev: 1, title: l[0].toUpperCase() + l.slice(1), kind: "lane", rows: rows.filter((r) => r[1] === l).map(row) })), groups: [], docs: [] };
}
const ROWS: [string, Lane, number, string][] = [
  ["row_a", "next", 11, "Write the changelog"],
  ["row_b", "doing", 12, "Fix the installer"],
  ["row_c", "doing", 13, "Tidy the docs"],
  ["row_r", "review", 14, "Cut the release"],
  ["row_d", "done", 15, "Pick a name"],
];
/** `live`: #12's work is the suite's daemon's pane (the machine `m_suite` on the fixture page), as #987's drawer draws it. */
const launch = (live?: number) =>
  view(B, "Launch", ROWS, live === undefined ? {} : { row_b: [{ id: "att_term", role: "work", pointer: { kind: "block", machine: "m_suite", pane: String(live) }, block: { state: "working", title: "Fixer" } }] });

/** The platform's boards for the project, and the hub's sessions by card (`null`: the route isn't there yet, 404). */
async function serve(page: Page, byCard: unknown = null, live?: number) {
  await page.route(`**/api/projects/${P}/boards`, (r) => r.fulfill({ json: { project: P, boards: [board(B, "Launch"), board(LATER, "Later")] } }));
  await page.route(`**/api/boards/${B}`, (r) => r.fulfill({ json: launch(live) }));
  await page.route(`**/api/boards/${LATER}`, (r) => r.fulfill({ json: view(LATER, "Later", [["row_z", "next", 21, "Someday"]]) }));
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => (byCard ? r.fulfill({ json: byCard }) : r.fulfill({ status: 404, json: { error: "not found" } })));
  await page.route("**/api/issues/iss_*", (r) => r.fulfill({ status: 404, json: { error: "not found" } }));
}

/** The app on the project's Overview, with an agent block on #12 beside the reset's terminal (in its session), and a second session in the project on no card. */
async function overview(page: Page, byCard: unknown = null, live = false): Promise<{ term: number; agent: number; scratch: number }> {
  await reset(page);
  const [term] = await panes(page);
  await serve(page, byCard, live ? term : undefined);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(`/p/${P}`);
  await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
  await expect.poll(() => page.evaluate(() => (window as unknown as { __app?: { client: { state: unknown } } }).__app?.client.state !== null)).toBe(true);
  const made = await page.evaluate(
    async ({ fakeAgent, term, P, B }) => {
      const json = (body: unknown) => ({ method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
      const config = { agent: "acp", command: ["python3", fakeAgent], cwd: "/tmp", prompt: "hello", work: { issue: "#12" } };
      const agent = (await (await fetch("/api/blocks", json({ type: "agent", config, from_pane: term }))).json()).block as number;
      await fetch(`/api/blocks/${agent}/call/set_work`, json({ boards: [{ project: P, board: B, title: "Launch", row: "row_b" }] }));
      // A session of its own in the project, on no card.
      const c = (window as unknown as { __app: { client: { intent(i: unknown): void; state: { sessions: { id: number }[] } | null } } }).__app.client;
      const before = c.state!.sessions.map((s) => s.id);
      c.intent({ op: "new_session", name: "scratch", from_pane: null });
      return { agent, before };
    },
    { fakeAgent, term, P, B },
  );
  const scratchSession = await page.evaluate(async (before) => {
    const c = (window as unknown as { __app: { client: { state: { sessions: { id: number }[] } | null } } }).__app.client;
    for (let i = 0; i < 100; i++) {
      const s = c.state!.sessions.find((x) => !before.includes(x.id));
      if (s) return s.id;
      await new Promise((r) => setTimeout(r, 100));
    }
    return null;
  }, made.before);
  expect(scratchSession).not.toBeNull();
  await page.evaluate(
    ({ session, P }) => (window as unknown as { __app: { client: { intent(i: unknown): void } } }).__app.client.intent({ op: "set_session_project", session, project: { id: P, name: "Arugula" } }),
    { session: scratchSession!, P },
  );
  const scratch = await page.evaluate((session) => {
    const c = (window as unknown as { __app: { client: { state: { panes: { id: number; session: number }[] } | null } } }).__app.client;
    return c.state!.panes.find((p) => p.session === session)?.id ?? null;
  }, scratchSession!);
  return { term, agent: made.agent, scratch: scratch! };
}

const tile = (page: Page, row: string) => page.locator(`[data-ov-card="${row}"]`);

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("boards, then the cards in Doing and Review, each holding its sessions; Not on a card last", async ({ page }, info) => {
    const { term, agent } = await overview(page);
    const launch = page.locator(`[data-ov-board="${B}"]`);
    await expect(launch.locator("[data-ov-board-title]")).toHaveText("Launch");
    await expect(launch.locator("[data-ov-board-line]")).toHaveText("3 in flight · 1 next · 1 of 5 done");
    // A board with nothing in flight isn't drawn; Next and Done are counts, not tiles.
    await expect(page.locator(`[data-ov-board="${LATER}"]`)).toHaveCount(0);
    await expect(launch.locator("[data-ov-lane]")).toHaveCount(2);
    await expect(launch.locator('[data-ov-lane="Doing"] [data-ov-card]')).toHaveCount(2);
    await expect(launch.locator('[data-ov-lane="Review"] [data-ov-card]')).toHaveCount(1);
    for (const r of ["row_a", "row_d"]) await expect(tile(page, r)).toHaveCount(0);
    // #12 has the agent's session, its terminal in it, and comes before #13, which no one is on.
    const s = tile(page, "row_b").locator("[data-ov-session]");
    await expect(s).toHaveCount(1, { timeout: 20_000 });
    await expect.poll(async () => ((await s.getAttribute("data-ov-panes")) ?? "").split(" ").map((k) => Number(k.split(":").pop())).sort()).toEqual([term, agent].sort());
    await expect(s.locator("[data-ov-where]")).not.toBeEmpty();
    await expect(launch.locator('[data-ov-lane="Doing"] [data-ov-card]').first()).toHaveAttribute("data-ov-card", "row_b");
    await expect(tile(page, "row_c")).toHaveAttribute("data-ov-standing", "idle");
    await expect(tile(page, "row_c").locator("[data-ov-card-state]")).toHaveText("no one on it");
    // Not on a card: the scratch session, after the boards.
    await expect(page.locator("[data-ov-loose] [data-ov-session]")).toHaveCount(1);
    const order = await page.locator("[data-ov-field] > *").evaluateAll((els) => els.map((e) => (e.hasAttribute("data-ov-board") ? "board" : e.hasAttribute("data-ov-loose") ? "loose" : e.hasAttribute("data-ov-more") ? "more" : "")).filter(Boolean));
    expect(order).toEqual(["board", "loose", "more"]);
    // What the project's page showed before is still there, below.
    await expect(page.locator("[data-ov-more] [data-project-rooms]")).toBeAttached();
    await expect(page.locator(".ws-rail [data-open-overview]")).toHaveAttribute("aria-current", "page");
    await page.screenshot({ path: info.outputPath("overview.png") });
  });

  test("a card opens the board's drawer; Esc and the path climb back; a session opens on its machine", async ({ page }, info) => {
    const { agent } = await overview(page);
    await tile(page, "row_b").locator("[data-ov-card-open]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/?]*\\?board=${B}&card=row_b$`));
    const drawer = page.locator('[data-board-drawer="row_b"]');
    await expect(drawer).toBeVisible();
    await expect(drawer.locator("[data-board-drawer-title]")).toHaveText("Fix the installer");
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula team\s*Arugula\s*Overview\s*Launch\s*#12 · Fix the installer/);
    await page.screenshot({ path: info.outputPath("overview-drawer.png") });
    await page.keyboard.press("Escape");
    await expect(drawer).toHaveCount(0);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/?]*$`));
    // The path's Overview is a level up too; its board's name, the board.
    await tile(page, "row_c").click();
    await expect(page.locator('[data-board-drawer="row_c"]')).toBeVisible();
    await expect(page.locator("[data-app-path] [data-path=board]")).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*/boards/${B}`));
    await page.locator("[data-app-path] [data-path=home]").click();
    await expect(page.locator("[data-board-drawer]")).toHaveCount(0);
    // A session, in one click: its machine's page, on its pane.
    await tile(page, "row_b").locator("[data-ov-session]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines.*pane=${agent}`));
    await expect(page.locator(`[data-pane="${agent}"]`)).toBeVisible();
    // Overview on the rail, back.
    await page.locator(".ws-rail [data-open-overview]").click();
    await expect(page.locator("[data-overview]")).toBeVisible();
  });

  test("a question in a card's session pulses its tile, and is answered in place on Needs you", async ({ page }, info) => {
    const { term } = await overview(page);
    const needs = page.locator("[data-ov-needs]");
    // The agent finishing may put a Finished card there (Swarm's rail does); nothing asks yet.
    await expect(needs.locator('.swarm-card[data-kind="ask"]')).toHaveCount(0);
    // Claude Code's question, asked in the agent's terminal.
    await page.evaluate(
      ({ term, line }) => fetch(`/api/panes/${term}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: line, enter: true }) }),
      { term, line: `out=$(${cli} ask < ${hookInput}); echo "ONE[\${#out}]"; echo "$out" | grep -o '"Which [a-z ]*?":"[^"]*"'` },
    );
    const card = needs.locator('.swarm-card[data-kind="ask"]');
    await expect(card).toHaveCount(1, { timeout: 20_000 });
    // It names its card, and the card waits on you.
    await expect(card.locator("[data-card-work]")).toHaveText("#12 · Fix the installer");
    await expect(tile(page, "row_b")).toHaveAttribute("data-ov-standing", "needs");
    await expect(tile(page, "row_b")).toHaveClass(/ov-pulse/);
    await expect(needs.locator("[data-ov-needs-count]")).toHaveText("1");
    await expect(card).toBeVisible();
    // The session is named by its agent, though it's the terminal that asks.
    await expect(tile(page, "row_b").locator("[data-ov-session] b")).toHaveText("Agent");
    await page.waitForTimeout(400);
    await page.screenshot({ path: info.outputPath("overview-needs.png") });
    const q = (n: number) => card.locator(`[data-question="${n}"]`);
    await q(0).getByRole("radio", { name: /^Blue/ }).click();
    await q(1).getByRole("checkbox", { name: /Pear/ }).check();
    await q(2).getByRole("radio", { name: /^Cat/ }).click();
    await card.getByRole("button", { name: "Submit" }).click();
    await expect(tile(page, "row_b")).not.toHaveAttribute("data-ov-standing", "needs", { timeout: 20_000 });
    await expect.poll(() => page.evaluate((t) => (window as unknown as { __app: { text(p: number): string } }).__app.text(t), term)).toContain('"Which colour do you prefer?":"Blue"');
  });

  test("the card's drawer draws its session live, and its question at the top, as on the board", async ({ page }, info) => {
    const { term } = await overview(page, null, true);
    await tile(page, "row_b").locator("[data-ov-card-open]").click();
    const drawer = page.locator('[data-board-drawer="row_b"]');
    const live = drawer.locator("[data-board-drawer-live]");
    await expect(live.locator(`[data-live-slot="${term}"] .xterm`)).toBeVisible({ timeout: 20_000 });
    // The session's question, answered at the drawer's top.
    await page.evaluate(
      ({ term, line }) => fetch(`/api/panes/${term}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: line, enter: true }) }),
      { term, line: `out=$(${cli} ask < ${hookInput}); echo "$out" | grep -o '"Which [a-z ]*?":"[^"]*"'` },
    );
    const ask = drawer.locator("[data-board-drawer-ask]");
    await expect(ask).toBeVisible({ timeout: 20_000 });
    await expect(ask.locator("h3")).toHaveText("Claude Code asks");
    await page.screenshot({ path: info.outputPath("overview-drawer-live.png") });
    await ask.locator('[data-question="0"]').getByRole("radio", { name: /^Blue/ }).click();
    await ask.locator('[data-question="1"]').getByRole("checkbox", { name: /Pear/ }).check();
    await ask.locator('[data-question="2"]').getByRole("radio", { name: /^Cat/ }).click();
    await ask.getByRole("button", { name: "Submit" }).click();
    await expect(ask).toHaveCount(0);
    await expect.poll(() => page.evaluate(async (p) => (await fetch(`/api/panes/${p}/capture?format=text`)).text(), term)).toContain('"Which colour do you prefer?":"Blue"');
  });

  test("the hub's sessions by card add the sessions the machines don't show here", async ({ page }) => {
    const byCard = {
      project: P,
      groups: [
        { card: { id: "iss_13", number: 13, title: "Tidy the docs" }, sessions: [{ kind: "hosted", id: "hs_e2e", name: "arugula/13-tidy-docs", on: { kind: "hosted" }, state: "working", updated_at: 0 }] },
        { card: null, sessions: [{ kind: "machine", id: "9", name: "release", on: { kind: "machine", id: "0123456789abcdef", name: "far-away" }, panes: [{ pane: 1, kind: "terminal", work_item: null, state: null }], updated_at: 0 }] },
      ],
    };
    await overview(page, byCard);
    const hosted = tile(page, "row_c").locator('[data-ov-session="hosted:hs_e2e"]');
    await expect(hosted).toContainText("arugula/13-tidy-docs");
    await expect(hosted.locator("[data-ov-where]")).toHaveText("Arugula cloud");
    await expect(tile(page, "row_c")).toHaveAttribute("data-ov-standing", "working");
    // A machine this page doesn't reach: named, on no card, and not a link.
    const far = page.locator("[data-ov-loose] [data-ov-session]", { hasText: "release" });
    await expect(far.locator("[data-ov-where]")).toHaveText("far-away");
    await expect(far).toBeDisabled();
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the Overview is one list, with Needs you on top", async ({ page }, info) => {
    await overview(page);
    const ov = page.locator("[data-overview]");
    await expect(ov).toHaveClass(/phone/);
    const first = await ov.locator("> *").first().evaluate((e) => [...e.attributes].map((a) => a.name));
    expect(first).toContain("data-ov-needs");
    await expect(tile(page, "row_b")).toBeVisible();
    await expect(page.locator("[data-phone-tabs] [data-tab-overview]")).toHaveAttribute("aria-current", "page");
    await page.screenshot({ path: info.outputPath("overview-phone.png") });
  });
});
