// Asks as forms, answered from the channel (Arugula #214), on the platform
// page: Alice (owner) and Bob (editor) in team Acme, with chat on and two
// hosted workspaces written into control's database (dev with o/app and
// o/web, web with o/web), none chosen for its agents, against an E2B
// nobody answers. No box runs here, so what the box reports is written
// into the database with the live event the platform sends for it, as in
// sessions.spec.ts; what people do goes through the platform.
//
// Bob's `@arugula` names no repo: the ask is in #general itself, a
// mention of Bob, and its buttons are his. He presses one in the channel
// and the session starts on a box with that repo. Its agent asks a
// question with choices: the card in the channel has a button each, and
// Bob's press is the answer. Done, Move to… sends it to the other box.

import { DatabaseSync } from "node:sqlite";
import { randomBytes } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("session-asks");

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
const dev = `hw_${randomBytes(16).toString("hex")}`;
const web = `hw_${randomBytes(16).toString("hex")}`;

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
  state: string;
  repo: string | null;
  workspace_id: string;
}

const sessionOn = (parent: number) =>
  sql((db) => db.prepare("SELECT id, state, repo, workspace_id FROM agent_sessions WHERE parent_seq = ?").get(parent) as unknown as Ses | undefined);

/** What the box would report, as the platform keeps it (sessions.spec.ts). */
function boxSays(ses: string, to: string, detail: Record<string, unknown>) {
  sql((db) => {
    const s = db.prepare("SELECT state, room_id, last_seq, daemon_n FROM agent_sessions WHERE id = ?").get(ses) as unknown as { state: string; room_id: string; last_seq: number; daemon_n: number };
    const now = Date.now();
    const seq = s.last_seq + 1;
    const n = s.daemon_n + 1;
    db.prepare(
      `INSERT INTO session_events (session_id, seq, kind, from_state, to_state, at, detail, daemon, daemon_n, sig)
       VALUES (?, ?, 'state', ?, ?, ?, ?, ?, ?, '00')`,
    ).run(ses, seq, s.state, to, now, JSON.stringify(detail), machine, n);
    db.prepare(
      `UPDATE agent_sessions SET state = ?, last_seq = ?, daemon_n = ?, heard_at = ?, updated_at = ?, machine = ?,
         block = COALESCE(?, block) WHERE id = ?`,
    ).run(to, seq, n, now, now, machine, typeof detail.block === "string" ? detail.block : null, ses);
    const prompts = Array.isArray(detail.prompts) ? (detail.prompts as string[]) : [];
    for (const p of prompts) db.prepare("UPDATE session_prompts SET state = 'sent', sent_at = ? WHERE id = ?").run(now, p);
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

/** #general on the platform page. */
async function room(p: Page) {
  await p.bringToFront();
  await p.goto(`${app}/p/${project}/chat/${general}`);
  await expect(p.locator("[data-chat-control] [data-room-view]")).toBeVisible({ timeout: 15_000 });
}

/** `who`'s unread mentions in #general, as the chat list says. */
async function mentions(p: Page): Promise<number> {
  const r = await api(p, "GET", "/api/chat");
  return ((r.body as { rooms: { id: string; mentions: number }[] }).rooms.find((x) => x.id === general)?.mentions ?? -1) as number;
}

/** The channel's own timeline (not a thread panel). */
const channel = (p: Page) => p.locator("[data-room-view]").first();

test("Acme with chat on, two hosted workspaces, and #general", async ({ browser }) => {
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

  await alice.goto(`${app}/`);
  await expect(alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" })).toBeVisible({ timeout: 15_000 });
  const projects = (await api(alice, "GET", "/api/projects")).body as { me: { account: string }; projects: { id: string; team?: { id: string } | null }[] };
  project = projects.projects.find((p) => p.team?.id === team)!.id;
  const me = projects.me.account;
  const now = Date.now();
  const box = (id: string, name: string, repos: string[]) =>
    sql((db) =>
      db
        .prepare(
          `INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, lease_until, created_by, created_at, updated_at)
           VALUES (?, ?, ?, ?, 'h', 'ready', ?, ?, ?, ?, ?)`,
        )
        .run(id, project, name, JSON.stringify({ repos }), id === dev ? machine : randomBytes(16).toString("hex"), now + 86_400_000, me, now, now),
    );
  box(dev, "dev", ["https://github.com/o/app", "https://github.com/o/web"]);
  box(web, "web", ["https://github.com/o/web"]);
  const ch = await api(alice, "POST", `/api/chat/teams/${team}/channels`, { name: "general" });
  expect(ch.status, JSON.stringify(ch.body)).toBe(200);
  general = (ch.body as { id: string }).id;
  expect((await api(alice, "POST", `/api/chat/rooms/${general}/members`, { accounts: [bobAccount] })).status).toBe(200);
});

let parent = 0;
let ses = "";

test("the which-repo ask is in the channel, Bob's mention, and his press starts the session", async () => {
  test.setTimeout(120_000);
  await room(alice);
  // Signed in on the platform page (its sign-in hop has come back), not in
  // the room.
  await bob.goto(`${app}/`);
  await expect(bob.locator("section[data-platform-home] [data-project]", { hasText: "Acme" })).toBeVisible({ timeout: 15_000 });
  const r = await api(bob, "POST", `/api/chat/rooms/${general}/messages`, { text: "@arugula fix the canary" });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  parent = (r.body as { seq: number }).seq;
  await expect.poll(() => mentions(bob), { timeout: 15_000 }).toBe(1);
  expect(await mentions(alice)).toBe(0);

  // Alice sees it in the channel, waiting on Bob: no buttons of hers.
  const theirs = channel(alice).locator('[data-ask-state="open"]');
  await expect(theirs.locator("[data-ask-label]")).toHaveText("asks bob", { timeout: 15_000 });
  await expect(theirs.locator("[data-ask-choice]")).toHaveCount(0);

  // Bob's, in the channel itself: a button per repo.
  await room(bob);
  const mine = channel(bob).locator('[data-ask-state="open"]');
  await expect(mine.locator("[data-ask-label]")).toHaveText("asks you", { timeout: 15_000 });
  await expect(mine.locator("[data-ask-choice]")).toHaveText(["o/app", "o/web"]);
  await mine.locator('[data-ask-choice="o/web"]').click();
  await expect.poll(() => sessionOn(parent)?.repo, { timeout: 15_000 }).toBe("o/web");
  const s = sessionOn(parent)!;
  ses = s.id;
  expect(s.workspace_id).toBe(dev);
  await expect(channel(bob).locator('[data-ask-state="answered"]')).toBeVisible({ timeout: 15_000 });
  await expect(channel(bob).locator(`[data-session-card="${ses}"]`)).toBeVisible({ timeout: 15_000 });
  await expect(channel(alice).locator('[data-ask-state="answered"]')).toBeVisible({ timeout: 15_000 });
  await expect.poll(() => mentions(bob), { timeout: 15_000 }).toBe(0);
});

test("the agent's question has a button per choice on its card in the channel", async () => {
  test.setTimeout(60_000);
  const prompt = sql((db) => (db.prepare("SELECT id FROM session_prompts WHERE session_id = ?").get(ses) as { id: string }).id);
  boxSays(ses, "working", { prompts: [prompt], block: "3" });
  boxSays(ses, "needs_input", { question: "Which colour?", request: "req_1", ask: "question", options: ["Red", "Blue"] });
  const card = channel(bob).locator(`[data-session-card="${ses}"]`);
  await expect(card.locator("[data-session-label]")).toHaveText("needs input", { timeout: 15_000 });
  await expect(card.locator("[data-ask-choice]")).toHaveText(["Red", "Blue"]);
  await expect(card.locator("[data-ask-words]")).toBeVisible();
  await card.locator('[data-ask-choice="Blue"]').click();
  await expect(card.locator("[data-session-answered]")).toContainText("Answered by bob", { timeout: 15_000 });
  const action = sql((db) => db.prepare("SELECT kind, by_account, request, answer FROM session_actions WHERE session_id = ?").get(ses) as Record<string, string>);
  expect([action.kind, action.by_account, action.request, JSON.parse(action.answer)]).toEqual(["answer", bobAccount, "req_1", { question_0: "Blue" }]);
});

test("done, Move to… sends it to the other box with its repo", async () => {
  test.setTimeout(60_000);
  boxSays(ses, "working", {});
  boxSays(ses, "done", {});
  const card = channel(bob).locator(`[data-session-card="${ses}"]`);
  await expect(card.locator("[data-session-label]")).toHaveText("done", { timeout: 15_000 });
  const menu = card.locator("[data-session-move]");
  await expect(menu.locator("option")).toHaveText(["Move to…", "web (asleep)"]);
  await menu.selectOption(web);
  await expect.poll(() => sessionOn(parent)?.workspace_id, { timeout: 15_000 }).toBe(web);
  expect(sessionOn(parent)?.state).toBe("queued");
  await expect(card.locator("[data-session-label]")).toHaveText("queued", { timeout: 15_000 });
  await expect(card).toContainText("on web");
});
