// The Overview's specs' fixture (overview.spec.ts, overview-needs.spec.ts):
// the app's frame over the suite's daemon (app-frame-page.tsx), on the
// project Arugula with two boards, Launch (cards in every lane, #12 worked
// by an agent block of the daemon's) and Later (one card in Next, so it's
// quiet), the hub's sessions by card, what the Inbox lists (`say`), made-up
// panes that ask (`asking`), and the chief of staff's room (`proposing`,
// `staffRoomOn`). The platform's boards and the hub's sessions are made up
// here; the panes are the daemon's own.
import { fileURLToPath } from "node:url";
import { expect, type Locator, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

export const P = "prj_aaaa0002";
export const B = "brd_e2e0launch";
export const LATER = "brd_e2e1later";

const fakeAgent = fileURLToPath(new URL("../../crates/daemon/tests/fake_acp.py", import.meta.url));
export const hookInput = fileURLToPath(new URL("../../crates/daemon/tests/fixtures/s13-hook-ask.json", import.meta.url));
export const cli = fileURLToPath(new URL("../../target/debug/arugula", import.meta.url));


export type Lane = "next" | "doing" | "review" | "done";
export const LANES: Lane[] = ["next", "doing", "review", "done"];
export const board = (id: string, title: string) => ({ id, project: P, title, created_by: "", created_at: 1, updated_at: 1, goal: "", done_note: "", progress: { done: 0, total: 0, proven: 0, review: 0, doing: 0 } });
/** A board's view as the platform answers it: a row is `[id, lane, number, title]`, its card the native issue `iss_<number>`. */
export function view(id: string, title: string, rows: [string, Lane, number, string][], attached: Record<string, unknown[]> = {}) {
  const row = ([rid, lane, n, t]: [string, Lane, number, string]) => ({
    id: rid, rev: 1, pointer: { kind: "issue", id: `iss_${n}` }, note: "", status: "none", lane, attachments: attached[rid] ?? [],
    live: { state: "open", kind: "native", number: n, title: t }, created_at: 1, updated_at: n,
  });
  return { board: board(id, title), zones: LANES.map((l) => ({ id: `lane_${l}`, rev: 1, title: l[0].toUpperCase() + l.slice(1), kind: "lane", rows: rows.filter((r) => r[1] === l).map(row) })), groups: [], docs: [] };
}
export const ROWS: [string, Lane, number, string][] = [
  ["row_a", "next", 11, "Write the changelog"],
  ["row_b", "doing", 12, "Fix the installer"],
  ["row_c", "doing", 13, "Tidy the docs"],
  ["row_r", "review", 14, "Cut the release"],
  ["row_d", "done", 15, "Pick a name"],
];
/** `live`: #12's work is the suite's daemon's pane (the machine `m_suite` on the fixture page), as #987's drawer draws it. */
export const launch = (live?: number, lead?: unknown) => {
  const v = view(B, "Launch", ROWS, live === undefined ? {} : { row_b: [{ id: "att_term", role: "work", pointer: { kind: "block", machine: "m_suite", pane: String(live) }, block: { state: "working", title: "Fixer" } }] });
  return lead ? { ...v, board: { ...v.board, lead } } : v;
};

/** The platform's boards for the project, and the hub's sessions by card (`null`: the route isn't there yet, 404). The open boards' views come in one read, after `hold` if it's given; `null`: a hub with no such route (404). */
export async function serve(page: Page, byCard: unknown = null, live?: number, lead?: () => unknown, hold?: Promise<void> | null) {
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
export async function overview(page: Page, byCard: unknown = null, live = false, lead?: () => unknown, hold?: Promise<void> | null): Promise<{ term: number; agent: number; scratch: number }> {
  await reset(page);
  const [term] = await panes(page);
  await serve(page, byCard, live ? term : undefined, lead, hold);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  // Nothing in the Inbox until a test says so (`say`).
  await page.addInitScript(() => void ((window as unknown as { __inbox?: unknown[] }).__inbox ??= []));
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
export const START = { id: "prp_start", action: "start", by_session: "ses_cos", issue: { id: "iss_12", number: 12, title: "Fix the installer" }, repo: "acme/app", prompt: null, sessions: [], why: "Nobody works on it and the release waits on it." };
export const CLOSE = {
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

export const RUN = {
  id: "prp_run",
  action: "run_board",
  by_session: "Chief of staff",
  issue: null,
  repo: null,
  prompt: null,
  sessions: [],
  why: "Its plan is ready and nobody leads it.",
  run: {
    board: "brd_e2e1later",
    board_title: "Later",
    lead_machine: { id: "m_geek", name: "geek" },
    machines: [
      { id: "m_geek", name: "geek" },
      { id: "m_mini", name: "jake-mini" },
    ],
    cap: 3,
    repo: "acme/app",
    checkouts: [
      { machine: "m_geek", repo: "acme/app" },
      { machine: "m_mini" },
    ],
  },
};

/** The reader's staff room, with `proposal`'s message in it (seq 2), and the
 * hub's routes for it: the project's open asks, the ask, and its answer
 * (each body posted in `window.posted`). The fixture page answers chat
 * itself, so it's made up in the page. */
export async function proposing(page: Page, proposal: typeof START | typeof CLOSE | typeof RUN) {
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
  // And the Inbox lists it (Arugula #1433).
  await say(page, [{ id: proposal.id, kind: "proposal", group: "decide", counts: true, for: "acct_a", title: proposal.why, from: {}, subject: { type: "proposal", id: proposal.id, url: "/", project: P }, since: 1 }]);
}

/** Launch's room on control, its messages and what's posted in it (in
 * `window.said`): the fixture page answers chat itself, so it's made up in
 * the page. */

export async function staffRoomOn(page: Page, lead: unknown): Promise<{ host: string; head: Locator }> {
  await page.route("**/api/hosts", async (r) => {
    const list = (await (await r.fetch()).json()) as { this: string; hosts: unknown[] };
    const pi = { name: "pi", id: "m_pi", urls: ["http://127.0.0.1:9"], transport: "tailnet", added_ms: 0, last_seen_ms: null, status: "offline" };
    await r.fulfill({ json: { ...list, hosts: [...list.hosts, { name: list.this, id: "m_suite", urls: [], transport: "tailnet", added_ms: 0, last_seen_ms: null }, pi] } });
  });
  await overview(page);
  const host = await suiteHost(page);
  await page.evaluate((host) => {
    type Fleet = { list: unknown[]; clientOf(h: string): unknown; request(h: string, ...rest: unknown[]): Promise<unknown> };
    const w = window as unknown as { __app: { fleet: Fleet }; __asked: string[] };
    const fleet = w.__app.fleet;
    const proto = Object.getPrototypeOf(fleet) as Fleet;
    const real = Object.getOwnPropertyDescriptor(proto, "list")!.get!.bind(fleet);
    Object.defineProperty(fleet, "list", { configurable: true, get: () => [...(real() as unknown[]), { name: "laptop", id: "m_laptop", transport: "tailnet", state: "connected", lastSeen: null, summary: null, path: null }] });
    w.__asked = [];
    fleet.clientOf = (h) => proto.clientOf.call(fleet, h === "laptop" ? host : h);
    fleet.request = (h, ...rest) => (w.__asked.push(h), proto.request.call(fleet, h === "laptop" ? host : h, ...rest));
  }, host);
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate(
    ({ P, first }) => {
      const room = (lead: unknown) => ({ id: "dm_cos", kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: 0, read: 0, unread: 0, mentions: 0, wants: 0, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead });
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
      let lead = first;
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === `/api/chat/projects/${P}/staff` && init?.method === "POST") {
          lead = { machine: "m_laptop", machine_name: "laptop", session: "Chief of staff · Arugula" };
          return json(room(lead));
        }
        if (url.pathname === "/api/chat/rooms/dm_cos/messages") return json({ room: room(lead), messages: [], more: false });
        return before(input, init);
      };
      const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
      c.on = true;
      c.rooms = [...c.rooms, room(lead)];
      c.emit();
    },
    { P, first: lead },
  );
  return { host, head: page.locator('[data-ov-panel] [data-ov-who="staff"] [data-agent-head]') };
}

/** `move`'s start with laptop picked (#1453, #1472): the grant card asks
 * for laptop's conversation powers over the reader's DMs and Allow gives
 * them; then start_agent, recipe chief-of-staff, in the project, goes to
 * laptop, and the room ends led by its block there, the offer gone.
 * `said` is what start_agent answers. */
export async function startsOnLaptop(page: Page, move: Locator, head: Locator, said: string) {
  const given: unknown[] = [];
  await page.route("**/api/grants", (r) => {
    if (r.request().method() !== "POST") return r.fulfill({ json: { grants: [] } });
    given.push(r.request().postDataJSON());
    return r.fulfill({ json: {} });
  });
  const calls: unknown[] = [];
  await page.route("**/api/mcp/call", (r) => {
    calls.push(r.request().postDataJSON());
    return r.fulfill({ json: { content: [{ type: "text", text: "Started an agent in %9" }], structuredContent: { block: 9, chief_of_staff: { project: P, session: "Chief of staff · Arugula", lead: true, said } } } });
  });
  await move.locator("[data-ov-staff-go]").click();
  const card = move.locator('[data-ov-staff-grant="laptop"]');
  await expect(card).toBeVisible();
  expect(calls).toHaveLength(0);
  await card.locator("[data-ov-staff-allow]").click();
  await expect.poll(() => calls).toEqual([{ tool: "start_agent", arguments: { agent: "claude", recipe: "chief-of-staff", project: P, prompt: "" }, client: "Overview" }]);
  expect(await page.evaluate(() => (window as unknown as { __asked: string[] }).__asked)).toEqual(["laptop"]);
  expect(given.length).toBeGreaterThan(0);
  expect(new Set(given.map((g) => (g as { daemon: string }).daemon))).toEqual(new Set(["m_laptop"]));
  await expect(head).toHaveText("Chief of staff · idle on laptop");
  await expect(page.locator("[data-ov-staff-move]")).toHaveCount(0);
}

export async function chatOn(page: Page) {
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate(() => {
    const c = (window as unknown as { __chat: { on: boolean | null; emit(): void } }).__chat;
    c.on = true;
    c.emit();
  });
}

/** The threads the page opens (`window.__opened`): the fixture page answers chat inside the page, so no request shows it. */
export async function spyOpened(page: Page) {
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
export const opened = (page: Page) => page.evaluate(() => [...((window as unknown as { __opened?: string[] }).__opened ?? [])].sort());


export type AskSpec = { host: string; person: { id: string; name: string; kind: "me" | "person" | "team" }; owner?: string; ask: string; watch?: boolean };
/** Asks of made-up panes on the Overview's project, beside the daemon's own: the fleet takes them as it takes a fake one. A `watch` one is a session you only view. */
/** What the platform's Inbox lists (Arugula #1433), said to the page as
 * its stream says the Inbox changed. */
export const say = (page: Page, items: unknown[]) => page.evaluate((items) => (window as unknown as { __inboxSay(i: unknown[]): void }).__inboxSay(items), items);
/** The Inbox's item for pane `pane` on `host` (#1431's): the reader's, or
 * `who`'s, not counted. */
export const paneItem = (host: string, pane: number, who?: string) => ({
  id: `${host}.${pane}:1`,
  kind: "agent_ask",
  group: "decide",
  counts: !who,
  for: who ?? "acct_a",
  title: "An agent asks",
  from: {},
  subject: { type: "pane", id: `${host}.${pane}`, url: "/", project: P, machine: host, pane: String(pane) },
  since: pane,
});

export async function asking(page: Page, specs: AskSpec[]) {
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
  // And the Inbox lists them: a teammate's (or a team's box) for them.
  await say(page, specs.map((s, i) => paneItem(s.host, i + 1, s.person.kind === "me" ? undefined : (s.owner ?? s.person.name))));
}
export const ME = { id: "me", name: "me", kind: "me" as const };
export const SAM = { id: "account:sam", name: "sam", kind: "person" as const };
export const JO = { id: "account:jo", name: "jo", kind: "person" as const };
export const INFRA = { id: "team:infra", name: "infra", kind: "team" as const };


/** The suite daemon's machine name, as the fleet has it. */
export const suiteHost = (page: Page) => page.evaluate(() => (window as unknown as { __app: { fleet: { panes: { host: string }[] } } }).__app.fleet.panes[0]!.host);
