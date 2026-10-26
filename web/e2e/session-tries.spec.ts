// Try 3 (M264) from a session's card on the platform page: Alice (owner)
// in team Acme, with chat on and one hosted workspace written into
// control's database, against an E2B nobody answers, as sessions.spec.ts
// sets it up. `@arugula` in #general starts a session; no box runs here, so
// what the box reports is written into the database with the live events
// the platform sends for it (`chat_events`), the parent's card's too.
//
// Alice clicks Try 3: the card's compare view lists the session and its two
// siblings (Claude Code and Codex). The box takes them, and each turn
// leaves its diffstat. She picks the Codex one: the other two say "not
// picked", and the kept one's card is in the thread, its PR button hers.

import { DatabaseSync } from "node:sqlite";
import { randomBytes } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("tries");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let app = "";
let team = "";
let project = "";
let general = "";
const machine = randomBytes(16).toString("hex");

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
  state: string;
  last_seq: number;
  daemon_n: number;
  branch: string;
  parent_session: string | null;
}

const sessionOn = (parent: number) =>
  sql((db) => db.prepare("SELECT id, room_id, state, last_seq, daemon_n, branch, parent_session FROM agent_sessions WHERE parent_seq = ? AND parent_session IS NULL").get(parent) as unknown as Ses | undefined);

const siblingsOf = (ses: string) =>
  sql((db) => db.prepare("SELECT id, room_id, state, last_seq, daemon_n, branch, parent_session FROM agent_sessions WHERE parent_session = ? ORDER BY created_at").all(ses) as unknown as Ses[]);

/** What the box would report, as the platform keeps it, and the live
 * events that tell the session's card and its parent's. */
function boxSays(ses: string, kind: "state" | "turn", to: string | null, detail: Record<string, unknown>) {
  sql((db) => {
    const s = db.prepare("SELECT state, room_id, last_seq, daemon_n, parent_session FROM agent_sessions WHERE id = ?").get(ses) as unknown as Ses;
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
         block = COALESCE(?, block) WHERE id = ?`,
    ).run(next, seq, n, now, now, machine, typeof detail.block === "string" ? detail.block : null, ses);
    const prompts = Array.isArray(detail.prompts) ? (detail.prompts as string[]) : [];
    for (const p of prompts) db.prepare("UPDATE session_prompts SET state = 'sent', sent_at = ? WHERE id = ?").run(now, p);
    for (const told of [ses, s.parent_session].filter(Boolean)) {
      db.prepare("INSERT INTO chat_events (kind, body, to_account, at) VALUES ('live', ?, NULL, ?)").run(JSON.stringify({ type: "session", room: s.room_id, ses: told }), now);
    }
  });
}

/** The box takes `ses` (on `harness`), runs a turn that pushes and leaves
 * `diffstat`, and it's done. */
function runs(ses: string, harness: string, diffstat: string) {
  const prompt = sql((db) => (db.prepare("SELECT id FROM session_prompts WHERE session_id = ? AND state = 'queued'").get(ses) as { id: string }).id);
  boxSays(ses, "state", "working", { prompts: [prompt], block: "5", harness });
  const branch = sql((db) => (db.prepare("SELECT branch FROM agent_sessions WHERE id = ?").get(ses) as { branch: string }).branch);
  boxSays(ses, "turn", null, { branch, pushed: true, commits: ["a1b2c3 fix the web login"], diffstat });
  boxSays(ses, "state", "done", {});
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
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
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
});

test("Try 3, then pick one of the three", async () => {
  test.setTimeout(150_000);
  await room(alice);
  const r = await api(alice, "POST", `/api/chat/rooms/${general}/messages`, { text: "@arugula fix the login page" });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const parent = (r.body as { seq: number }).seq;
  await expect.poll(() => sessionOn(parent)?.state, { timeout: 15_000 }).toBe("queued");
  const first = sessionOn(parent)!;
  const a = card(alice, first.id);
  runs(first.id, "claude", " web/login.ts | 4 ++--\n 1 file changed");
  await expect(a.locator("[data-session-label]")).toHaveText("done", { timeout: 15_000 });

  // Try 3, with Claude Code and Codex for the two more (the default).
  await expect(a.locator("[data-session-try-with]")).toHaveValue("codex");
  await a.locator("[data-session-try-go]").click();
  const tries = a.locator("[data-session-tries] [data-session-try]");
  await expect(tries).toHaveCount(3, { timeout: 15_000 });
  await expect(a.locator("[data-session-try-go]")).toHaveCount(0);
  const [s2, s3] = siblingsOf(first.id);
  expect([s2, s3].map((s) => s.state)).toEqual(["queued", "queued"]);
  await expect(a.locator("[data-session-tries-line]")).toContainText("2 starting, 1 done");
  await expect(tries.nth(2).locator("[data-session-try-harness]")).toHaveText("Codex");
  await expect(tries.nth(0).locator("[data-session-try-diffstat]")).toContainText("web/login.ts");

  // The box runs them, the Codex one on Codex; the card hears each.
  runs(s2.id, "claude", " web/login.ts | 9 +++++----\n 1 file changed");
  runs(s3.id, "codex", " web/login.ts | 2 +-\n web/auth.ts | 6 ++++++\n 2 files changed");
  await expect(a.locator("[data-session-tries-line]")).toContainText("3 done", { timeout: 15_000 });
  await expect(tries.nth(2).locator("[data-session-try-diffstat]")).toContainText("web/auth.ts");
  await expect(tries.nth(1).locator("[data-session-try-page]")).toHaveAttribute("href", new RegExp(`/sessions/${s2.id}$`));
  await expect(tries.nth(2).locator("[data-session-try-diff]")).toHaveCount(0); // no repo: no GitHub diff

  // Pick the Codex one: the others aren't picked, and it's the thread's.
  await tries.nth(2).locator("[data-session-pick]").click();
  await expect(tries.nth(2).locator("[data-session-try-label]")).toHaveText("kept", { timeout: 15_000 });
  await expect(tries.nth(0).locator("[data-session-try-label]")).toHaveText("not picked");
  await expect(tries.nth(1).locator("[data-session-try-label]")).toHaveText("not picked");
  await expect(a.locator("[data-session-pick]")).toHaveCount(0);
  await expect(a.locator("[data-session-label]")).toHaveText("not picked");
  expect(siblingsOf(first.id).map((s) => s.state)).toEqual(["stopped", "done"]);
  expect(sessionOn(parent)!.state).toBe("stopped");
  // Its card, in the thread and the timeline: its PR is Alice's to send.
  const kept = card(alice, s3.id);
  await expect(kept.locator("[data-session-label]")).toHaveText("done", { timeout: 15_000 });
  await expect(kept.locator("[data-session-parent]")).toContainText(": kept.");
  await expect(kept.locator("button[data-session-pr]")).toBeEnabled();
  await expect(kept.locator("button[data-session-pr]")).toHaveText("PR…");
});
