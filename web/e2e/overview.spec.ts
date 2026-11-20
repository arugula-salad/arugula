// The project's Overview (Arugula inversion, L6), in the app's frame over
// the suite's daemon (app-frame-page.tsx): the project's open boards, one
// row each with its count and what's going on in words, the window's width;
// a board opened by a click, or by itself when it asks you, with only its
// cards in Doing and Review drawn, one line each with one word for how it
// stands; none of the old home's lists, and the team's faces in the header
// only while chat is on; a card's preview, on hover or keyboard
// focus, showing the session on it (an agent block on the card, with the
// terminal in its session) and its last line, and Not on a card last; a
// card with no session opening the board's drawer (one with a session, by
// its peek's card link), its address and the path in the bar, Esc and the
// path climbing back; a session opening on its machine's page from its
// peek; a session's peek (#1271): an agent block's and a shell's last lines
// and a box that reaches them, Silence and Stop, a cloud workspace's last
// answer, and a far machine's read-only; a question in a
// card's session pulsing its row and answered in place on Needs you; the
// card's drawer drawing its session live with its question (#987's, as on
// the board); the hub's sessions by card adding a cloud workspace's
// session; a board's thread opened from its row as a tab in the talk
// panel under Needs you, its messages and a box that posts (#1270); the
// briefing line counted from the event log since the reader last looked
// (#1277); and on a phone, one list with Needs you on top. The platform's boards and the
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
const launch = (live?: number, lead?: unknown) => {
  const v = view(B, "Launch", ROWS, live === undefined ? {} : { row_b: [{ id: "att_term", role: "work", pointer: { kind: "block", machine: "m_suite", pane: String(live) }, block: { state: "working", title: "Fixer" } }] });
  return lead ? { ...v, board: { ...v.board, lead } } : v;
};

/** The platform's boards for the project, and the hub's sessions by card (`null`: the route isn't there yet, 404). */
async function serve(page: Page, byCard: unknown = null, live?: number, lead?: () => unknown) {
  await page.route(`**/api/projects/${P}/boards`, (r) => r.fulfill({ json: { project: P, boards: [board(B, "Launch"), board(LATER, "Later")] } }));
  await page.route(`**/api/boards/${B}`, (r) => r.fulfill({ json: launch(live, lead?.()) }));
  await page.route(`**/api/boards/${LATER}`, (r) => r.fulfill({ json: view(LATER, "Later", [["row_z", "next", 21, "Someday"]]) }));
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => (byCard ? r.fulfill({ json: byCard }) : r.fulfill({ status: 404, json: { error: "not found" } })));
  await page.route("**/api/issues/iss_*", (r) => r.fulfill({ status: 404, json: { error: "not found" } }));
}

