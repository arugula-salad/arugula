// An agent session's status card (M257) in a thread on the platform page:
// Alice (owner) and Bob (editor) in team Acme, with chat on and one hosted
// workspace written into control's database, against an E2B nobody
// answers. `@arugula` in #general starts a session (M256), and its card is
// the platform's message in the timeline. No box runs here, so what the box
// reports (taking it, asking, finishing a turn) is written into the
// database, with the live event the platform sends for it (`chat_events`),
// as the box's signed reports would leave them; what people do (Bob's
// answer, Stop) and what the platform does (marking a quiet box's session)
// go through the platform itself.
//
// The card goes queued → working → needs input → done, Bob answers the
// question from his page, a second session's box goes quiet (stopped
// responding), and Bob stops it. The session's own page shows its history.

import { DatabaseSync } from "node:sqlite";
import { randomBytes } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("sessions");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let app = "";
let team = "";
let project = "";
let general = "";
let bobAccount = "";
const machine = randomBytes(16).toString("hex");

/** Write to (or read) control's database beside it. */
function sql<T>(f: (db: DatabaseSync) => T): T {
  const db = new DatabaseSync(t.db);
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    return f(db);
  } finally {
    db.close();
  }
}

interface Ses {
  id: string;
  room_id: string;
  parent_seq: number;
  state: string;
  last_seq: number;
  daemon_n: number;
}

const sessionOn = (parent: number) =>
  sql((db) => db.prepare("SELECT id, room_id, parent_seq, state, last_seq, daemon_n FROM agent_sessions WHERE parent_seq = ?").get(parent) as unknown as Ses | undefined);

/** What the box would report, as the platform keeps it: the session's new
 * state, its event (signed: a stand-in signature, nobody checks it here),
 * and the live event that tells the card. `heard` is when the platform
 * last heard from the box. */
function boxSays(ses: string, kind: "state" | "turn", to: string | null, detail: Record<string, unknown>, heard = Date.now()) {
  sql((db) => {
    const s = db.prepare("SELECT state, room_id, last_seq, daemon_n FROM agent_sessions WHERE id = ?").get(ses) as unknown as Ses & { room_id: string };
    const now = Date.now();
    const seq = s.last_seq + 1;
    const n = s.daemon_n + 1;
    const next = to ?? s.state;
    db.prepare(
      `INSERT INTO session_events (session_id, seq, kind, from_state, to_state, at, detail, daemon, daemon_n, sig)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '00')`,
    ).run(ses, seq, kind, s.state, next, now, JSON.stringify(detail), machine, n);
    db.prepare(
      `UPDATE agent_sessions SET state = ?, last_seq = ?, daemon_n = ?, heard_at = ?, updated_at = ?, machine = ?,
         block = COALESCE(?, block), last_tool = ? WHERE id = ?`,
    ).run(next, seq, n, heard, heard, machine, typeof detail.block === "string" ? detail.block : null, typeof detail.tool === "string" ? detail.tool : null, ses);
    const prompts = Array.isArray(detail.prompts) ? (detail.prompts as string[]) : [];
    for (const p of prompts) db.prepare("UPDATE session_prompts SET state = 'sent', sent_at = ? WHERE id = ?").run(now, p);
    const actions = Array.isArray(detail.actions) ? (detail.actions as string[]) : [];
    for (const a of actions) db.prepare("UPDATE session_actions SET state = 'done', done_at = ?, done_n = ? WHERE id = ?").run(now, n, a);
    db.prepare("INSERT INTO chat_events (kind, body, to_account, at) VALUES ('live', ?, NULL, ?)").run(JSON.stringify({ type: "session", room: s.room_id, ses }), now);
  });
}

async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as unknown };
    },
    [method, path, body] as const,
  );
}

async function say(p: Page, text: string): Promise<number> {
  const r = await api(p, "POST", `/api/chat/rooms/${general}/messages`, { text });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return (r.body as { seq: number }).seq;
}

