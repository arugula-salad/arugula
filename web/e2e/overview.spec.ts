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
// (#1277); zooming in place (#1287): a board's row to its Board level, a
// ticket to its Work level, Esc out a level at a time, each with its
// address, the bar's path, the level pill and the talk panel's front tab,
// and a Needs you card to the work that asks; the Work level's panes live
// (#1289), a shell and an agent block typed into and a question answered
// above them; the boards' views read in one request, painted together
// behind a skeleton, with no thread opened by the page, and read one by one
// from a hub that has no such route yet (#1296), the stats line saying
// so until they land, not 0 in flight, and a session's Work address by the
// hub's id (#1380), and its talk tab by that id across a reload (#1390);
// an issue agent's chip at the Board level opening its tab, its issue's
// thread over its peek, posted in, with its rung, who started it and its
// lead's latest word (#1441); coming back to the page
// after 30 seconds reading the boards once and the briefing once, and after
// less reading nothing (#1297); and on a phone, one list
// with Needs you on top. The platform's boards and the
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

/** The platform's boards for the project, and the hub's sessions by card (`null`: the route isn't there yet, 404). The open boards' views come in one read, after `hold` if it's given; `null`: a hub with no such route (404). */
async function serve(page: Page, byCard: unknown = null, live?: number, lead?: () => unknown, hold?: Promise<void> | null) {
  await page.route(`**/api/projects/${P}/boards`, (r) => r.fulfill({ json: { project: P, boards: [board(B, "Launch"), board(LATER, "Later")] } }));
  await page.route(`**/api/projects/${P}/boards/views`, async (r) => {
    if (hold === null) return r.fulfill({ status: 404, json: { error: "not found" } });
    await hold;
    await r.fulfill({ json: [launch(live, lead?.()), view(LATER, "Later", [["row_z", "next", 21, "Someday"]])] });
  });
  await page.route(`**/api/boards/${B}`, (r) => r.fulfill({ json: launch(live, lead?.()) }));
  await page.route(`**/api/boards/${LATER}`, (r) => r.fulfill({ json: view(LATER, "Later", [["row_z", "next", 21, "Someday"]]) }));
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => (byCard ? r.fulfill({ json: byCard }) : r.fulfill({ status: 404, json: { error: "not found" } })));
  await page.route("**/api/issues/iss_*", (r) => r.fulfill({ status: 404, json: { error: "not found" } }));
}

/** The app on the project's Overview, with an agent block on #12 beside the reset's terminal (in its session), and a second session in the project on no card. */
async function overview(page: Page, byCard: unknown = null, live = false, lead?: () => unknown, hold?: Promise<void> | null): Promise<{ term: number; agent: number; scratch: number }> {
  await reset(page);
  const [term] = await panes(page);
  await serve(page, byCard, live ? term : undefined, lead, hold);
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

/** Proposals (Arugula #1278): what the chief of staff asks to do as the reader. */
const START = { id: "prp_start", action: "start", by_session: "ses_cos", issue: { id: "iss_12", number: 12, title: "Fix the installer" }, repo: "acme/app", prompt: null, sessions: [], why: "Nobody works on it and the release waits on it." };
const CLOSE = {
  id: "prp_close",
  action: "close",
  by_session: "ses_cos",
  issue: null,
  repo: null,
  prompt: null,
  sessions: [
    { id: "ses_a", title: "fix-installer", state: "done" },
    { id: "ses_b", title: "tidy-docs", state: "stopped responding" },
  ],
  why: "Both are finished and hold a box each.",
};

/** The reader's staff room, with `proposal`'s message in it (seq 2), and the
 * hub's routes for it: the project's open asks, the ask, and its answer
 * (each body posted in `window.posted`). The fixture page answers chat
 * itself, so it's made up in the page. */
async function proposing(page: Page, proposal: typeof START | typeof CLOSE) {
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate(
    ({ P, proposal }) => {
      const room = { id: "dm_cos", kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: 2, read: 1, unread: 1, mentions: 1, wants: 1, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead: { machine: "m_geek", machine_name: "geek", session: "cos" } };
      const msg = (seq: number, author: string, text: string, more = {}) => ({ seq, at: Date.now() - (3 - seq) * 60_000, author, name: author ? "Sam's agent on geek" : "", text, quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent: author ? { machine: "m_geek", machine_name: "geek" } : null, reactions: [], files: [], pinned: false, also: false, ...more });
      let state = "open";
      const ask = () => ({
        room: "dm_cos", seq: 2, parent: null, kind: "proposal", asker: "acct_a", asker_name: "Sam", question: "", choices: ["Approve", "Deny"], open: state === "open", mine: true, session: null,
        proposal: { ...proposal, state, answered_by_name: state === "open" ? null : "Sam", outcome: state === "approved" ? "Started a session on #12." : null, started: state === "approved" && proposal.action === "start" ? "ses_started" : null },
      });
      const said = () => [
        msg(1, "acct_a", "Morning: two boards moved."),
        msg(2, "", `Your chief of staff asks to start an agent on #12: ${proposal.why}`, { via: `ask:${proposal.id}`, mentions: state === "open" ? ["acct_a"] : [] }),
      ];
      const w = window as unknown as { posted: unknown[] };
      w.posted = [];
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === `/api/projects/${P}/asks`) return json({ asks: state === "open" ? [ask()] : [] });
        if (url.pathname === "/api/chat/rooms/dm_cos/messages") return json({ room, messages: said(), more: false });
        if (url.pathname === "/api/chat/rooms/dm_cos/asks/2") {
          if (init?.method === "POST") {
            const body = JSON.parse(String(init.body)) as { choice: string };
            w.posted.push(body);
            state = body.choice === "Approve" ? "approved" : "denied";
          }
          return json(ask());
        }
        return before(input, init);
      };
      // The room listed: the Overview reads its proposals again, and pins its tab.
      const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
      c.on = true;
      c.rooms = [...c.rooms, room];
      c.emit();
    },
    { P, proposal },
  );
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

/** #12's thread on control (Arugula #1441), led by `lead`, with the board's
 * lead's word in it, and what's posted in it (in `window.saidOn12`); any
 * other thread is left to what was there. */
async function issueRoom(page: Page, lead: { machine: string; machine_name: string; session: string }) {
  await page.evaluate((lead) => {
    const room = { id: "room_i12", kind: "object", team: null, name: "#12", machine: null, machine_name: null, target: "iss_12", project: null, with: null, others: [], private: false, last: 2, read: 2, unread: 0, mentions: 0, wants: 0, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, lead };
    const msg = (seq: number, name: string, text: string, agent: unknown = null) => ({ seq, at: Date.now() - 60_000 + seq, author: `acct_${name}`, name, text, quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent, reactions: [], files: [], pinned: false, also: false });
    const said = [msg(1, "Jo", "The installer fails on Fedora."), msg(2, "Me", "Start with the Linux installer; leave macOS for now.", { machine: lead.machine, machine_name: lead.machine_name, session: "scratch" })];
    (window as unknown as { saidOn12: typeof said }).saidOn12 = said;
    const now = () => ({ ...room, last: said.length, read: said.length });
    const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
    const before = window.fetch;
    window.fetch = async (input, init) => {
      const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
      if (url.pathname === "/api/chat/targets" && (JSON.parse(String(init?.body ?? "{}")) as { target?: string }).target === "iss_12") return json(now());
      if (url.pathname === "/api/chat/rooms/room_i12/messages") {
        if (init?.method === "POST") {
          said.push(msg(said.length + 1, "Me", (JSON.parse(String(init.body)) as { text: string }).text));
          return json({ seq: said.length, mentions: [], unreached: [] });
        }
        const after = Number(url.searchParams.get("after") ?? 0);
        return json({ room: now(), messages: said.filter((m) => m.seq > after), more: false });
      }
      return before(input, init);
    };
  }, lead);
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

/** The threads the page opens (`window.__opened`): the fixture page answers chat inside the page, so no request shows it. */
async function spyOpened(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as { __chat?: { openObject(t: string): Promise<unknown> }; __opened?: string[] };
    let held: typeof w.__chat;
    Object.defineProperty(window, "__chat", {
      configurable: true,
      get: () => held,
      set: (c: NonNullable<typeof w.__chat>) => {
        const open = c.openObject.bind(c);
        c.openObject = (t) => ((w.__opened ??= []).push(t), open(t));
        held = c;
      },
    });
  });
}
const opened = (page: Page) => page.evaluate(() => [...((window as unknown as { __opened?: string[] }).__opened ?? [])].sort());

const tile = (page: Page, row: string) => page.locator(`[data-ov-card="${row}"]`);
/** Open Launch's row, to its cards in flight. */
async function openLaunch(page: Page) {
  await page.locator(`[data-ov-board="${B}"] [data-ov-board-toggle]`).click();
  await expect(page.locator(`[data-ov-board="${B}"]`)).toHaveAttribute("data-ov-open", "");
}

/** Launch's board with a group, "UX11", holding a native ticket and a GitHub item from a long-named repo, as a board opened from Overview draws them. */
async function groupedLaunch(page: Page) {
  const v = view(B, "Launch", [["row_n", "next", 413, "A native ticket"]]);
  const gh = { ...v.zones[0].rows[0], id: "row_g", pointer: { kind: "forge_item", id: "fi_1622" }, live: { state: "open", kind: "issue", repo: "arugula-salad/arugula", number: 1622, title: "A GitHub ticket" } };
  v.zones[0].rows.push(gh);
  const grouped = { ...v, groups: [{ id: "zon_ux11", rev: 1, title: "UX11" }], zones: v.zones.map((z) => ({ ...z, rows: z.rows.map((r) => ({ ...r, group: "zon_ux11" })) })) };
  await page.route(`**/api/boards/${B}`, (r) => r.fulfill({ json: grouped }));
  await page.route(`**/api/projects/${P}/boards/views`, (r) => r.fulfill({ json: [grouped, view(LATER, "Later", [["row_z", "next", 21, "Someday"]])] }));
  await page.goto(`/p/${P}/board/${B}`);
  await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
  return page.locator(`[data-ov-level-board="${B}"]`);
}