/** The app on the project's Overview, with an agent block on #12 beside the reset's terminal (in its session), and a second session in the project on no card. */
async function overview(page: Page, byCard: unknown = null, live = false, lead?: () => unknown): Promise<{ term: number; agent: number; scratch: number }> {
  await reset(page);
  const [term] = await panes(page);
  await serve(page, byCard, live ? term : undefined, lead);
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

/** Launch's room on control, its messages and what's posted in it (in
 * `window.said`): the fixture page answers chat itself, so it's made up in
 * the page. */
async function launchRoom(page: Page) {
  await page.evaluate((B) => {
    const room = { id: "room_launch", kind: "object", team: null, name: "Launch", machine: null, machine_name: null, target: B, project: null, with: null, others: [], private: false, last: 1, read: 1, unread: 0, mentions: 0, wants: 0, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true };
    const msg = (seq: number, name: string, text: string) => ({ seq, at: Date.now() - 60_000 + seq, author: `acct_${name}`, name, text, quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent: null, reactions: [], files: [], pinned: false, also: false });
    const said = [msg(1, "Jo", "Release notes are drafted.")];
    (window as unknown as { said: typeof said }).said = said;
    const now = () => ({ ...room, last: said.length, read: said.length });
    const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
    const before = window.fetch;
    window.fetch = async (input, init) => {
      const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
      if (url.pathname === "/api/chat/targets") return json(now());
      if (url.pathname === "/api/chat/rooms/room_launch/messages") {
        if (init?.method === "POST") {
          said.push(msg(said.length + 1, "Me", (JSON.parse(String(init.body)) as { text: string }).text));
          return json({ seq: said.length, mentions: [], unreached: [] });
        }
        const after = Number(url.searchParams.get("after") ?? 0);
        return json({ room: now(), messages: said.filter((m) => m.seq > after), more: false });
      }
      return before(input, init);
    };
  }, B);
}

/** Chat on, as the page reads it once it has said it's off. */
async function chatOn(page: Page) {
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate(() => {
    const c = (window as unknown as { __chat: { on: boolean | null; emit(): void } }).__chat;
    c.on = true;
    c.emit();
  });
}

const tile = (page: Page, row: string) => page.locator(`[data-ov-card="${row}"]`);
/** Open Launch's row, to its cards in flight. */
async function openLaunch(page: Page) {
  await page.locator(`[data-ov-board="${B}"] [data-ov-board-toggle]`).click();
  await expect(page.locator(`[data-ov-board="${B}"]`)).toHaveAttribute("data-ov-open", "");
}

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("a row per board, opened to the cards in Doing and Review, a line each; a card's preview holds its sessions; Not on a card last", async ({ page }, info) => {
    const { term, agent } = await overview(page);
    // The page takes the window's width, not the reading column.
    const width = await page.locator("[data-overview]").evaluate((e) => e.getBoundingClientRect().width);
    expect(width).toBeGreaterThan(1000);
    const launch = page.locator(`[data-ov-board="${B}"]`);
    await expect(launch.locator("[data-ov-board-title]")).toHaveText("Launch");
    await expect(launch.locator("[data-ov-board-count]")).toHaveText("1/5");
    await expect(launch.locator("[data-ov-board-says]")).toContainText("1 in review");
    await expect(launch.locator(".board-bar i")).not.toHaveCount(0);
    // A board with nothing in flight is a row too, last, and doesn't open.
    const later = page.locator(`[data-ov-board="${LATER}"]`);
    await expect(later.locator("[data-ov-board-count]")).toHaveText("0/1");
    await expect(later.locator("[data-ov-board-says]")).toHaveText("nothing in flight");
    await expect(later.locator("[data-ov-board-toggle]")).toBeDisabled();
    await expect.poll(() => page.locator("[data-ov-board]").evaluateAll((els) => els.map((e) => e.getAttribute("data-ov-board")))).toEqual([B, LATER]);
    // Nothing asks you, so Launch is closed; a click on its row opens it, its title is the board.
    await expect(launch.locator("[data-ov-card]")).toHaveCount(0);
    await expect(launch.locator("[data-ov-board-title]")).toHaveAttribute("href", new RegExp(`/boards/${B}`));
    await launch.locator("[data-ov-board-says]").click();
    await expect(launch).toHaveAttribute("data-ov-open", "");
    // Doing and Review in one list, no lane headings, no Open board link; under four, so no "more".
    await expect(launch.locator("[data-ov-card]")).toHaveCount(3);
    await expect(launch.locator("[data-ov-board-open], [data-ov-more-rows]")).toHaveCount(0);
    for (const r of ["row_a", "row_d"]) await expect(tile(page, r)).toHaveCount(0);
    await expect(tile(page, "row_b").locator("[data-ov-card-title]")).toHaveText("Fix the installer");
    // #13, which no one is on, says nothing; #14, in Review, says so.
    await expect(tile(page, "row_c")).toHaveAttribute("data-ov-standing", "idle");
    await expect(tile(page, "row_c").locator("[data-ov-card-state]")).toHaveCount(0);
    await expect(tile(page, "row_r").locator("[data-ov-card-state]")).toHaveText("in review");
    // #12 comes first; its sessions show only in its preview, and no last line is read until then.
    await expect(launch.locator("[data-ov-card]").first()).toHaveAttribute("data-ov-card", "row_b");
    await expect(page.locator("[data-ov-preview], [data-ov-line]")).toHaveCount(0);
    // Hovering #12 opens it: the agent's session, its terminal in it, where it runs and its last line.
    await tile(page, "row_b").hover();
    const s = tile(page, "row_b").locator("[data-ov-preview] [data-ov-session]");
    await expect(s).toHaveCount(1, { timeout: 20_000 });
    await expect.poll(async () => ((await s.getAttribute("data-ov-panes")) ?? "").split(" ").map((k) => Number(k.split(":").pop())).sort()).toEqual([term, agent].sort());
    await expect(s.locator("[data-ov-where]")).not.toBeEmpty();
    await expect(s.locator("[data-ov-line]")).toBeAttached();
    await page.screenshot({ path: info.outputPath("overview-preview.png") });
    // Off it, it closes; keyboard focus opens it too, and Esc closes it.
    await page.mouse.move(1, 1);
    await expect(page.locator("[data-ov-preview]")).toHaveCount(0);
    // (Worst first: #12, then #14 in review, then #13.)
    await expect.poll(() => launch.locator("[data-ov-card]").evaluateAll((els) => els.map((e) => e.getAttribute("data-ov-card")))).toEqual(["row_b", "row_r", "row_c"]);
    await tile(page, "row_r").locator("[data-ov-row-open]").focus();
    await page.keyboard.press("Shift+Tab");
    await expect(tile(page, "row_b").locator("[data-ov-preview]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-ov-preview]")).toHaveCount(0);
    // Not on a card: the scratch session, after the boards.
    await expect(page.locator("[data-ov-loose] [data-ov-session]")).toHaveCount(1);
    const order = await page.locator("[data-ov-field] > *").evaluateAll((els) => els.map((e) => (e.hasAttribute("data-ov-boards") ? "boards" : e.hasAttribute("data-ov-loose") ? "loose" : "")).filter(Boolean));
    expect(order).toEqual(["boards", "loose"]);
    // None of the old home's lists: no chat, no people, no issues, runs, repos or workspaces.
    await expect(page.locator("[data-project-rooms], [data-chat-off], [data-project-members], [data-project-runs], [data-project-repos], [data-project-envs], [data-project-boards], [data-ov-more]")).toHaveCount(0);
    // Chat is off here: no faces in the header.
    await expect(page.locator("[data-project-faces]")).toHaveCount(0);
    await expect(page.locator(".ws-rail [data-open-plan]")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-space-places=plan] [data-space-overview]")).toHaveAttribute("aria-current", "page");
    await page.screenshot({ path: info.outputPath("overview.png") });
  });

  test("a card opens the board's drawer; Esc and the path climb back; a session opens on its machine", async ({ page }, info) => {
    const { agent } = await overview(page);
    await openLaunch(page);
    // #12 has a session: its row peeks at it, and the peek's card link is the drawer.
    await tile(page, "row_b").locator("[data-ov-row-open]").click();
    await page.locator("[data-ov-peek-card]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/?]*\\?board=${B}&card=row_b$`));
    const drawer = page.locator('[data-board-drawer="row_b"]');
    await expect(drawer).toBeVisible();
    await expect(drawer.locator("[data-board-drawer-title]")).toHaveText("Fix the installer");
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula team\s*Arugula\s*Overview\s*Launch\s*#12 · Fix the installer/);
    await page.screenshot({ path: info.outputPath("overview-drawer.png") });
    await page.keyboard.press("Escape");
    await expect(drawer).toHaveCount(0);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/?]*$`));
    // The path's Overview is a level up too; its board's name, the board. (#12, focused again, shows its preview on the right.)
    await tile(page, "row_c").locator("[data-ov-row-open]").click({ position: { x: 12, y: 8 } });
    await expect(page.locator('[data-board-drawer="row_c"]')).toBeVisible();
    await expect(page.locator("[data-app-path] [data-path=board]")).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*/boards/${B}`));
    await page.locator("[data-app-path] [data-path=home]").click();
    await expect(page.locator("[data-board-drawer]")).toHaveCount(0);
    // A session, from its card's preview: its peek, and Open in Panes is its machine's page, on its pane.
    await tile(page, "row_b").hover();
    await tile(page, "row_b").locator("[data-ov-session]").click();
    await page.locator("[data-ov-peek-panes]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines.*pane=${agent}`));
    await expect(page.locator(`[data-pane="${agent}"]`)).toBeVisible();
    // Plan on the rail opens on the Overview, back.
    await page.locator(".ws-rail [data-open-plan]").click();
    await expect(page.locator("[data-overview]")).toBeVisible();
  });

  test("a question in a card's session pulses its row, and is answered in place on Needs you", async ({ page }, info) => {
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
    // It names its card, and the card waits on you: its board opens by itself.
    await expect(card.locator("[data-card-work]")).toHaveText("#12 · Fix the installer");
    await expect(page.locator(`[data-ov-board="${B}"]`)).toHaveAttribute("data-ov-open", "");
    await expect(page.locator(`[data-ov-board="${B}"] [data-ov-board-says] .tone-accent`)).toHaveText("1 needs you");
    await expect(tile(page, "row_b")).toHaveAttribute("data-ov-standing", "needs");
    await expect(tile(page, "row_b")).toHaveClass(/ov-pulse/);
    await expect(needs.locator("[data-ov-needs-count]")).toHaveText("1");
    await expect(card).toBeVisible();
    await expect(tile(page, "row_b").locator("[data-ov-card-state]")).toHaveText("needs you");
    // The session is named by its agent, though it's the terminal that asks.
    await tile(page, "row_b").hover();
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
    await openLaunch(page);
    await tile(page, "row_b").locator("[data-ov-row-open]").click();
    await page.locator("[data-ov-peek-card]").click();
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

  test("a board's thread opens as a tab in the talk panel, under Needs you, with its messages and a box that posts (Arugula #1270)", async ({ page }, info) => {
    await overview(page);
    await launchRoom(page);
    const panel = page.locator("[data-ov-panel]");
    // Needs you is the panel's top; no tab yet, and with chat off a row has no way to its thread.
    await expect(panel.locator("> [data-ov-needs]")).toBeVisible();
    await expect(panel.locator("[data-ov-talk-none]")).toBeVisible();
    await expect(page.locator("[data-ov-board-talk]")).toHaveCount(0);
    await chatOn(page);
    const width = await page.locator("[data-ov-field]").evaluate((e) => e.getBoundingClientRect().width);
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-talk]`).click();
    const tab = panel.locator(`[data-ov-tab="thread:${B}"]`);
    await expect(tab.getByRole("tab")).toHaveAttribute("aria-selected", "true");
    await expect(tab.getByRole("tab")).toHaveText("Launch");
    const open = panel.locator(`[data-ov-talk-open="thread:${B}"]`);
    await expect(open.locator('[data-room="room_launch"]')).toContainText("Release notes are drafted.");
    // The field keeps its width and its place.
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    expect(await page.locator("[data-ov-field]").evaluate((e) => e.getBoundingClientRect().width)).toBe(width);
    // The box posts in the board's room.
    const box = open.locator(".thread-compose textarea");
    await box.fill("Ship it on Tuesday.");
    await box.press("Enter");
    await expect(open.locator('[data-room="room_launch"]')).toContainText("Ship it on Tuesday.");
    expect(await page.evaluate(() => (window as unknown as { said: { text: string }[] }).said.map((m) => m.text))).toContain("Ship it on Tuesday.");
    await page.screenshot({ path: info.outputPath("overview-talk.png") });
    // Opening it again doesn't make a second tab; × closes it.
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-talk]`).click();
    await expect(panel.locator("[data-ov-tab]")).toHaveCount(1);
    await tab.locator("[data-ov-tab-close]").click();
    await expect(panel.locator("[data-ov-tab]")).toHaveCount(0);
    await expect(panel.locator("[data-ov-talk-none]")).toBeVisible();
  });

  test("the reader's staff room is the talk panel's first tab, pinned, saying which session hears it (Arugula #1275)", async ({ page }) => {
    await overview(page);
    const panel = page.locator("[data-ov-panel]");
    await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
    // No staff room yet: no pinned tab.
    await expect(panel.locator("[data-ov-tab]")).toHaveCount(0);
    await page.evaluate((P) => {
      const room = { id: "dm_cos", kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: 1, read: 0, unread: 1, mentions: 0, wants: 1, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead: { machine: "m_geek", machine_name: "geek", session: "cos" } };
      const said = [{ seq: 1, at: Date.now() - 60_000, author: "acct_me", name: "Me's agent on geek", text: "3 boards moved since this morning.", quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent: { machine: "m_geek", machine_name: "geek" }, reactions: [], files: [], pinned: false, also: false }];
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === "/api/chat/rooms/dm_cos/messages") return json({ room, messages: said, more: false });
        return before(input, init);
      };
      const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
      c.on = true;
      c.rooms = [...c.rooms, room];
      c.emit();
    }, P);
    const tab = panel.locator('[data-ov-tab="staff:dm_cos"]');
    await expect(tab.getByRole("tab")).toHaveText("Chief of staff");
    await expect(tab.getByRole("tab")).toHaveAttribute("aria-selected", "true");
    await expect(tab.locator("[data-ov-tab-close]")).toHaveCount(0);
    await expect(panel.locator("[data-ov-tab]").first()).toHaveAttribute("data-ov-tab", "staff:dm_cos");
    await expect(panel.locator('[data-ov-staff="dm_cos"]')).toContainText("3 boards moved since this morning.");
  });

  test("a card's row peeks at its agent, and a session on no card at its shell: last lines, a box that reaches them, Silence and Stop (Arugula #1271)", async ({ page }, info) => {
    await overview(page);
    await openLaunch(page);
    const panel = page.locator("[data-ov-panel]");
    // #12's row: a peek tab on its session, its agent block's lines read live from its machine.
    await tile(page, "row_b").locator("[data-ov-row-open]").click();
    const peek = panel.locator("[data-ov-peek]");
    await expect(peek).toHaveAttribute("data-ov-peek-kind", "block");
    await expect(panel.locator('[data-ov-tab^="peek:"] [role="tab"]')).toHaveAttribute("aria-selected", "true");
    await expect(peek.locator("[data-ov-peek-name]")).toHaveText("Agent");
    await expect(peek.locator("[data-ov-peek-where]")).not.toBeEmpty();
    await expect(peek.locator("[data-ov-peek-tail]")).toHaveAttribute("data-ov-peek-live", "");
    // The field stays where it is.
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    // Its box goes in as its composer's does: the agent answers it.
    const reply = peek.locator("[data-ov-peek-reply] input");
    await reply.fill("env PEEK_1271");
    await reply.press("Enter");
    await expect(peek.locator("[data-ov-peek-said]")).toContainText("next tool call");
    await expect(reply).toHaveValue("");
    await expect(peek.locator("[data-ov-peek-tail]")).toContainText("ENV PEEK_1271", { timeout: 20_000 });
    // Silence holds its mentions, on its machine; Allow lets them in again.
    await peek.locator('[data-ov-peek-silence="silence"]').click();
    await expect(peek.locator('[data-ov-peek-silence="allow"]')).toBeVisible({ timeout: 10_000 });
    await page.screenshot({ path: info.outputPath("overview-peek-agent.png") });
    await peek.locator('[data-ov-peek-silence="allow"]').click();
    await expect(peek.locator('[data-ov-peek-silence="silence"]')).toBeVisible({ timeout: 10_000 });
    // The scratch session, on no card: a shell, its box typed with Enter, its lines live.
    await page.locator("[data-ov-loose] [data-ov-session] [data-ov-row-open]").click();
    await expect(panel.locator('[data-ov-tab^="peek:"]')).toHaveCount(2);
    await expect(peek).toHaveAttribute("data-ov-peek-kind", "shell");
    await expect(peek.locator("[data-ov-peek-silence]")).toHaveCount(0);
    await reply.fill("echo PEEK-$((6*7))");
    await reply.press("Enter");
    await expect(peek.locator("[data-ov-peek-tail]")).toContainText("PEEK-42", { timeout: 20_000 });
    await page.screenshot({ path: info.outputPath("overview-peek-shell.png") });
    // Stop is Ctrl-C to a shell: a sleep ends.
    await reply.fill("sleep 300; echo SL''EPT");
    await reply.press("Enter");
    await peek.locator("[data-ov-peek-stop]").click();
    await expect(peek.locator("[data-ov-peek-said]")).toHaveText("Stopped.");
    await reply.fill("echo AFTER-$((2+2))");
    await reply.press("Enter");
    await expect(peek.locator("[data-ov-peek-tail]")).toContainText("AFTER-4", { timeout: 20_000 });
    await expect(peek.locator("[data-ov-peek-tail]")).not.toContainText("SLEPT");
    // Open in Panes: the shell on its machine's page.
    const shell = ((await page.locator("[data-ov-loose] [data-ov-session]").getAttribute("data-ov-panes")) ?? "").split(":").pop();
    await peek.locator("[data-ov-peek-panes]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines.*pane=${shell}$`));
  });

  test("a board's lead is a chip on its row, with its session's dot, name and machine, that opens the board's thread and doesn't toggle the row (Arugula #1272)", async ({ page }, info) => {
    // Launch is led by the project's scratch session on the suite's daemon, which this page knows by its name; Later has no lead.
    let led: unknown;
    await overview(page, null, false, () => led);
    const tile = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(tile).toHaveAttribute("data-ov-panes", /:/);
    const host = (await tile.getAttribute("data-ov-panes"))!.split(":")[0];
    led = { machine: "m_suite", machine_name: host, session: "scratch", at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    await launchRoom(page);
    const lead = page.locator(`[data-ov-board="${B}"] [data-ov-board-lead]`);
    await expect(lead).toHaveAttribute("data-ov-board-lead", "scratch");
    await expect(lead.locator(".ov-lead-name")).toHaveText("scratch");
    await expect(lead.locator(".ov-lead-machine")).toHaveText(host);
    // Its session's panes are on this page, so its dot says how it stands.
    await expect(lead.locator("[data-ov-lead-standing]")).toHaveCount(1);
    await expect(page.locator(`[data-ov-board="${LATER}"] [data-ov-board-lead]`)).toHaveCount(0);
    // With chat off it's a label, and a click on it doesn't toggle the row.
    await expect(lead).not.toHaveRole("button");
    await lead.click();
    await expect(page.locator(`[data-ov-board="${B}"]`)).not.toHaveAttribute("data-ov-open", "");
    await chatOn(page);
    await expect(lead).toHaveRole("button");
    await lead.click();
    const panel = page.locator("[data-ov-panel]");
    await expect(panel.locator(`[data-ov-tab="thread:${B}"]`).getByRole("tab")).toHaveAttribute("aria-selected", "true");
    await expect(panel.locator(`[data-ov-talk-open="thread:${B}"] [data-room="room_launch"]`)).toContainText("Release notes are drafted.");
    await expect(page.locator(`[data-ov-board="${B}"]`)).not.toHaveAttribute("data-ov-open", "");
    await page.screenshot({ path: info.outputPath("overview-lead.png") });
  });

  test("the briefing line counts the event log since the reader last looked, says the chief of staff's latest sentence, opens its tab, and keeps the head it showed when the reader leaves (Arugula #1277)", async ({ page }, info) => {
    // Last looked at 18:04 yesterday, at #100 of the log; the head is #120.
    const d = new Date();
    const at = new Date(d.getFullYear(), d.getMonth(), d.getDate() - 1, 18, 4).getTime();
    const ev = (seq: number, type: string, subject: string, data: Record<string, unknown>) => ({ id: `evt_${seq}`, seq, type, v: 1, source: "github", scope: { team: "t", project: P }, subject, actor: null, at_ms: at + seq, recorded_ms: at + seq, data, key: `k${seq}` });
    const events = [
      // Two merged, one of them told twice.
      ev(101, "github.pr.merged", "o/app#5", { repo: "o/app", pr: 5 }),
      ev(102, "github.pr.merged", "o/app#6", { repo: "o/app", pr: 6 }),
      ev(103, "github.pr.merged", "o/app#6", { repo: "o/app", pr: 6 }),
      // main red twice (two checks) and #13's card's branch: two went red. A PR into main isn't main, nor is a branch on no card.
      ev(104, "github.check.failed", "o/app@aa", { repo: "o/app", heads: ["main"], branches: ["main"] }),
      ev(105, "github.check.failed", "o/app@aa", { repo: "o/app", heads: ["main"], branches: ["main"] }),
      ev(106, "github.check.failed", "o/app@bb", { repo: "o/app", heads: ["13-tidy-docs"], branches: ["13-tidy-docs", "main"] }),
      ev(107, "github.check.failed", "o/app@cc", { repo: "o/app", heads: ["someone-else"], branches: ["someone-else", "main"] }),
      // Ready: #7 asks the reader's review, #8 doesn't.
      ev(108, "github.pr.ready", "o/app#7", { repo: "o/app", pr: 7, branch: "x" }),
      ev(109, "github.pr.ready", "o/app#8", { repo: "o/app", pr: 8, branch: "y" }),
    ];
    const put: unknown[] = [];
    const reads: string[] = [];
    await page.route(`**/api/projects/${P}/overview/seen`, async (r) => {
      if (r.request().method() === "PUT") {
        put.push(r.request().postDataJSON());
        return r.fulfill({ json: { seq: 120, at_ms: Date.now() } });
      }
      return r.fulfill({ json: { seq: 100, at_ms: at } });
    });
    await page.route(`**/api/projects/${P}/events?*`, (r) => {
      const q = new URL(r.request().url()).searchParams;
      reads.push(`${q.get("after")} ${q.get("types")}`);
      return r.fulfill({ json: { events, next_after: 120, head: 120 } });
    });
    const byCard = { project: P, groups: [{ card: { id: "iss_13", number: 13, title: "Tidy the docs" }, sessions: [{ kind: "hosted", id: "hs_e2e", name: "arugula/13-tidy-docs", on: { kind: "hosted" }, state: "working", branch: "13-tidy-docs", updated_at: 0 }] }] };
    await overview(page, byCard);
    const line = page.locator("[data-ov-briefing]");
    await expect(line.locator("[data-ov-briefing-counts]")).toHaveText(/^since \d{1,2}:04\s?(PM|pm)? yesterday: 2 merged · 2 went red$/);
    expect(reads[0]).toBe("100 github.pr.merged,github.check.failed,github.pr.ready");
    await expect(line).toHaveAttribute("data-ov-briefing", "120");
    // No staff room yet: a line, not a button.
    await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
    await expect(line).not.toHaveRole("button");
    // The staff room: the chief of staff's latest message's first sentence beside the counts, and a click opens its tab.
    await page.evaluate((P) => {
      const room = { id: "dm_cos", kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: 2, read: 2, unread: 0, mentions: 0, wants: 0, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead: { machine: "m_geek", machine_name: "geek", session: "cos" } };
      const msg = (seq: number, text: string, agent: boolean) => ({ seq, at: Date.now() - 60_000 + seq, author: "acct_me", name: agent ? "Me's agent on geek" : "Me", text, quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent: agent ? { machine: "m_geek", machine_name: "geek" } : null, reactions: [], files: [], pinned: false, also: false });
      const said = [msg(1, "Main went red at 02:14 and the launch lead reverted it. Two PRs wait on you.", true), msg(2, "thanks", false)];
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === "/api/chat/rooms/dm_cos/messages") return json({ room, messages: said, more: false });
        return before(input, init);
      };
      const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
      c.on = true;
      c.rooms = [...c.rooms, room];
      c.emit();
    }, P);
    await expect(line.locator("[data-ov-briefing-said]")).toHaveText("Main went red at 02:14 and the launch lead reverted it.");
    await expect(line).toHaveRole("button");
    const panel = page.locator("[data-ov-panel]");
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-talk]`).click();
    await expect(panel.locator(`[data-ov-tab="thread:${B}"]`).getByRole("tab")).toHaveAttribute("aria-selected", "true");
    await page.screenshot({ path: info.outputPath("overview-briefing.png") });
    await line.click();
    await expect(panel.locator('[data-ov-tab="staff:dm_cos"]').getByRole("tab")).toHaveAttribute("aria-selected", "true");
    // Hiding the window keeps the head this page showed.
    expect(put).toEqual([]);
    await page.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect.poll(() => put).toEqual([{ seq: 120 }]);
    // Coming back reads it again. Now the reader's inbox asks their review on #7 (and #9, which isn't ready), so #7 counts.
    await page.evaluate(() => {
      const entry = (n: number, reason: string) => ({ item: `fi_${n}`, reason, repo: "o/app", number: n, kind: "pr", state: "open", title: "A PR", author: "jo", url: "", project: null, seen_at: null });
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === "/api/inbox") return new Response(JSON.stringify({ entries: [entry(7, "review_requested"), entry(9, "review_requested"), entry(8, "mentioned")] }), { headers: { "Content-Type": "application/json" } });
        return before(input, init);
      };
      Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect(line.locator("[data-ov-briefing-counts]")).toHaveText(/yesterday: 2 merged · 2 went red · 1 waits on your review$/);
    expect(reads.length).toBe(2);
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
    await openLaunch(page);
    await tile(page, "row_c").hover();
    const hosted = tile(page, "row_c").locator('[data-ov-session="hosted:hs_e2e"]');
    await expect(hosted).toContainText("arugula/13-tidy-docs");
    await expect(hosted.locator("[data-ov-where]")).toHaveText("Arugula cloud");
    await expect(tile(page, "row_c")).toHaveAttribute("data-ov-standing", "working");
    // Its peek: its box is paused, so its last answer (with chat off, its history), and Stop on the platform.
    await page.route("**/api/sessions/hs_e2e", (r) =>
      r.fulfill({ json: { card: { session: "hs_e2e", project: P, room: "room_hs", parent: 1, state: "working", branch: "arugula/13-tidy-docs", machine: null, block: null, parked: false }, may: true, asked: true, now: Date.now(), events: [{ seq: 1, kind: "turn", from: null, to: "working", at: 1, signed: true, text: "Tidied the docs index; pushed." }] } }),
    );
    await hosted.click();
    const peek = page.locator("[data-ov-panel] [data-ov-peek]");
    await expect(peek).toHaveAttribute("data-ov-peek-kind", "hosted");
    await expect(peek.locator("[data-ov-peek-where]")).toHaveText("Arugula cloud · arugula/13-tidy-docs");
    await expect(peek.locator("[data-ov-peek-tail]")).toHaveText("Tidied the docs index; pushed.");
    await expect(peek.locator("[data-ov-peek-tail]")).not.toHaveAttribute("data-ov-peek-live", "");
    await expect(peek.locator("[data-ov-peek-stop]")).toBeVisible();
    // A machine this page doesn't reach: named, on no card; its peek says so and writes nothing.
    const far = page.locator("[data-ov-loose] [data-ov-session]", { hasText: "release" });
    await expect(far.locator("[data-ov-where]")).toHaveText("far-away");
    await far.locator("[data-ov-row-open]").click();
    await expect(peek).toHaveAttribute("data-ov-peek-readonly", "");
    await expect(peek.locator("[data-ov-peek-tail]")).toContainText("far-away");
    await expect(peek.locator("[data-ov-peek-reply], [data-ov-peek-stop]")).toHaveCount(0);
  });
});

test("the team's faces are in the header while chat is on", async ({ page }) => {
  await overview(page);
  await expect(page.locator("[data-project-faces]")).toHaveCount(0);
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate(() => {
    const c = (window as unknown as { __chat: { on: boolean | null; emit(): void } }).__chat;
    c.on = true;
    c.emit();
  });
  const faces = page.locator(`[data-project-home="${P}"] [data-page-head] [data-project-faces]`);
  await expect(faces.locator("[data-project-face]")).toHaveCount(2);
  await expect(faces.locator('[data-project-face="acct_b"]')).toHaveAttribute("title", "Jo");
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
    await openLaunch(page);
    await expect(tile(page, "row_b")).toBeVisible();
    await expect(page.locator("[data-phone-tabs] [data-tab-plan]")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-platform-bar] [data-space-overview]")).toHaveAttribute("aria-current", "page");
    await page.screenshot({ path: info.outputPath("overview-phone.png") });
  });
});