/** #general on the platform page. */
async function room(p: Page) {
  await p.bringToFront();
  await p.goto(`${app}/p/${project}/chat/${general}`);
  await expect(p.locator("[data-chat-control] [data-room-view]")).toBeVisible({ timeout: 15_000 });
}

const card = (p: Page, ses: string) => p.locator(`[data-room-view] [data-session-card="${ses}"]`).first();

test("Acme with chat on, a hosted workspace, and #general", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost", "--hosted-accounts", "*", "--e2b-api-key", "e2e-nobody", "--e2b-url", "http://127.0.0.1:9"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
  bob = await t.person(browser, "bob");
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await expect(bob.locator("[data-invite-pending]")).toBeVisible();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  bobAccount = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "bob")!.account);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);

  // Signed in on the platform page (its sign-in hop has come back).
  await alice.goto(`${app}/`);
  await expect(alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" })).toBeVisible({ timeout: 15_000 });
  const projects = (await api(alice, "GET", "/api/projects")).body as { me: { account: string }; projects: { id: string; team?: { id: string } | null }[] };
  project = projects.projects.find((p) => p.team?.id === team)!.id;
  const me = projects.me.account;
  const now = Date.now();
  sql((db) =>
    db
      .prepare(
        `INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, lease_until, created_by, created_at, updated_at)
         VALUES (?, ?, 'dev', '{}', 'h', 'ready', ?, ?, ?, ?, ?)`,
      )
      .run(`hw_${randomBytes(16).toString("hex")}`, project, machine, now + 86_400_000, me, now, now),
  );
  const ch = await api(alice, "POST", `/api/chat/teams/${team}/channels`, { name: "general" });
  expect(ch.status, JSON.stringify(ch.body)).toBe(200);
  general = (ch.body as { id: string }).id;
  expect((await api(alice, "POST", `/api/chat/rooms/${general}/members`, { accounts: [bobAccount] })).status).toBe(200);
});

let first: Ses;

test("the card goes queued, working, needs input, and Bob answers it", async () => {
  test.setTimeout(120_000);
  await room(alice);
  const parent = await say(alice, "@arugula fix the login page");
  await expect.poll(() => sessionOn(parent)?.state, { timeout: 15_000 }).toBe("queued");
  first = sessionOn(parent)!;
  const a = card(alice, first.id);
  await expect(a).toHaveAttribute("data-session-state", "queued", { timeout: 15_000 });
  await expect(a.locator("[data-session-label]")).toHaveText("queued");
  await expect(a.locator("[data-session-branch]")).toContainText(`arugula/${first.id.slice(4, 12)}-fix-the-login-page`);
  await expect(a.locator("[data-session-pr]")).toBeDisabled();

  // The box takes it and runs a tool.
  const prompt = sql((db) => (db.prepare("SELECT id FROM session_prompts WHERE session_id = ?").get(first.id) as { id: string }).id);
  boxSays(first.id, "state", "working", { prompts: [prompt], block: "3", tool: "Bash" });
  await expect(a).toHaveAttribute("data-session-state", "working", { timeout: 15_000 });
  await expect(a.locator("[data-session-tool]")).toContainText("Bash");
  await expect(a.locator("[data-session-open]")).toBeVisible();

  // It asks; Bob, an editor, sees the question on his page and answers.
  boxSays(first.id, "state", "needs_input", { question: "Which login page: web or admin?", request: "req_1", ask: "question" });
  await expect(a.locator("[data-session-label]")).toHaveText("needs input", { timeout: 15_000 });
  await room(bob);
  const b = card(bob, first.id);
  await expect(b.locator(".ask-message")).toHaveText("Which login page: web or admin?", { timeout: 15_000 });
  await b.getByRole("textbox", { name: /Your answer/ }).fill("the web one");
  await b.locator("[data-ask-submit]").click();
  await expect(b.locator("[data-session-answered]")).toContainText("Answered by bob");
  // Alice's card says so too, live.
  await expect(a.locator("[data-session-answered]")).toContainText("Answered by bob", { timeout: 15_000 });
  const action = sql((db) => db.prepare("SELECT id, kind, by_account, request, answer FROM session_actions WHERE session_id = ?").get(first.id) as Record<string, string>);
  expect([action.kind, action.by_account, action.request, JSON.parse(action.answer)]).toEqual(["answer", bobAccount, "req_1", { question_0: "the web one" }]);
});

