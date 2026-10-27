// What an agent session says in its thread (Arugula #215), on the platform
// page: Alice owns team Acme, with chat on and one hosted workspace written
// into control's database, against an E2B nobody answers. Her ask in
// #general starts a session (M256). No box runs here, so what the platform
// posts from the box's reports is written into the database as it leaves
// it: a turn's answer and a review, each the session's own message (no
// author, `via` `said:<ses>`), and the platform's line for a turn someone
// typed in the session's block.
//
// The answer and the review are drawn as the session's, not Alice's: its
// agent and short id, linked to its page, a badge, a mark in place of a
// person's picture. The typed turn is a plain line, its words as written.

import { DatabaseSync } from "node:sqlite";
import { randomBytes } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("session-said");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

/** The word that starts a session (never written out in fixtures). */
const MENTION = "arugula";

let alice: Page;
let app = "";
let team = "";
let project = "";
let general = "";
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

async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as unknown };
    },
    [method, path, body] as const,
  );
}

/** A reply in #general's thread `parent`, as the platform posts its own:
 * with no mentions, and the live event that tells the page. */
function posted(parent: number, author: string, via: string, text: string): number {
  return sql((db) => {
    const now = Date.now();
    const { last_seq: seq } = db.prepare("UPDATE chat_rooms SET last_seq = last_seq + 1 WHERE id = ? RETURNING last_seq").get(general) as { last_seq: number };
    db.prepare("INSERT INTO chat_messages (room, seq, author, at, text, mentions, via, parent, also) VALUES (?, ?, ?, ?, ?, '[]', ?, ?, 0)").run(general, seq, author, now, text, via, parent);
    db.prepare("INSERT INTO chat_events (kind, body, to_account, at) VALUES ('live', ?, NULL, ?)").run(JSON.stringify({ type: "reply", room: general, parent, seq }), now);
    return seq;
  });
}

const said = (ses: string) => alice.locator(`[data-thread-panel] [data-said="${ses}"]`);

test("Acme with chat on, a hosted workspace, and #general", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost", "--hosted-accounts", "*", "--e2b-api-key", "e2e-nobody", "--e2b-url", "http://127.0.0.1:9"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await alice.goto(`${app}/`);
  await expect(alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" })).toBeVisible({ timeout: 15_000 });
  const projects = (await api(alice, "GET", "/api/projects")).body as { me: { account: string }; projects: { id: string; team?: { id: string } | null }[] };
  project = projects.projects.find((p) => p.team?.id === team)!.id;
  const now = Date.now();
  sql((db) =>
    db
      .prepare(
        `INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, lease_until, created_by, created_at, updated_at)
         VALUES (?, ?, 'dev', '{}', 'h', 'ready', ?, ?, ?, ?, ?)`,
      )
      .run(`hw_${randomBytes(16).toString("hex")}`, project, machine, now + 86_400_000, projects.me.account, now, now),
  );
  const ch = await api(alice, "POST", `/api/chat/teams/${team}/channels`, { name: "general" });
  expect(ch.status, JSON.stringify(ch.body)).toBe(200);
  general = (ch.body as { id: string }).id;
});

test("a session's answer and its review are the session's, and a turn typed in its block is a plain line", async () => {
  test.setTimeout(120_000);
  const r = await api(alice, "POST", `/api/chat/rooms/${general}/messages`, { text: `@${MENTION} is the README up to date?` });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const parent = (r.body as { seq: number }).seq;
  await expect.poll(() => sql((db) => db.prepare("SELECT id FROM agent_sessions WHERE parent_seq = ?").get(parent)), { timeout: 15_000 }).toBeTruthy();
  const ses = sql((db) => (db.prepare("SELECT id FROM agent_sessions WHERE parent_seq = ?").get(parent) as { id: string }).id);
  const short = ses.slice(4, 12);
  // Its review: a read-only session under it, on Codex, done.
  const review = `ses_${randomBytes(16).toString("hex")}`;
  sql((db) =>
    db
      .prepare(
        `INSERT INTO agent_sessions (id, project_id, room_id, parent_seq, asker, workspace_id, branch, state, created_at, updated_at, parent_session, role, harness)
         SELECT ?, project_id, room_id, parent_seq, asker, workspace_id, branch, 'stopped', created_at, updated_at, id, 'review', 'codex' FROM agent_sessions WHERE id = ?`,
      )
      .run(review, ses),
  );

  await alice.goto(`${app}/p/${project}/chat/${general}?thread=${parent}`);
  await expect(alice.locator("[data-thread-panel]")).toBeVisible({ timeout: 15_000 });
  posted(parent, "", `said:${ses}`, "Mostly. The install steps are **stale**: @alice should I fix them?");
  posted(parent, "", "system", "Asked in the session by alice: “and the **changelog** too?”");
  posted(parent, "", `said:${review}`, `**Review of agent session ${short}**\n\nREADME.md:12 still says \`make\`.`);

  // The answer: the session's agent and short id, linked to its page, a
  // badge, its mark; no person's name, picture or "via".
  const answer = said(ses).first();
  await expect(answer.locator(".said-name")).toHaveText(`Claude Code · session ${short}`, { timeout: 15_000 });
  await expect(answer.locator("[data-said-page]")).toHaveAttribute("href", new RegExp(`/sessions/${ses}$`));
  await expect(answer.locator(".said-badge")).toHaveText("agent session");
  await expect(answer.locator("[data-said-avatar]")).toHaveText("CC");
  await expect(answer).toHaveClass(/by-session/);
  await expect(answer.locator(".via-badge, .msg-avatar img")).toHaveCount(0);
  await expect(answer).not.toContainText("alice via");
  // What the agent wrote is still its words: Markdown, an @ that's text.
  await expect(answer.locator("strong")).toHaveText("stale");

  // The review: Codex's, a review.
  const rev = said(review).first();
  await expect(rev.locator(".said-name")).toHaveText(`Codex review · session ${review.slice(4, 12)}`, { timeout: 15_000 });
  await expect(rev.locator(".said-badge")).toHaveText("review");
  await expect(rev.locator("[data-said-avatar]")).toHaveText("Cx");

  // The typed turn: the platform's plain line, its words as written.
  const line = alice.locator("[data-thread-panel] [data-system]", { hasText: "Asked in the session by alice" });
  await expect(line).toContainText("“and the **changelog** too?”");
  await expect(line.locator("strong")).toHaveCount(0);

  // Its card is still the card, not a said message.
  await alice.goto(`${app}/p/${project}/chat/${general}`);
  await expect(alice.locator(`[data-room-view] [data-session-card="${ses}"]`).first()).toBeVisible({ timeout: 15_000 });
  await expect(alice.locator("[data-room-view] [data-said]")).toHaveCount(0);
});