/** The board level's boxes don't run into each other: the group's title and count apart, and a GitHub row's pointer short, on one line and clear of the row's second line. */
async function expectSpaced(page: Page) {
  const level = await groupedLaunch(page);
  const head = level.locator('[data-ov-group="UX11"] h3');
  const [title, count] = await Promise.all([head.locator("span").first().boundingBox(), head.locator("span.dim").boundingBox()]);
  expect(count!.x).toBeGreaterThan(title!.x + title!.width);
  const row = level.locator('[data-ov-ticket="row_g"]');
  const num = row.locator("[data-ov-ticket-num]");
  await expect(num).toHaveText("arugula#1622");
  await expect(num).toHaveAttribute("title", "arugula-salad/arugula#1622");
  const [n, line, cell] = await Promise.all([num.boundingBox(), row.locator("[data-ov-ticket-pr]").boundingBox(), row.locator(".ov-bl-what").boundingBox()]);
  expect(n!.height).toBeLessThan(24);
  expect(n!.x + n!.width).toBeLessThanOrEqual(cell!.x);
  expect(n!.x + n!.width <= line!.x || n!.y + n!.height <= line!.y || line!.y + line!.height <= n!.y).toBe(true);
}

type AskSpec = { host: string; person: { id: string; name: string; kind: "me" | "person" | "team" }; owner?: string; ask: string; watch?: boolean };
/** Asks of made-up panes on the Overview's project, beside the daemon's own: the fleet takes them as it takes a fake one. A `watch` one is a session you only view. */
async function asking(page: Page, specs: AskSpec[]) {
  await page.evaluate(
    ({ specs, P }) => {
      const fleet = (window as unknown as { __app: { fleet: { inject(p: unknown[]): void; role(p: { host: string }): string } } }).__app.fleet;
      const watched = new Set(specs.filter((s) => s.watch).map((s) => s.host));
      const role = fleet.role.bind(fleet);
      fleet.role = (p) => (watched.has(p.host) ? "viewer" : role(p));
      fleet.inject(
        specs.map((s, i) => ({
          key: `${s.host}:${i + 1}`,
          host: s.host,
          id: i + 1,
          session: { id: 1, name: "main" },
          stale: false,
          owner: s.owner,
          person: s.person,
          driver: null,
          watchers: [],
          unread: 0,
          mention: false,
          info: {
            id: i + 1,
            title: "claude",
            kind: "agent",
            attention: "needs_input",
            session_project: { id: P, name: "Arugula" },
            activity: { bps: 0, last_ms: Date.now() },
            ask: { kind: "permission", id: `perm_${i}`, tool: "Bash", input: { command: s.ask }, message: "", suggestions: [] },
            reason: { kind: "ask", since_ms: 1000 + i, headline: `Bash wants to run ${s.ask}`, bundle: `ask:${P}:${i}`, ask: { id: `perm_${i}`, what: "approve", agent: "claude", tool: "Bash" }, actions: ["allow", "deny"] },
          },
        })),
      );
    },
    { specs, P },
  );
}
const ME = { id: "me", name: "me", kind: "me" as const };
const SAM = { id: "account:sam", name: "sam", kind: "person" as const };
const JO = { id: "account:jo", name: "jo", kind: "person" as const };
const INFRA = { id: "team:infra", name: "infra", kind: "team" as const };

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the right column is a third of the page, and a divider resizes it, by drag and by key, and is remembered (Arugula #1444)", async ({ page }) => {
    await overview(page);
    const panel = page.locator("[data-ov-panel]");
    const split = page.locator("[data-ov-split]");
    await expect(split).toHaveAttribute("aria-orientation", "vertical");
    // The sidebar and page padding take some of the window: compare with the Overview's own width.
    const ovShare = async () => (await panel.evaluate((e) => e.getBoundingClientRect().width)) / (await page.locator("[data-overview]").evaluate((e) => e.getBoundingClientRect().width));
    expect(Math.abs((await ovShare()) - 1 / 3)).toBeLessThan(0.02);
    const before = await ovShare();
    const box = (await split.boundingBox())!;
    const y = box.y + 40;
    await page.mouse.move(box.x + box.width / 2, y);
    await page.mouse.down();
    await page.mouse.move(box.x - 120, y, { steps: 5 });
    await page.mouse.up();
    const dragged = await ovShare();
    expect(dragged).toBeGreaterThan(before + 0.05);
    // It survives a reload.
    await page.reload();
    await expect(panel).toBeVisible();
    expect(Math.abs((await ovShare()) - dragged)).toBeLessThan(0.01);
    // The keyboard moves it: Right narrows the column.
    await split.focus();
    await page.keyboard.press("ArrowRight");
    expect(await ovShare()).toBeLessThan(dragged - 0.01);
    // A double-click goes back to a third.
    await split.dblclick();
    await expect.poll(async () => Math.abs((await ovShare()) - 1 / 3)).toBeLessThan(0.02);
  });

  test("a board's group title and count are apart, and a GitHub pointer is short and on one line", async ({ page }) => {
    await overview(page);
    await expectSpaced(page);
  });

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
    // Nothing asks you, so Launch is closed; its chevron opens it in place, and its title zooms to it (#1287).
    await expect(launch.locator("[data-ov-card]")).toHaveCount(0);
    await expect(launch.locator("[data-ov-board-title]")).toHaveAttribute("href", new RegExp(`/board/${B}$`));
    await launch.locator("[data-ov-board-toggle]").click();
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
    const order = await page.locator("[data-ov-field] [data-ov-stage] > *").evaluateAll((els) => els.map((e) => (e.hasAttribute("data-ov-boards") ? "boards" : e.hasAttribute("data-ov-loose") ? "loose" : "")).filter(Boolean));
    expect(order).toEqual(["boards", "loose"]);
    // None of the old home's lists: no chat, no people, no issues, runs, repos or workspaces.
    await expect(page.locator("[data-project-rooms], [data-chat-off], [data-project-members], [data-project-runs], [data-project-repos], [data-project-envs], [data-project-boards], [data-ov-more]")).toHaveCount(0);
    // Chat is off here: no faces in the header.
    await expect(page.locator("[data-project-faces]")).toHaveCount(0);
    await expect(page.locator(".ws-rail [data-open-overview]")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-place-views=overview] [data-view-overview]")).toHaveAttribute("aria-current", "page");
    await page.screenshot({ path: info.outputPath("overview.png") });
  });

  test("the boards' views are one read, painted together behind a skeleton, and the page opens no thread (Arugula #1296)", async ({ page }) => {
    const asked: string[] = [];
    page.on("request", (r) => {
      const u = new URL(r.url());
      if (u.pathname.startsWith("/api/") && r.method() !== "OPTIONS") asked.push(`${r.method()} ${u.pathname}`);
    });
    await spyOpened(page);
    let release!: () => void;
    const hold = new Promise<void>((done) => (release = done));
    await overview(page, null, false, undefined, hold);
    // The hub hasn't answered: a still skeleton, no row, no "Nothing in flight".
    await expect(page.locator("[data-ov-skeleton]")).toBeVisible();
    await expect(page.locator("[data-ov-board], [data-ov-empty]")).toHaveCount(0);
    release();
    await expect(page.locator("[data-ov-board]")).toHaveCount(2);
    await expect(page.locator("[data-ov-skeleton]")).toHaveCount(0);
    await expect(page.locator(`[data-ov-board="${B}"] [data-ov-board-count]`)).toHaveText("1/5");
    await expect(page.locator(`[data-ov-board="${LATER}"] [data-ov-board-count]`)).toHaveText("0/1");
    const mine = (path: RegExp) => asked.filter((a) => path.test(a));
    expect(mine(/^GET \/api\/projects\/[^/]+\/boards\/views$/)).toHaveLength(1);
    expect(mine(/^GET \/api\/boards\//)).toHaveLength(0);
    expect(await opened(page)).toEqual([]);
  });

  test("until the boards' views land the stats line says it's reading them, never 0 in flight (Arugula #1380)", async ({ page }) => {
    let release!: () => void;
    const hold = new Promise<void>((done) => (release = done));
    await overview(page, null, false, undefined, hold);
    const stats = page.locator("[data-ov-stats]");
    await expect(page.locator("[data-ov-skeleton]")).toBeVisible();
    await expect(stats).toHaveText("Reading the boards…");
    await expect(stats).toHaveAttribute("aria-busy", "true");
    release();
    // #12 and #13 in Doing, #14 in Review.
    await expect(stats).toHaveText(/^3 cards in flight · /);
    await expect(stats).not.toHaveAttribute("aria-busy");
  });

  test("a hub without the views route (404) is read board by board, each thread opened, the rows still painted together (Arugula #1296)", async ({ page }) => {
    const asked: string[] = [];
    page.on("request", (r) => {
      const u = new URL(r.url());
      if (u.pathname.startsWith("/api/")) asked.push(`${r.method()} ${u.pathname}`);
    });
    await spyOpened(page);
    await overview(page, null, false, undefined, null);
    await expect(page.locator("[data-ov-board]")).toHaveCount(2);
    await expect(page.locator(`[data-ov-board="${B}"] [data-ov-board-count]`)).toHaveText("1/5");
    expect(asked.filter((a) => a === `GET /api/boards/${B}` || a === `GET /api/boards/${LATER}`).sort()).toEqual([`GET /api/boards/${B}`, `GET /api/boards/${LATER}`].sort());
    expect(await opened(page)).toEqual([B, LATER].sort());
  });

  test("coming back after a short absence reads nothing; after a long one, one read of the boards and one of the briefing (Arugula #1297)", async ({ page }) => {
    const asked: string[] = [];
    page.on("request", (r) => {
      const u = new URL(r.url());
      if (u.pathname.startsWith("/api/") && r.method() === "GET") asked.push(u.pathname);
    });
    await overview(page);
    await expect(page.locator("[data-ov-board]")).toHaveCount(2);
    const reads = () => ({
      views: asked.filter((a) => /^\/api\/projects\/[^/]+\/boards\/views$/.test(a)).length,
      boards: asked.filter((a) => a.startsWith("/api/boards/")).length,
      briefing: asked.filter((a) => a.endsWith("/overview/seen")).length,
    });
    await expect.poll(() => reads().briefing).toBe(1);
    expect(reads()).toEqual({ views: 1, boards: 0, briefing: 1 });
    // The page's clock runs ahead by what the test says: no wait.
    await page.evaluate(() => {
      const w = window as unknown as { __skew: number };
      w.__skew = 0;
      const now = Date.now.bind(Date);
      Date.now = () => now() + w.__skew;
    });
    const show = (state: "hidden" | "visible") =>
      page.evaluate((state) => {
        Object.defineProperty(document, "visibilityState", { value: state, configurable: true });
        document.dispatchEvent(new Event("visibilitychange"));
      }, state);
    const away = async (ms: number) => {
      await show("hidden");
      await page.evaluate((ms) => ((window as unknown as { __skew: number }).__skew += ms), ms);
      await show("visible");
    };
    // Out for 29 s: the live pushes kept running, so nothing is read.
    await away(29_000);
    // A read after it would have been asked by now: the page answers a round trip first.
    await page.evaluate(() => fetch("/api/projects").catch(() => {}));
    expect(reads()).toEqual({ views: 1, boards: 0, briefing: 1 });
    // Out for 30 s: one read of every board, and the briefing's.
    await away(30_000);
    await expect.poll(() => reads()).toEqual({ views: 2, boards: 0, briefing: 2 });
    await page.evaluate(() => fetch("/api/projects").catch(() => {}));
    expect(reads()).toEqual({ views: 2, boards: 0, briefing: 2 });
  });

  test("a card opens the board's drawer; Esc and the path climb back; a session opens on its machine", async ({ page }, info) => {
    const { agent } = await overview(page);
    // The agent's session (the reset's) in the project, as one made on its machine page is: placed in
    // none (the suite's daemon has no control to place it), it's the reader's Default, and the machine's
    // page moves to that project the moment it knows (#1161, decision 3), after the address said this one.
    type Client = { intent(i: unknown): void; sessionOfPane(p: number): number | null; state: { sessions: { id: number; project?: { id: string } | null }[] } };
    const session = await page.evaluate((agent) => (window as unknown as { __app: { client: Client } }).__app.client.sessionOfPane(agent)!, agent);
    await page.evaluate(({ session, P }) => (window as unknown as { __app: { client: Client } }).__app.client.intent({ op: "set_session_project", session, project: { id: P, name: "Arugula" } }), { session, P });
    await expect.poll(() => page.evaluate((session) => (window as unknown as { __app: { client: Client } }).__app.client.state.sessions.find((s) => s.id === session)?.project?.id, session)).toBe(P);
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
    // The rail's Overview, back.
    await page.locator(".ws-rail [data-open-overview]").click();
    await expect(page.locator("[data-overview]")).toBeVisible();
  });

  test("a question in a card's session pulses its row, and is answered in place on Needs you", async ({ page }, info) => {
    const { term } = await overview(page);
    const needs = page.locator("[data-ov-needs]");
    // The agent finishing may put a Finished card there (Swarm's rail does); nothing asks yet.
    await expect(needs.locator('[data-ov-ask][data-kind="ask"]')).toHaveCount(0);
    // Claude Code's question, asked in the agent's terminal.
    await page.evaluate(
      ({ term, line }) => fetch(`/api/panes/${term}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: line, enter: true }) }),
      { term, line: `out=$(${cli} ask < ${hookInput}); echo "ONE[\${#out}]"; echo "$out" | grep -o '"Which [a-z ]*?":"[^"]*"'` },
    );
    const row = needs.locator('[data-ov-ask][data-kind="ask"]');
    await expect(row).toHaveCount(1, { timeout: 20_000 });
    // A row is two lines until it's opened: its card is the row's body.
    const card = row.locator(".swarm-card");
    await expect(card).toHaveCount(0);
    await row.locator(".ov-ask-row").click();
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
    // Its title zooms to the work that asks: #12's (#1287), and Back comes home.
    await card.locator("[data-card-work]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/board/${B}[^/]*/card/row_b$`));
    await expect(page.locator('[data-ov-level-work="row_b"]')).toBeVisible();
    await page.goBack();
    await expect(page.locator(`[data-ov-board="${B}"]`)).toBeVisible();
    const q = (n: number) => card.locator(`[data-question="${n}"]`);
    await q(0).getByRole("radio", { name: /^Blue/ }).click();
    await q(1).getByRole("checkbox", { name: /Pear/ }).check();
    await q(2).getByRole("radio", { name: /^Cat/ }).click();
    await card.getByRole("button", { name: "Submit" }).click();
    await expect(tile(page, "row_b")).not.toHaveAttribute("data-ov-standing", "needs", { timeout: 20_000 });
    await expect.poll(() => page.evaluate((t) => (window as unknown as { __app: { text(p: number): string } }).__app.text(t), term)).toContain('"Which colour do you prefer?":"Blue"');
  });

  test("Needs you groups the asks by owner: yours first, then each person with a count, a team's box under its starter else its name (Arugula #1333)", async ({ page }) => {
    await overview(page);
    await asking(page, [
      { host: "jo-box", person: JO, ask: "ls jo" },
      { host: "sam-box", person: SAM, ask: "ls sam 1" },
      { host: "sam-box-2", person: SAM, ask: "ls sam 2" },
      { host: "build-1", person: INFRA, owner: "sam", ask: "make" },
      { host: "build-2", person: INFRA, ask: "make all" },
      { host: "mine", person: ME, ask: "ls mine" },
    ]);
    // The daemon's own agent may add a Finished row of yours: yours is counted by the asks only.
    const heads = page.locator("[data-ov-needs] [data-ov-asks-head]");
    await expect(heads.locator("> span:first-child")).toHaveText(["Yours", "sam", "infra", "jo"]);
    await expect(heads.locator(".ov-asks-n")).toContainText(["", "3", "1", "1"]);
    await expect(page.locator('[data-ov-asks="me"] [data-ov-ask][data-kind="ask"]')).toHaveCount(1);
    await expect(page.locator('[data-ov-asks="sam"] [data-ov-ask]')).toHaveCount(3);
  });

  test("with every ask yours, Needs you has no heading", async ({ page }) => {
    await overview(page);
    await asking(page, [{ host: "mine", person: ME, ask: "ls mine" }]);
    // The daemon's own agent may add a Finished row of yours, as above: count the asks.
    await expect(page.locator('[data-ov-needs] [data-ov-ask][data-kind="ask"]')).toHaveCount(1);
    await expect(page.locator("[data-ov-needs] [data-ov-asks-head]")).toHaveCount(0);
  });

  test("an ask is two lines until it's opened by a click or Enter, and its action exists only while it's open", async ({ page }) => {
    await overview(page);
    await asking(page, [{ host: "mine", person: ME, ask: "npm test" }]);
    const row = page.locator('[data-ov-needs] [data-ov-ask][data-kind="ask"]');
    const head = row.locator(".ov-ask-row");
    await expect(head).toHaveAttribute("aria-expanded", "false");
    await expect(head.locator(".ov-ask-q")).toContainText("npm test");
    await expect(head.locator(".ov-ask-where")).toBeVisible();
    const allow = row.getByRole("button", { name: "Allow", exact: true });
    await expect(allow).toHaveCount(0);
    await head.click();
    await expect(head).toHaveAttribute("aria-expanded", "true");
    await expect(allow).toBeVisible();
    await head.click();
    await expect(allow).toHaveCount(0);
    // The keyboard: focus it, Enter opens, Space closes, Enter and Escape.
    await head.focus();
    await page.keyboard.press("Enter");
    await expect(allow).toBeVisible();
    await page.keyboard.press("Space");
    await expect(allow).toHaveCount(0);
    await page.keyboard.press("Enter");
    await expect(allow).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(allow).toHaveCount(0);
  });

  test("the gap between Needs you's rows belongs to them: the pointer is over a row all the way down, and an open row stays open (Arugula #1427)", async ({ page }) => {
    await overview(page);
    await asking(page, [
      { host: "one", person: ME, ask: "ls one" },
      { host: "two", person: ME, ask: "ls two" },
      { host: "three", person: ME, ask: "ls three" },
    ]);
    const rows = page.locator('[data-ov-needs] [data-ov-ask][data-kind="ask"]');
    await expect(rows).toHaveCount(3);
    // Where a row shows: its box less a transparent border (the half-gap that is its hit area).
    const shown = (i: number) =>
      rows.nth(i).evaluate((e) => {
        const r = e.getBoundingClientRect();
        const cs = getComputedStyle(e);
        const clear = (c: string) => c === "rgba(0, 0, 0, 0)" || c === "transparent";
        const top = clear(cs.borderTopColor) ? parseFloat(cs.borderTopWidth) : 0;
        const bottom = clear(cs.borderBottomColor) ? parseFloat(cs.borderBottomWidth) : 0;
        return { x: r.x + r.width / 2, top: r.top + top, bottom: r.bottom - bottom, boxTop: r.top, boxBottom: r.bottom };
      });
    // Hover the first: it opens (and the rows below it move down), so measure after.
    const first = await shown(0);
    await page.mouse.move(first.x, (first.top + first.bottom) / 2);
    await expect(rows.nth(0).locator(".ov-ask-row")).toHaveAttribute("aria-expanded", "true");
    const [a, b] = [await shown(0), await shown(1)];
    // The visible spacing is the 8px it was.
    expect(b.top - a.bottom).toBe(8);
    // Every pixel from the first's box to the second's is over some row.
    const overRow = await page.evaluate(
      ({ x, from, to }) => {
        const out: number[] = [];
        for (let y = Math.ceil(from); y < to; y++) if (!document.elementFromPoint(x, y)?.closest("[data-ov-ask]")) out.push(y);
        return out;
      },
      { x: a.x, from: a.boxBottom, to: b.boxTop + 1 },
    );
    expect(overRow).toEqual([]);
    // Just below its visible bottom, in the gap, it stays open.
    await page.mouse.move(a.x, a.bottom + 1);
    await page.waitForTimeout(150);
    await expect(rows.nth(0).locator(".ov-ask-row")).toHaveAttribute("aria-expanded", "true");
  });

  test("a teammate's ask you only watch says an editor answers it, and has no action", async ({ page }) => {
    await overview(page);
    await asking(page, [{ host: "sam-box", person: SAM, ask: "ls sam", watch: true }]);
    const row = page.locator('[data-ov-needs] [data-ov-ask][data-kind="ask"]');
    await row.locator(".ov-ask-row").click();
    await expect(row.locator(".ask-viewer")).toHaveText("You're watching this session: an editor answers it.");
    await expect(row.getByRole("button", { name: /^(Allow|Deny|Answer)/ })).toHaveCount(0);
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

  test("with no ask or one, the page keeps its height and only the open thread's list scrolls (Arugula #1334)", async ({ page }) => {
    await overview(page);
    await launchRoom(page);
    await chatOn(page);
    await page.evaluate(() => {
      const said = (window as unknown as { said: { seq: number; at: number; author: string; name: string; text: string; quote: null; pane: null; mentions: never[]; edited: null; deleted: null; imported: null; agent: null; reactions: never[]; files: never[]; pinned: boolean; also: boolean }[] }).said;
      for (let seq = 2; seq <= 60; seq++) said.push({ seq, at: Date.now() - 60_000 + seq, author: "acct_Jo", name: "Jo", text: `Message ${seq} of a long thread.`, quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent: null, reactions: [], files: [], pinned: false, also: false });
    });
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-talk]`).click();
    const list = page.locator(`[data-ov-talk-open="thread:${B}"] .thread-list`);
    await expect(list).toContainText("Message 60 of a long thread.");
    await expect(page.locator("[data-ov-needs]")).toBeVisible();
    // Nothing above the list scrolls: not the document, nor any container the Overview sits in.
    const still = () =>
      page.evaluate(() => {
        const root = document.scrollingElement!;
        const scrolls = [root.scrollHeight > root.clientHeight];
        for (let e = document.querySelector("[data-overview]") as HTMLElement | null; e; e = e.parentElement) scrolls.push(e.scrollHeight > e.clientHeight && getComputedStyle(e).overflowY !== "visible");
        return { scrolls: scrolls.some(Boolean), panel: document.querySelector("[data-ov-panel]")!.getBoundingClientRect().height };
      });
    const empty = await still();
    expect(empty.scrolls).toBe(false);
    const scrollable = async () => {
      const [over, moved] = await list.evaluate((e) => {
        e.scrollTop = 40;
        return [e.scrollHeight > e.clientHeight, e.scrollTop > 0];
      });
      expect(over && moved).toBe(true);
    };
    await scrollable();
    // One ask: the same, and the panel as tall as with none.
    await asking(page, [{ host: "mine", person: ME, ask: "ls mine" }]);
    await expect(page.locator('[data-ov-needs] [data-ov-ask][data-kind="ask"]')).toHaveCount(1);
    const one = await still();
    expect(one.scrolls).toBe(false);
    expect(one.panel).toBe(empty.panel);
    await scrollable();
  });

  test("with no ask and a long thread open, Needs you keeps its height above the tabs (https://app.arugula.io/issues/iss_2ec9ffb4a17f1be76e3d41d30238c775)", async ({ page }) => {
    await overview(page);
    await launchRoom(page);
    await chatOn(page);
    await page.evaluate(() => {
      const said = (window as unknown as { said: { seq: number; at: number; author: string; name: string; text: string; quote: null; pane: null; mentions: never[]; edited: null; deleted: null; imported: null; agent: null; reactions: never[]; files: never[]; pinned: boolean; also: boolean }[] }).said;
      for (let seq = 2; seq <= 60; seq++) said.push({ seq, at: Date.now() - 60_000 + seq, author: "acct_Jo", name: "Jo", text: `Message ${seq} of a long thread.`, quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent: null, reactions: [], files: [], pinned: false, also: false });
    });
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-talk]`).click();
    await expect(page.locator(`[data-ov-talk-open="thread:${B}"] .thread-list`)).toContainText("Message 60 of a long thread.");
    // No ask, only a finished session: Needs you's heading and that row.
    const needs = page.locator("[data-ov-panel] > [data-ov-needs]");
    await expect(needs.getByRole("heading", { name: "Needs you" })).toBeVisible();
    await expect(needs.locator('[data-ov-ask][data-kind="ask"]')).toHaveCount(0);
    // It isn't squeezed into a box shorter than itself, and neither its heading nor its row reaches the tabs.
    const [over, head, row, tabs] = await Promise.all([needs.evaluate((e) => e.scrollHeight - e.clientHeight), needs.locator("header").boundingBox(), needs.locator("[data-ov-ask]").last().boundingBox(), page.locator("[data-ov-tabs]").boundingBox()]);
    expect(over).toBeLessThanOrEqual(0);
    expect(head!.y + head!.height).toBeLessThanOrEqual(tabs!.y);
    expect(row!.y + row!.height).toBeLessThanOrEqual(tabs!.y);
  });

  test("the reader's staff room is the talk panel's first tab, pinned, its head saying where its chief of staff runs and whether it's awake (Arugula #1275, #1342)", async ({ page }) => {
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
    // Its head (#1342): a machine this page doesn't reach tells only where it runs.
    const head = panel.locator('[data-ov-who="staff"] [data-agent-head]');
    await expect(head).toHaveText("Chief of staff · geek");
    await expect(head).toHaveAttribute("data-agent-state", "unknown");
    // Led by the project's scratch session on the suite's daemon, which this page reads: idle there, its shell quiet.
    const host = (await page.locator("[data-ov-loose] [data-ov-panes]").getAttribute("data-ov-panes"))!.split(":")[0];
    await page.evaluate((host) => {
      const c = (window as unknown as { __chat: { rooms: { id: string; lead?: unknown }[]; emit(): void } }).__chat;
      c.rooms = c.rooms.map((r) => (r.id === "dm_cos" ? { ...r, lead: { machine: "m_suite", machine_name: host, session: "scratch" } } : r));
      c.emit();
    }, host);
    await expect(head).toHaveText(`Chief of staff · idle on ${host}`);
    await expect(head.locator("[title]")).toHaveAttribute("title", "wakes on its own");
    // No session leads it: none.
    await page.evaluate(() => {
      const c = (window as unknown as { __chat: { rooms: { id: string; lead?: unknown }[]; emit(): void } }).__chat;
      c.rooms = c.rooms.map((r) => (r.id === "dm_cos" ? { ...r, lead: undefined } : r));
      c.emit();
    });
    await expect(head).toHaveText("Chief of staff · none");
  });

  test("an agent's proposal to start one is under Needs you, counted; Approve sends it and it leaves (Arugula #1278)", async ({ page }) => {
    await overview(page);
    await proposing(page, START);
    const needs = page.locator("[data-ov-panel] > [data-ov-needs]");
    const card = needs.locator('[data-proposal="prp_start"]');
    await expect(card).toHaveAttribute("data-proposal-action", "start");
    await expect(card).toHaveAttribute("data-ask-state", "open");
    await expect(card.locator("[data-ask-label]")).toHaveText("asks you");
    await expect(card).toContainText("Chief of staff · start an agent");
    await expect(card.locator("[data-proposal-issue] a")).toHaveText("#12 Fix the installer");
    await expect(card.locator("[data-proposal-repo]")).toHaveText("in acme/app");
    await expect(card.locator("[data-proposal-why]")).toContainText("Nobody works on it and the release waits on it.");
    await expect(needs.locator("[data-ov-needs-count]")).toHaveText("1");
    await expect(needs.locator("[data-ov-needs-none]")).toHaveCount(0);
    await card.locator("[data-proposal-approve]").click();
    await expect.poll(() => page.evaluate(() => (window as unknown as { posted: unknown[] }).posted)).toEqual([{ choice: "Approve" }]);
    await expect(card).toHaveCount(0);
    await expect(page.locator("[data-platform-toast]")).toContainText("Sam approved: Started a session on #12.");
    await expect(needs.locator("[data-ov-needs-count]")).toHaveCount(0);
  });

  test("an agent's proposal to stop sessions lists them; Deny sends it and it leaves (Arugula #1278)", async ({ page }) => {
    await overview(page);
    await proposing(page, CLOSE);
    const card = page.locator('[data-ov-needs] [data-proposal="prp_close"]');
    await expect(card).toHaveAttribute("data-proposal-action", "close");
    await expect(card).toContainText("Chief of staff · stop sessions");
    await expect(card.locator("[data-proposal-session]")).toHaveText(["fix-installer · done", "tidy-docs · stopped responding"]);
    await card.locator("[data-proposal-deny]").click();
    await expect.poll(() => page.evaluate(() => (window as unknown as { posted: unknown[] }).posted)).toEqual([{ choice: "Deny" }]);
    await expect(card).toHaveCount(0);
    await expect(page.locator("[data-ov-needs-count]")).toHaveCount(0);
  });

  test("a proposal's message in the staff room is its card in the staff tab; answered, a line (Arugula #1278)", async ({ page }) => {
    await overview(page);
    await proposing(page, START);
    const staff = page.locator('[data-ov-staff="dm_cos"]');
    const card = staff.locator('[data-proposal="prp_start"]');
    await expect(card).toHaveAttribute("data-ask", "2");
    await expect(card.locator("[data-proposal-approve]")).toBeVisible();
    // Its plain text is only the fallback.
    await expect(staff).not.toContainText("Your chief of staff asks to start an agent");
    // Its where opens this tab, from Needs you too.
    await page.locator('[data-ov-needs] [data-proposal="prp_start"] [data-proposal-open]').click();
    await expect(page.locator('[data-ov-tab="staff:dm_cos"] [role="tab"]')).toHaveAttribute("aria-selected", "true");
    await card.locator("[data-proposal-approve]").click();
    const done = staff.locator('[data-proposal="prp_start"][data-ask-state="answered"]');
    await expect(done).toHaveText("Sam approved: Started a session on #12. Its session");
    await expect(done.locator("[data-proposal-started]")).toHaveAttribute("href", /ses_started/);
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
    // It's on #12 for Launch: its who-line is its head, with its rung (#1441); with chat off, its tab is its peek alone.
    await expect(panel.locator('[data-ov-who="peek"] [data-agent-head="Agent"]')).toBeVisible();
    await expect(panel.locator('[data-ov-who="peek"] [data-agent-rung]')).toHaveText("on #12 for Launch · started by you");
    await expect(panel.locator("[data-ov-agent-tab]")).toHaveCount(0);
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

  test("zooming: a board's row to its Board level, a ticket to its Work level, Esc out twice; the address, the path, the pill and the front tab right at each (Arugula #1287)", async ({ page }, info) => {
    // Launch is led by the project's scratch session, so its lead's tab is that session's.
    let led: unknown;
    await overview(page, null, true, () => led);
    const loose = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(loose).toHaveAttribute("data-ov-panes", /:/);
    led = { machine: "m_suite", machine_name: (await loose.getAttribute("data-ov-panes"))!.split(":")[0], session: "scratch", at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    const pill = (l: string) => page.locator(`[data-ov-level-pill="${l}"]`);
    const front = page.locator("[data-ov-tabs] .ov-tab-on");
    // The lead's tab is its session's peek, by its key past the card's.
    const lead = `peek:${(await page.locator("[data-ov-loose] [data-ov-session]").getAttribute("data-ov-session"))!.split("|")[1]}`;
    await expect(pill("project")).toHaveAttribute("aria-current", "true");
    await expect(page.locator("[data-ov-zoom-bar] .ov-zoom-hint")).toHaveCount(0);

    // In: the row's title, to the board, its open tickets a row each (Done folded).
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-title]`).click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/board/${B}[^/]*$`));
    await expect(page.locator(`[data-ov-level-board="${B}"]`)).toBeVisible();
    await expect(page.locator("[data-ov-boards]")).toHaveCount(0);
    await expect(page.locator("[data-ov-ticket]")).toHaveCount(4);
    await expect(pill("board")).toHaveAttribute("aria-current", "true");
    await expect(pill("work")).toBeDisabled();
    await expect(page.locator('[data-app-path] [data-path="board"]')).toHaveText("Launch");
    await expect(front).toHaveAttribute("data-ov-tab", lead);
    await page.waitForTimeout(300);
    await page.screenshot({ path: info.outputPath("overview-zoom-board.png") });

    // In again: #12, its sessions, its agent's tab in front and its row outlined.
    await page.locator('[data-ov-ticket="row_b"]').click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/board/${B}[^/]*/card/row_b$`));
    const work = page.locator('[data-ov-level-work="row_b"]');
    await expect(work).toBeVisible();
    await expect(work.locator("h2")).toHaveText("#12 Fix the installer");
    await expect(page.locator('[data-app-path] [data-path="card"]')).toHaveText("#12 · Fix the installer");
    await expect(pill("work")).toHaveAttribute("aria-current", "true");
    await expect(work.locator("[data-ov-work-session]")).not.toHaveCount(0);
    await expect(work.locator("[data-ov-talking]")).toHaveCount(1);
    await expect(front).toHaveAttribute("data-ov-tab", /^peek:/);
    await expect(front).not.toHaveAttribute("data-ov-tab", lead);
    // The lead's tab is still open beside it.
    await expect(page.locator(`[data-ov-tab="${lead}"]`)).toHaveCount(1);
    await page.waitForTimeout(300);
    await page.screenshot({ path: info.outputPath("overview-zoom-work.png") });
    // A reload lands on it.
    await page.reload();
    await expect(work).toBeVisible({ timeout: 20_000 });

    // Out: Esc to the board, the lead's tab in front again; Esc to the project.
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/board/${B}[^/]*$`));
    await expect(front).toHaveAttribute("data-ov-tab", lead);
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/?]*$`));
    await expect(page.locator(`[data-ov-board="${B}"]`)).toBeVisible();
    await expect(pill("project")).toHaveAttribute("aria-current", "true");
    // Back goes in again, and the pill comes out.
    await page.goBack();
    await expect(page.locator(`[data-ov-level-board="${B}"]`)).toBeVisible();
    await pill("project").click();
    await expect(page.locator("[data-ov-boards]")).toBeVisible();
  });

  test("an issue agent's chip at the Board level opens its tab: its issue's thread over its peek, a post there that prompts it, its rung, who started it and its lead's latest word; the Work level brings the same tab (Arugula #1441)", async ({ page }, info) => {
    // Launch is led by the project's scratch session; #12's thread is led by its agent block, by its session's name, as control names it.
    let led: unknown;
    const { agent } = await overview(page, null, false, () => led);
    const loose = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(loose).toHaveAttribute("data-ov-panes", /:/);
    const host = (await loose.getAttribute("data-ov-panes"))!.split(":")[0];
    const session = await page.evaluate(async (id) => ((await (await fetch("/api/panes")).json()) as { id: number; session_name: string }[]).find((p) => p.id === id)!.session_name, agent);
    led = { machine: "m_suite", machine_name: host, session: "scratch", at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    await issueRoom(page, { machine: "m_suite", machine_name: host, session });
    await chatOn(page);

    // The Board level: #12's agent is a chip on its ticket.
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-title]`).click();
    await expect(page.locator(`[data-ov-level-board="${B}"]`)).toBeVisible();
    const panel = page.locator("[data-ov-panel]");
    // The lead's tab is in front: its head says it leads Launch.
    await expect(panel.locator('[data-ov-who="peek"] [data-agent-rung]')).toHaveText("lead of Launch · started by you");
    const chip = page.locator('[data-ov-ticket="row_b"] [data-ov-agent-chip]');
    await expect(chip).toHaveCount(1);
    await chip.click();
    const front = panel.locator("[data-ov-tabs] .ov-tab-on");
    await expect(front).toHaveAttribute("data-ov-tab", /^peek:/);
    const key = (await front.getAttribute("data-ov-tab"))!;
    // Its tab: #12's thread, which its agent leads, over its peek, which has no box of its own.
    const tab = panel.locator('[data-ov-agent-tab="iss_12"]');
    await expect(tab).toHaveAttribute("data-ov-agent-hears", "");
    const thread = tab.locator('[data-ov-issue-thread="room_i12"]');
    await expect(thread).toContainText("The installer fails on Fedora.");
    await expect(tab.locator("[data-ov-peek]")).toHaveAttribute("data-ov-peek-kind", "block");
    await expect(tab.locator("[data-ov-peek-tail]")).toHaveAttribute("data-ov-peek-live", "");
    await expect(tab.locator("[data-ov-peek-reply]")).toHaveCount(0);
    await expect(tab.locator("[data-ov-agent-note]")).toHaveCount(0);
    // Its head: where it runs, its rung and who started it, and its lead's latest word to it.
    const who = panel.locator('[data-ov-who="peek"]');
    await expect(who.locator("[data-agent-head]")).toContainText(host);
    await expect(who.locator("[data-agent-rung]")).toHaveText("on #12 for Launch · started by you");
    await expect(who.locator("[data-agent-word]")).toHaveText("Last from its lead, scratch: “Start with the Linux installer; leave macOS for now.”");
    // A post in its thread, which prompts it.
    const box = thread.locator(".thread-compose textarea");
    await box.fill("Check the arm64 build too.");
    await box.press("Enter");
    await expect(thread).toContainText("Check the arm64 build too.");
    expect(await page.evaluate(() => (window as unknown as { saidOn12: { text: string }[] }).saidOn12.map((m) => m.text))).toContain("Check the arm64 build too.");
    await page.waitForTimeout(300);
    await page.screenshot({ path: info.outputPath("overview-issue-agent.png") });

    // The Work level brings the same tab to the front, and its pane is the one outlined.
    await page.locator(`[data-ov-tab="${key}"] [data-ov-tab-close]`).click();
    await page.locator('[data-ov-ticket="row_b"] [data-ov-ticket-title]').click();
    await expect(page.locator('[data-ov-level-work="row_b"]')).toBeVisible();
    await expect(front).toHaveAttribute("data-ov-tab", key);
    await expect(panel.locator('[data-ov-agent-tab="iss_12"]')).toBeVisible();
    await expect(page.locator('[data-ov-level-work="row_b"] [data-ov-work-pane]').first()).toHaveAttribute("data-ov-work-kind", "block");

    // A thread another session leads says so, and the peek keeps its box.
    await page.evaluate(() => {
      const c = (window as unknown as { __chat: { rooms: { id: string; lead?: unknown }[]; emit(): void } }).__chat;
      c.rooms = c.rooms.map((r) => (r.id === "room_i12" ? { ...r, lead: { machine: "m_other", machine_name: "elsewhere", session: "someone" } } : r));
      c.emit();
    });
    await expect(panel.locator("[data-ov-agent-tab]")).not.toHaveAttribute("data-ov-agent-hears", "");
    await expect(panel.locator("[data-ov-agent-note]")).toContainText("someone leads #12's thread");
    await expect(panel.locator("[data-ov-agent-tab] [data-ov-peek-reply]")).toBeVisible();
  });

  test("the Work level draws the card's panes live side by side: a shell and an agent block typed into, its question answered above them; a card with no agent offers its lead (Arugula #1289)", async ({ page }, info) => {
    // The suite's daemon is the machine `m_suite` here, as control would name it, so the page reaches its panes as the app does.
    await page.route("**/api/hosts", async (r) => {
      const list = (await (await r.fetch()).json()) as { this: string; hosts: unknown[] };
      // The daemon itself, by its id only: no address, so it isn't listed twice.
      await r.fulfill({ json: { ...list, hosts: [...list.hosts, { name: list.this, id: "m_suite", urls: [], transport: "tailnet", added_ms: 0, last_seen_ms: null }] } });
    });
    const { term, agent } = await overview(page, null, true);
    await page.goto(`/p/${P}/board/${B}/card/row_b`);
    const work = page.locator('[data-ov-level-work="row_b"]');
    await expect(work).toBeVisible({ timeout: 20_000 });
    // #12's session: its agent block and the terminal beside it, each live, its agent's outlined (its tab is in front).
    const pane = (id: number) => work.locator(`[data-ov-work-pane$=":${id}"]`);
    await expect(work.locator("[data-ov-work-pane]")).toHaveCount(2);
    await expect(pane(agent)).toHaveAttribute("data-ov-work-kind", "block");
    await expect(pane(term)).toHaveAttribute("data-ov-work-kind", "shell");
    await expect(pane(agent)).toHaveAttribute("data-ov-talking", "");
    await expect(pane(term)).not.toHaveAttribute("data-ov-talking", "");
    await expect(pane(term).locator(`[data-live-slot="${term}"] .xterm`)).toBeVisible({ timeout: 20_000 });
    await expect(pane(agent).locator(".agent-composer textarea")).toBeVisible();
    await expect(work.locator("[data-ov-work-noagent]")).toHaveCount(0);

    // Typed straight into the shell.
    await pane(term).locator(".xterm").click();
    await page.keyboard.type("echo WORK$((40+2))");
    await page.keyboard.press("Enter");
    await expect.poll(() => page.evaluate(async (p) => (await fetch(`/api/panes/${p}/capture?format=text`)).text(), term), { timeout: 20_000 }).toContain("WORK42");
    // Esc in a terminal is the terminal's, not a zoom out.
    await page.keyboard.press("Escape");
    await expect(work).toBeVisible();
    // (Ctrl-C drops the half-read Escape, so readline takes the next line whole.)
    await page.keyboard.press("Control+c");

    // And into the agent block's composer.
    await pane(agent).locator(".agent-composer textarea").fill("hello");
    await pane(agent).locator(".agent-composer textarea").press("Enter");
    await expect(pane(agent).locator(".agent-msg").last()).toHaveText("Hello! I am fake.", { timeout: 20_000 });
    await page.screenshot({ path: info.outputPath("overview-work-panes.png") });

    // Talk brings the session's tab to the front.
    await pane(term).locator("[data-ov-work-talk]").click();
    await expect(page.locator("[data-ov-tabs] .ov-tab-on")).toHaveAttribute("data-ov-tab", /^peek:/);

    // A question in the terminal sits above the panes, answered there as Needs you answers it.
    await page.evaluate(
      ({ term, line }) => fetch(`/api/panes/${term}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: line, enter: true }) }),
      { term, line: `out=$(${cli} ask < ${hookInput}); echo "$out" | grep -o '"Which [a-z ]*?":"[^"]*"'` },
    );
    const ask = work.locator("[data-ov-work-asks] [data-board-drawer-ask]");
    await expect(ask).toBeVisible({ timeout: 20_000 });
    await expect(ask.locator("h3")).toHaveText("Claude Code asks");
    await page.waitForTimeout(400);
    await page.screenshot({ path: info.outputPath("overview-work-live.png") });
    await ask.locator('[data-question="0"]').getByRole("radio", { name: /^Blue/ }).click();
    await ask.locator('[data-question="1"]').getByRole("checkbox", { name: /Pear/ }).check();
    await ask.locator('[data-question="2"]').getByRole("radio", { name: /^Cat/ }).click();
    await ask.getByRole("button", { name: "Submit" }).click();
    await expect(ask).toHaveCount(0);
    await expect.poll(() => page.evaluate(async (p) => (await fetch(`/api/panes/${p}/capture?format=text`)).text(), term)).toContain('"Which colour do you prefer?":"Blue"');

    // #13 has no one on it: it says so, and offers the lead.
    await page.goto(`/p/${P}/board/${B}/card/row_c`);
    const none = page.locator('[data-ov-level-work="row_c"]');
    await expect(none.locator("[data-ov-work-noagent]")).toBeVisible({ timeout: 20_000 });
    await expect(none.locator("[data-ov-work-pane]")).toHaveCount(0);
  });

  test("on a daemon's own page, whose machines have no ids, the Work level draws its own panes live and opens them in Panes (https://app.arugula.io/issues/iss_78ce975b330e471f9827d13cb22024bb)", async ({ page }) => {
    // No made-up id for the suite's daemon here: the page knows it by its name, as a daemon's own page does.
    const { term, agent } = await overview(page, null, true);
    await page.goto(`/p/${P}/board/${B}/card/row_b`);
    const work = page.locator('[data-ov-level-work="row_b"]');
    await expect(work).toBeVisible({ timeout: 20_000 });
    const pane = (id: number) => work.locator(`[data-ov-work-pane$=":${id}"]`);
    await expect(work.locator("[data-ov-work-pane]")).toHaveCount(2);
    // Live, not "this page doesn't see this session".
    await expect(pane(term).locator(`[data-live-slot="${term}"] .xterm`)).toBeVisible({ timeout: 20_000 });
    await expect(pane(agent).locator(".agent-composer textarea")).toBeVisible();
    await expect(work.locator(".ov-work-note")).toHaveCount(0);
    // And Open in Panes opens its session on its machine's page, on the pane it leads with.
    await pane(term).locator("[data-ov-work-panes]").click();
    await expect(page).toHaveURL(new RegExp(`/machines\\?(.*&)?pane=${agent}\\b`));
    await expect(page.locator(`[data-pane="${agent}"]`)).toBeVisible({ timeout: 20_000 });
  });

  test("a session's Work address by the hub's id opens it: a cloud workspace's, and one on a machine here; one not in the project says so (Arugula #1380)", async ({ page }) => {
    // The suite's daemon is the machine `m_suite`, as control names it, so the hub's session names its pane.
    await page.route("**/api/hosts", async (r) => {
      const list = (await (await r.fetch()).json()) as { this: string; hosts: unknown[] };
      await r.fulfill({ json: { ...list, hosts: [...list.hosts, { name: list.this, id: "m_suite", urls: [], transport: "tailnet", added_ms: 0, last_seen_ms: null }] } });
    });
    await overview(page);
    // The scratch session's pane, from its row on no card (`host:id`).
    const loose = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(loose).toHaveAttribute("data-ov-panes", /:\d+$/);
    const scratch = Number((await loose.getAttribute("data-ov-panes"))!.split(":").pop());
    // The hub's sessions on no card: the scratch session's pane, as the hub names its session, and a cloud workspace's.
    let answer!: () => void;
    const answered = new Promise<void>((done) => (answer = done));
    await page.route(`**/api/projects/${P}/sessions/by-card`, async (r) => {
      await answered;
      await r.fulfill({
        json: {
          project: P,
          groups: [
            {
              card: null,
              sessions: [
                { kind: "machine", id: "ses_e2escratch", name: "scratch", on: { kind: "machine", id: "m_suite", name: "suite" }, panes: [{ pane: scratch, kind: "terminal", work_item: null, state: null }], updated_at: 0 },
                { kind: "hosted", id: "ses_e2ehosted", name: "arugula/tidy", on: { kind: "hosted" }, state: "working", branch: "arugula/tidy", updated_at: 0 },
              ],
            },
          ],
        },
      });
    });
    await page.route("**/api/sessions/ses_e2ehosted", (r) =>
      r.fulfill({ json: { card: { session: "ses_e2ehosted", project: P, room: "room_hs", parent: 1, state: "working", branch: "arugula/tidy", machine: null, block: null, parked: false }, may: true, asked: true, now: Date.now(), events: [{ seq: 1, kind: "turn", from: null, to: "working", at: 1, signed: true, text: "Tidied it." }] } }),
    );

    // The machine's session: loading while the hub hasn't answered, never "isn't in the project"; then its pane.
    await page.goto(`/p/${P}/work/ses_e2escratch`);
    await expect(page.locator("[data-ov-field]")).toHaveAttribute("data-ov-level", "work", { timeout: 20_000 });
    await expect(page.locator("[data-ov-level-empty]")).toHaveCount(0);
    answer();
    const work = page.locator("[data-ov-level-work]");
    await expect(work.locator(`[data-ov-work-pane$=":${scratch}"]`)).toBeVisible({ timeout: 20_000 });
    await expect(work.locator("[data-ov-level-empty]")).toHaveCount(0);

    // The cloud workspace's: its last answer, as the Work level has a paused box's.
    await page.goto(`/p/${P}/work/ses_e2ehosted`);
    await expect(work.locator('[data-ov-work-paused="ses_e2ehosted"]')).toContainText("Tidied it.", { timeout: 20_000 });
    await expect(work.locator("h2")).toContainText("arugula/tidy");

    // A session the hub doesn't list in the project.
    await page.goto(`/p/${P}/work/ses_e2enotours`);
    await expect(page.locator("[data-ov-level-empty]")).toHaveText("This session isn't in the project now.", { timeout: 20_000 });
  });

  test("a session's tab opened before its pane is on the page finds it again after a reload, by the hub's id; a tab saved under its tile's key takes that id (Arugula #1390)", async ({ page }, info) => {
    // The suite's daemon is the machine `m_suite`, as control names it, so the hub's session names its pane.
    await page.route("**/api/hosts", async (r) => {
      const list = (await (await r.fetch()).json()) as { this: string; hosts: unknown[] };
      await r.fulfill({ json: { ...list, hosts: [...list.hosts, { name: list.this, id: "m_suite", urls: [], transport: "tailnet", added_ms: 0, last_seen_ms: null }] } });
    });
    await overview(page);
    // The scratch session's pane, from its row on no card (`host:id`).
    const loose = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(loose).toHaveAttribute("data-ov-panes", /:\d+$/);
    const scratch = Number((await loose.getAttribute("data-ov-panes"))!.split(":").pop());
    // The hub names the scratch session by a pane the page doesn't have yet, as while its daemon restarts.
    let pane = 999_999;
    await page.route(`**/api/projects/${P}/sessions/by-card`, (r) =>
      r.fulfill({
        json: {
          project: P,
          groups: [{ card: null, sessions: [{ kind: "machine", id: "ses_e2escratch", name: "scratch", on: { kind: "machine", id: "m_suite", name: "suite" }, panes: [{ pane, kind: "terminal", work_item: null, state: null }], updated_at: 0 }] }],
        },
      }),
    );
    const panel = page.locator("[data-ov-panel]");
    const ready = async () => {
      await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
      await expect(page.locator(`[data-ov-loose] [data-ov-panes$=":${scratch}"]`)).toBeVisible({ timeout: 20_000 });
    };
    await page.reload();
    await ready();
    await page.locator('[data-ov-loose] [data-ov-session="machine:m_suite:ses_e2escratch"] [data-ov-row-open]').click();
    await expect(panel.locator('[data-ov-tab^="peek:"]')).toHaveCount(1);

    // Its pane comes onto the page: the reloaded tab is that session's shell, not "isn't in the project now".
    pane = scratch;
    await page.reload();
    await ready();
    await expect(panel.locator("[data-ov-peek], [data-ov-peek-gone]")).toBeVisible({ timeout: 20_000 });
    await page.screenshot({ path: info.outputPath("overview-talk-tab-reloaded.png") });
    await expect(panel.locator("[data-ov-peek-gone]")).toHaveCount(0);
    await expect(panel.locator("[data-ov-peek]")).toHaveAttribute("data-ov-peek-kind", "shell");
    await expect(panel.locator('[data-ov-tab^="peek:"]')).toHaveCount(1);
    await expect(panel.locator('[data-ov-tab^="peek:"]')).toHaveAttribute("data-ov-tab", "peek:ses_e2escratch");

    // A tab saved under its tile's key (`host#N`) is the same session's, now keyed by the hub's id.
    const tile = (await page.locator(`[data-ov-loose] [data-ov-panes$=":${scratch}"]`).getAttribute("data-ov-session"))!.split("|").pop()!;
    expect(tile).not.toContain("ses_");
    await page.evaluate(({ P, tile }) => sessionStorage.setItem(`arugula.ov-talk:${P}`, JSON.stringify({ tabs: [{ kind: "peek", id: tile, title: "scratch" }], active: `peek:${tile}` })), { P, tile });
    await page.reload();
    await ready();
    await expect(panel.locator('[data-ov-tab^="peek:"]')).toHaveAttribute("data-ov-tab", "peek:ses_e2escratch", { timeout: 20_000 });
    await expect(panel.locator("[data-ov-peek]")).toHaveAttribute("data-ov-peek-kind", "shell");
    expect(await page.evaluate((P) => JSON.parse(sessionStorage.getItem(`arugula.ov-talk:${P}`)!).active, P)).toBe("peek:ses_e2escratch");
  });

  test("a board's lead is a chip that opens that agent, never the board's thread, which is the row's own button; the who-line says which (Arugula #1290)", async ({ page }, info) => {
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
    const row = page.locator(`[data-ov-board="${B}"]`);
    const lead = row.locator("[data-ov-board-lead]");
    await expect(lead).toHaveAttribute("data-ov-board-lead", "scratch");
    await expect(lead.locator(".ov-lead-name")).toHaveText("scratch");
    await expect(lead.locator(".ov-lead-machine")).toHaveText(host);
    // Its session's panes are on this page, so its dot says how it stands.
    await expect(lead.locator("[data-ov-lead-standing]")).toHaveCount(1);
    await expect(page.locator(`[data-ov-board="${LATER}"] [data-ov-board-lead]`)).toHaveCount(0);
    // With chat on, every row has its thread button, led or not.
    await chatOn(page);
    await expect(row.locator("[data-ov-board-talk]")).toBeVisible();
    await expect(page.locator(`[data-ov-board="${LATER}"] [data-ov-board-talk]`)).toBeVisible();
    // The chip opens the lead's own session, and doesn't toggle the row.
    await lead.click();
    const panel = page.locator("[data-ov-panel]");
    const selected = panel.locator('[role="tab"][aria-selected="true"]');
    await expect(selected).toHaveText("scratch");
    await expect(panel.locator(`[data-ov-tab="thread:${B}"]`)).toHaveCount(0);
    // The peek is the scratch session's own: its shell, as its row on no card shows it.
    await expect(panel.locator("[data-ov-peek]")).toHaveAttribute("data-ov-peek-kind", "shell");
    // Its who-line is its head (#1342, #1441): where it runs and how it stands, and its rung.
    await expect(panel.locator('[data-ov-who="peek"] [data-agent-head]')).toHaveText(`scratch · idle on ${host}`);
    await expect(panel.locator('[data-ov-who="peek"] [data-agent-rung]')).toHaveText("lead of Launch · started by you");
    await expect(panel.locator("[data-ov-peek-reply]")).toBeVisible();
    await expect(row).not.toHaveAttribute("data-ov-open", "");
    // It's the same tab as the session's own row opens.
    await page.locator("[data-ov-loose] [data-ov-session] [data-ov-row-open]").click();
    await expect(panel.locator('[data-ov-tab^="peek:"]')).toHaveCount(1);
    await page.screenshot({ path: info.outputPath("overview-lead.png") });
    // The thread is its own button, and the who-line says where a message goes there.
    await row.locator("[data-ov-board-talk]").click();
    await expect(panel.locator(`[data-ov-tab="thread:${B}"]`).getByRole("tab")).toHaveAttribute("aria-selected", "true");
    await expect(panel.locator(`[data-ov-talk-open="thread:${B}"] [data-room="room_launch"]`)).toContainText("Release notes are drafted.");
    await expect(panel.locator('[data-ov-who="thread"]')).toHaveText("Talking in Launch's thread: a message goes to the board's thread, where its lead and anyone mentioned read it.");
    await page.screenshot({ path: info.outputPath("overview-lead-thread.png") });
  });

  test("your own terminal's Claude Code session leads a board by its conversation's title, not its session's name, and opens as a live peek with Open in Panes, not a far one (Arugula #1331)", async ({ page }) => {
    // The daemon names a lead by the title of the conversation in its pane; the pane here is in the session `scratch`.
    let led: unknown;
    await overview(page, null, false, () => led);
    const tile = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(tile).toHaveAttribute("data-ov-panes", /:/);
    const host = (await tile.getAttribute("data-ov-panes"))!.split(":")[0];
    const scratch = await expect
      .poll(() => page.evaluate(async () => ((await (await fetch("/api/panes")).json()) as { id: number; session_name: string }[]).find((p) => p.session_name === "scratch")?.id ?? null))
      .not.toBeNull()
      .then(() => page.evaluate(async () => ((await (await fetch("/api/panes")).json()) as { id: number; session_name: string }[]).find((p) => p.session_name === "scratch")!.id));
    const sent = await page.evaluate((id) => fetch(`/api/panes/${id}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: "printf '\\033]0;Product Improvements\\007'", enter: true }) }).then(async (r) => r.status), scratch);
    expect(sent).toBe(200);
    await expect.poll(() => page.evaluate(async (id) => ((await (await fetch("/api/panes")).json()) as { id: number; title?: string }[]).find((p) => p.id === id)?.title, scratch)).toBe("Product Improvements");
    led = { machine: "m_suite", machine_name: host, session: "Product Improvements", at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    await launchRoom(page);
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-lead]`).click();
    const panel = page.locator("[data-ov-panel]");
    const peek = panel.locator("[data-ov-peek]");
    await expect(peek).toHaveAttribute("data-ov-peek-kind", "shell");
    await expect(peek).not.toHaveAttribute("data-ov-peek-readonly", "");
    await expect(panel.locator("[data-ov-peek-tail]")).toHaveAttribute("data-ov-peek-live", "");
    await expect(panel.locator("[data-ov-peek-panes]")).toBeEnabled();
    await expect(panel.locator('[data-ov-who="peek"] [data-agent-rung]')).toHaveText("lead of Launch · started by you");
    await expect(panel.locator("[data-ov-peek-tail]")).not.toContainText("doesn't reach");
    // It takes a message, typed into its terminal.
    await panel.locator("[data-ov-peek-reply] input").fill("echo from-the-lead");
    await panel.locator("[data-ov-peek-reply] button").click();
    await expect(panel.locator("[data-ov-peek-said]")).toHaveText("Typed, with Enter.");
  });

  test("your own lead whose pane has no title and whose session is named otherwise is found by its pane, so its peek has its lines and Open in Panes (Arugula #1368)", async ({ page }) => {
    // A Claude Code terminal's pane reports no title, and its session is `scratch`: only the pane the lead was taken from says which it is.
    let led: unknown;
    await overview(page, null, false, () => led);
    const tile = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(tile).toHaveAttribute("data-ov-panes", /:/);
    const host = (await tile.getAttribute("data-ov-panes"))!.split(":")[0];
    const scratch = Number((await tile.getAttribute("data-ov-panes"))!.split(":").pop());
    await expect.poll(() => page.evaluate(async (id) => ((await (await fetch("/api/panes")).json()) as { id: number; title?: string }[]).find((p) => p.id === id)?.title ?? "", scratch)).toBe("");
    led = { machine: "m_suite", machine_name: host, session: "Product Improvements", pane: scratch, at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    await launchRoom(page);
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-lead]`).click();
    const panel = page.locator("[data-ov-panel]");
    const peek = panel.locator("[data-ov-peek]");
    await expect(peek).toHaveAttribute("data-ov-peek-kind", "shell");
    await expect(peek).not.toHaveAttribute("data-ov-peek-readonly", "");
    await expect(panel.locator("[data-ov-peek-tail]")).toHaveAttribute("data-ov-peek-live", "");
    await expect(panel.locator("[data-ov-peek-panes]")).toBeEnabled();
    await expect(panel.locator('[data-ov-who="peek"] [data-agent-rung]')).toHaveText("lead of Launch · started by you");
    await expect(panel.locator("[data-ov-peek-tail]")).not.toContainText("doesn't reach");
  });

  test("a lead found by its pane is that pane alone, not the session's worst: another pane of its session that needs input leaves its peek and its standing as its own (Arugula #1368)", async ({ page }) => {
    let led: unknown;
    await overview(page, null, false, () => led);
    const tile = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(tile).toHaveAttribute("data-ov-panes", /:/);
    const host = (await tile.getAttribute("data-ov-panes"))!.split(":")[0];
    const scratch = Number((await tile.getAttribute("data-ov-panes"))!.split(":").pop());
    led = { machine: "m_suite", machine_name: host, session: "Product Improvements", pane: scratch, at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    await launchRoom(page);
    // Another pane in the lead's own session asks for input.
    await page.evaluate(
      ({ host, scratch, P }) => {
        const fleet = (window as unknown as { __app: { fleet: { panes: { host: string; id: number; session: unknown }[]; inject(p: unknown[]): void } } }).__app.fleet;
        const mine = fleet.panes.find((p) => p.host === host && p.id === scratch)!;
        fleet.inject([
          {
            key: `${host}:9999`,
            host,
            id: 9999,
            session: mine.session,
            stale: false,
            person: { id: "me", name: "me", kind: "me" },
            driver: null,
            watchers: [],
            unread: 0,
            mention: false,
            info: {
              id: 9999,
              title: "Board",
              kind: "agent",
              attention: "needs_input",
              session_project: { id: P, name: "Arugula" },
              activity: { bps: 0, last_ms: Date.now() },
              ask: { kind: "permission", id: "perm_x", tool: "Bash", input: { command: "ls" }, message: "", suggestions: [] },
              reason: { kind: "ask", since_ms: 1000, headline: "Bash wants to run ls", bundle: `ask:${P}:x`, ask: { id: "perm_x", what: "approve", agent: "claude", tool: "Bash" }, actions: ["allow", "deny"] },
            },
          },
        ]);
      },
      { host, scratch, P },
    );
    const chip = page.locator(`[data-ov-board="${B}"] [data-ov-board-lead]`);
    await expect(chip.locator("[data-ov-lead-standing]")).not.toHaveAttribute("data-ov-lead-standing", "needs");
    await chip.click();
    const panel = page.locator("[data-ov-panel]");
    const peek = panel.locator("[data-ov-peek]");
    await expect(peek).toHaveAttribute("data-ov-peek", new RegExp(`:${scratch}$`));
    await expect(peek).toHaveAttribute("data-ov-peek-kind", "shell");
    await expect(panel.locator("[data-ov-peek-tail]")).toHaveAttribute("data-ov-peek-live", "");
    await expect(panel.locator("[data-ov-peek-state]")).not.toHaveText(/needs/i);
  });

  /** A board led from geek, a teammate's machine, by its pane 77. */
  async function geekLead(page: Page) {
    let led: unknown;
    await overview(page, null, false, () => led);
    led = { machine: "m_geek", machine_name: "geek", session: "Plan overview page prototype", pane: 77, at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    await launchRoom(page);
  }

  test("a teammate's lead found by its pane is watched, though its pane has no title and its session is named otherwise (Arugula #1368)", async ({ page }) => {
    await geekLead(page);
    await page.evaluate((P) => {
      const fleet = (window as unknown as { __app: { fleet: { inject(p: unknown[]): void; role(p: { host: string }): string } } }).__app.fleet;
      const role = fleet.role.bind(fleet);
      fleet.role = (p) => (p.host === "geek" ? "viewer" : role(p));
      fleet.inject([
        {
          key: "geek:77",
          host: "geek",
          id: 77,
          session: { id: 3, name: "geek-main" },
          stale: false,
          owner: "jhgaylor",
          person: { id: "account:jhgaylor", name: "jhgaylor", kind: "person" },
          driver: null,
          watchers: [],
          unread: 0,
          mention: false,
          info: { id: 77, kind: "terminal", attention: "idle", session_project: { id: P, name: "Arugula" }, activity: { bps: 0, last_ms: Date.now() } },
        },
      ]);
    }, P);
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-lead]`).click();
    const panel = page.locator("[data-ov-panel]");
    await expect(panel.locator('[data-ov-who="peek"] [data-agent-rung]')).toHaveText("lead of Launch · started by jhgaylor");
    await expect(panel.locator("[data-ov-peek-readonly]")).toBeVisible();
    await expect(panel.locator('[data-ov-who="peek"]')).not.toContainText("doesn't reach");
    await expect(panel.locator("[data-ov-peek-tail]")).not.toContainText("doesn't reach");
    await expect(panel.locator("[data-ov-peek-panes]")).toBeEnabled();
  });

  test("a teammate's lead whose pane isn't shared with you says whose machine it runs on, not that this page doesn't reach it (Arugula #1368)", async ({ page }) => {
    await geekLead(page);
    // The machine is listed, as a teammate's, with none of its panes.
    await page.evaluate(() => {
      const fleet = (window as unknown as { __app: { fleet: { list: unknown[] } } }).__app.fleet;
      const real = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(fleet), "list")!.get!.bind(fleet);
      Object.defineProperty(fleet, "list", { configurable: true, get: () => [...real(), { name: "geek", id: "m_geek", transport: "relay", owner: "jhgaylor", ownerId: "jhgaylor", state: "connected", lastSeen: null, summary: null, path: null }] });
    });
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-lead]`).click();
    const panel = page.locator("[data-ov-panel]");
    await expect(panel.locator("[data-ov-peek-tail]")).toHaveText("It runs on geek, jhgaylor's machine, whose panes aren't shared with you.");
    await expect(panel.locator("[data-ov-peek-panes]")).toBeDisabled();
  });

  test("a lead with no pane (from an older daemon) is still found by its session's name (Arugula #1368)", async ({ page }) => {
    let led: unknown;
    await overview(page, null, false, () => led);
    const tile = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(tile).toHaveAttribute("data-ov-panes", /:/);
    const host = (await tile.getAttribute("data-ov-panes"))!.split(":")[0];
    led = { machine: "m_suite", machine_name: host, session: "scratch", at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    await launchRoom(page);
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-lead]`).click();
    const panel = page.locator("[data-ov-panel]");
    await expect(panel.locator("[data-ov-peek]")).toHaveAttribute("data-ov-peek-kind", "shell");
    await expect(panel.locator("[data-ov-peek-panes]")).toBeEnabled();
    await expect(panel.locator("[data-ov-peek-tail]")).not.toContainText("doesn't reach");
  });

  test("the Board level: its goal and bar, its lead's card with Talk to the lead and Board thread apart, its open tickets with their word and the agents on them, Done folded; a board no one leads asks the chief of staff for one (Arugula #1288)", async ({ page }, info) => {
    // Launch is led by the project's scratch session, and #12's agent is on it; Later has no lead.
    let led: unknown;
    await overview(page, null, true, () => led);
    const loose = page.locator("[data-ov-loose] [data-ov-panes]");
    await expect(loose).toHaveAttribute("data-ov-panes", /:/);
    const host = (await loose.getAttribute("data-ov-panes"))!.split(":")[0];
    led = { machine: "m_suite", machine_name: host, session: "scratch", at: 1 };
    await page.reload();
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    await launchRoom(page);
    await chatOn(page);
    await page.locator(`[data-ov-board="${B}"] [data-ov-board-title]`).click();
    const level = page.locator(`[data-ov-level-board="${B}"]`);
    await expect(level).toBeVisible();
    await expect(level.getByRole("progressbar")).toBeVisible();
    const panel = page.locator("[data-ov-panel]");
    const selected = panel.locator('[role="tab"][aria-selected="true"]');

    // The lead's card: its name, its machine, the one board it leads.
    const lead = level.locator('[data-ov-lead-card="scratch"]');
    await expect(lead.locator(".ov-bl-lead-name")).toHaveText("scratch");
    await expect(lead.locator("[data-ov-lead-sub]")).toContainText(`on ${host}`);
    await expect(lead.locator("[data-ov-lead-sub]")).toContainText("leads this board");

    // Open tickets in lane order, each with its word; Done folded into one line.
    const tickets = level.locator("[data-ov-ticket]");
    await expect(tickets).toHaveCount(4);
    const word = (row: string) => level.locator(`[data-ov-ticket="${row}"] [data-ov-ticket-word]`);
    await expect(word("row_b")).toHaveText("working");
    await expect(word("row_r")).toHaveText("in review");
    await expect(word("row_a")).toHaveText("next");
    await expect(level.locator('[data-ov-ticket="row_a"] [data-ov-ticket-pr]')).toHaveText("no agent yet");
    const done = level.locator("[data-ov-level-done]");
    await expect(done.locator("summary")).toHaveText("1 done ticket");
    await expect(level.locator('[data-ov-ticket="row_d"]')).toHaveCount(0);
    await done.locator("summary").click();
    await expect(level.locator('[data-ov-ticket="row_d"] [data-ov-ticket-word]')).toHaveText("done");
    await done.locator("summary").click();

    // #12's agent is a chip; it opens that agent's tab, and the board stays.
    const chip = level.locator('[data-ov-ticket="row_b"] [data-ov-agent-chip]');
    await expect(chip).toHaveCount(1);
    await expect(chip.locator(".ov-lead-name")).toHaveText("Agent");
    await page.waitForTimeout(300);
    await page.screenshot({ path: info.outputPath("overview-board-level.png") });
    await chip.click();
    await expect(selected).toHaveText("Agent");
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/board/${B}[^/]*$`));

    // Talk to the lead: the lead's own session, not the board's thread.
    await lead.locator("[data-ov-lead-talk]").click();
    await expect(selected).toHaveText("scratch");
    await expect(panel.locator(`[data-ov-tab="thread:${B}"]`)).toHaveCount(0);
    // Board thread: its own button, its own tab.
    await lead.locator("[data-ov-lead-thread]").click();
    await expect(panel.locator(`[data-ov-tab="thread:${B}"]`).getByRole("tab")).toHaveAttribute("aria-selected", "true");

    // A ticket's row (anywhere but a chip) zooms to its Work level.
    await level.locator('[data-ov-ticket="row_r"] [data-ov-ticket-word]').click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/board/${B}[^/]*/card/row_r$`));

    // Later has no lead: it says so, and asks the chief of staff in the staff room once there is one.
    await page.goto(`/p/${P}/board/${LATER}`);
    await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
    const later = page.locator(`[data-ov-level-board="${LATER}"]`);
    await expect(later).toBeVisible();
    const none = later.locator('[data-ov-lead-card=""]');
    await expect(none).toContainText("No lead");
    await expect(none.locator("[data-ov-lead-talk]")).toHaveCount(0);
    await chatOn(page);
    await expect(none.locator("[data-ov-ask-chief]")).toHaveCount(0);
    await page.evaluate((P) => {
      const room = { id: "dm_cos", kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: 0, read: 0, unread: 0, mentions: 0, wants: 0, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead: { machine: "m_geek", machine_name: "geek", session: "cos" } };
      const posted: string[] = [];
      (window as unknown as { posted: string[] }).posted = posted;
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === "/api/chat/rooms/dm_cos/messages") {
          if (init?.method === "POST") {
            posted.push((JSON.parse(String(init.body)) as { text: string }).text);
            return json({ seq: posted.length, mentions: [], unreached: [] });
          }
          return json({ room, messages: [], more: false });
        }
        return before(input, init);
      };
      const c = (window as unknown as { __chat: { rooms: unknown[]; emit(): void } }).__chat;
      c.rooms = [...c.rooms, room];
      c.emit();
    }, P);
    await expect(none.locator("[data-ov-lead-thread]")).toBeVisible();
    await page.screenshot({ path: info.outputPath("overview-board-level-no-lead.png") });
    await none.locator("[data-ov-ask-chief]").click();
    await expect.poll(() => page.evaluate(() => (window as unknown as { posted: string[] }).posted)).toEqual([`Please start a session to lead the board "Later" (${LATER}): read its goal and open tickets, then lead it.`]);
    await expect(panel.locator('[data-ov-tab="staff:dm_cos"]').getByRole("tab")).toHaveAttribute("aria-selected", "true");
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
      const w = window as unknown as { __skew: number };
      w.__skew = 0;
      const now = Date.now.bind(Date);
      Date.now = () => now() + w.__skew;
      Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect.poll(() => put).toEqual([{ seq: 120 }]);
    // Coming back after a while reads it again. Now the reader's inbox asks their review on #7 (and #9, which isn't ready), so #7 counts.
    await page.evaluate(() => {
      const entry = (n: number, reason: string) => ({ item: `fi_${n}`, reason, repo: "o/app", number: n, kind: "pr", state: "open", title: "A PR", author: "jo", url: "", project: null, seen_at: null });
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === "/api/inbox") return new Response(JSON.stringify({ entries: [entry(7, "review_requested"), entry(9, "review_requested"), entry(8, "mentioned")] }), { headers: { "Content-Type": "application/json" } });
        return before(input, init);
      };
      (window as unknown as { __skew: number }).__skew += 30_000;
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
    // Working has the yellow (warn); quiet is the muted grey (#1445).
    const colour = (v: string) => page.evaluate((name) => { const e = document.createElement("i"); e.style.color = `var(${name})`; document.body.append(e); const c = getComputedStyle(e).color; e.remove(); return c; }, v);
    const warn = await colour("--warn");
    const working = tile(page, "row_c").locator("[data-ov-card-state]");
    await expect(working).toHaveText("working");
    await expect(working).toHaveCSS("color", warn);
    // Its session in the preview, on no card: shows its standing word the same way.
    const quiet = page.locator("[data-ov-loose] [data-ov-session]", { hasText: "release" }).locator(".ov-word");
    await expect(quiet).toHaveText("quiet");
    await expect(quiet).not.toHaveCSS("color", warn);
    await expect(quiet).toHaveCSS("color", await colour("--muted"));
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
    await expect(page.locator("[data-ov-split]")).toHaveCount(0);
    const ov = page.locator("[data-overview]");
    await expect(ov).toHaveClass(/phone/);
    const first = await ov.locator("> *").first().evaluate((e) => [...e.attributes].map((a) => a.name));
    expect(first).toContain("data-ov-needs");
    await openLaunch(page);
    await expect(tile(page, "row_b")).toBeVisible();
    await expect(page.locator("[data-phone-tabs] [data-tab-overview]")).toHaveAttribute("aria-current", "page");
    // No Swarm on a phone's Overview: no views in its bar (Arugula #1345).
    await expect(page.locator("[data-platform-bar] [data-place-views]")).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("overview-phone.png") });
  });

  test("on a phone a board's group title and count are apart, and a GitHub pointer is short and on one line", async ({ page }) => {
    await overview(page);
    await expectSpaced(page);
  });

  test("on a phone an ask's two lines fit the column and open in it (Arugula #1333)", async ({ page }) => {
    await overview(page);
    await asking(page, [{ host: "sam-box", person: SAM, ask: "a long command that goes on and on and on and on and on and on and on" }]);
    const row = page.locator('[data-ov-needs] [data-ov-ask][data-kind="ask"]');
    await row.locator(".ov-ask-row").click();
    await expect(row.getByRole("button", { name: "Allow", exact: true })).toBeVisible();
    const fits = await page.locator("[data-ov-needs]").evaluate((e) => e.scrollWidth <= e.clientWidth);
    expect(fits).toBe(true);
  });
});