test("the box takes the answer and the turn ends: done, with what it left", async () => {
  test.setTimeout(60_000);
  const action = sql((db) => (db.prepare("SELECT id FROM session_actions WHERE session_id = ?").get(first.id) as { id: string }).id);
  boxSays(first.id, "state", "working", { actions: [action] });
  const a = card(alice, first.id);
  await expect(a.locator("[data-session-label]")).toHaveText("working", { timeout: 15_000 });
  await expect(a.locator("[data-session-answered]")).toHaveCount(0);
  const branch = (await a.locator("[data-session-branch]").textContent())!.trim();
  boxSays(first.id, "turn", null, { branch, pushed: true, commits: ["a1b2c3 fix the web login"], diffstat: " web/login.ts | 12 ++++++++----\n 1 file changed" });
  boxSays(first.id, "state", "done", {});
  await expect(a.locator("[data-session-label]")).toHaveText("done", { timeout: 15_000 });
  await expect(a.locator("[data-session-diffstat]")).toContainText("1 file changed");
  await expect(a.locator("[data-session-turn]")).toContainText("Pushed");
  await expect(a.locator("[data-session-stop]")).toBeVisible();

  // Its own page: the card, larger, and its history.
  await alice.goto(`${app}/p/${project}/sessions/${first.id}`);
  const history = alice.locator("[data-session-history] li");
  await expect(history).toHaveCount(6, { timeout: 15_000 });
  await expect(history.first()).toContainText("Asked for");
  await expect(history.nth(2)).toContainText("Asks: Which login page");
  await expect(history.last()).toContainText("The turn finished");
  await expect(alice.locator(".session-card.large [data-session-label]")).toHaveText("done");
});

test("a second session's box goes quiet: stopped responding, and Bob stops it", async () => {
  test.setTimeout(120_000);
  await room(alice);
  const parent = await say(alice, "@arugula add a health check");
  await expect.poll(() => sessionOn(parent)?.state, { timeout: 15_000 }).toBe("queued");
  const second = sessionOn(parent)!;
  const prompt = sql((db) => (db.prepare("SELECT id FROM session_prompts WHERE session_id = ?").get(second.id) as { id: string }).id);
  // Taken, and last heard from 61 s ago: never "working" on the card.
  boxSays(second.id, "state", "working", { prompts: [prompt], block: "4" }, Date.now() - 61_000);
  const a = card(alice, second.id);
  await expect(a.locator("[data-session-label]")).toHaveText("stopped responding", { timeout: 15_000 });
  await expect(a.locator("[data-session-quiet]")).toBeVisible();
  // The platform marks it so too (its job looks within seconds).
  await expect.poll(() => sessionOn(parent)?.state, { timeout: 45_000 }).toBe("unresponsive");
  await expect(a).toHaveAttribute("data-session-state", "unresponsive", { timeout: 15_000 });

  await room(bob);
  const b = card(bob, second.id);
  await expect(b.locator("[data-session-label]")).toHaveText("stopped responding", { timeout: 15_000 });
  await b.locator("[data-session-stop]").click();
  await expect(b.locator("[data-session-label]")).toHaveText("stopped");
  await expect(a.locator("[data-session-label]")).toHaveText("stopped", { timeout: 15_000 });
  await expect(a.locator("[data-session-stop]")).toHaveCount(0);
  expect(sessionOn(parent)?.state).toBe("stopped");
});
