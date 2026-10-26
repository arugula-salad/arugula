// M265 (thread = session) on the platform page: Alice owns team Acme, with
// chat on, one hosted workspace that clones o/app (linked to the project),
// written into control's database, against an E2B nobody answers.
//
// - A session's PR merges: what the box reports, and what the platform
//   does when the mirror sees the merge (no GitHub App runs here), are
//   written into the database with the live event that tells the card, as
//   they'd leave them. Before its PR, the card offers the pushed branch's
//   diff (no editor: the box's editor can't open a session's checkout);
//   once merged it says "merged ✓", and the thread leaves the Threads view.
// - The project page counts its sessions, PRs opened and merged, and the
//   median from the `@` to the PR.
// - Agent on this on a native issue writes `@arugula` in its thread as
//   Alice: the session's card is in the issue's comments, its branch named
//   for the issue.

import { DatabaseSync } from "node:sqlite";
import { randomBytes } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("session-done");

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
  branch: string;
  state: string;
  last_seq: number;
  daemon_n: number;
}

const sessionOn = (room: string, parent: number) =>
  sql((db) => db.prepare("SELECT id, room_id, branch, state, last_seq, daemon_n FROM agent_sessions WHERE room_id = ? AND parent_seq = ?").get(room, parent) as unknown as Ses | undefined);

/** A move kept as the platform keeps it, and the live event for its card:
 * the box's (`daemon`) or control's own. */
function moved(ses: string, kind: "state" | "turn", to: string | null, detail: Record<string, unknown>, daemon: boolean, also = "") {
  sql((db) => {
    const s = db.prepare("SELECT state, room_id, last_seq, daemon_n FROM agent_sessions WHERE id = ?").get(ses) as unknown as Ses;
    const now = Date.now();
    const seq = s.last_seq + 1;
    const n = daemon ? s.daemon_n + 1 : null;
    const next = to ?? s.state;
    db.prepare(
      `INSERT INTO session_events (session_id, seq, kind, from_state, to_state, at, detail, daemon, daemon_n, sig)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(ses, seq, kind, s.state, next, now, JSON.stringify(detail), daemon ? machine : null, n, daemon ? "00" : null);
    db.prepare(`UPDATE agent_sessions SET state = ?, last_seq = ?, daemon_n = COALESCE(?, daemon_n), heard_at = ?, updated_at = ?, machine = ?${also} WHERE id = ?`).run(
      next,
      seq,
      n,
      now,
      now,
      machine,
      ses,
    );
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

const card = (p: Page, ses: string) => p.locator(`[data-session-card="${ses}"]`).first();

test("Acme with chat on, a hosted workspace on o/app, and #general", async ({ browser }) => {
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
  sql((db) => {
    db.prepare(
      `INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, lease_until, created_by, created_at, updated_at)
       VALUES (?, ?, 'dev', ?, 'h', 'ready', ?, ?, ?, ?, ?)`,
    ).run(`hw_${randomBytes(16).toString("hex")}`, project, JSON.stringify({ repos: ["https://github.com/o/app"] }), machine, now + 86_400_000, projects.me.account, now, now);
    db.prepare("INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at) VALUES (7, 'o/app', 0, ?, 1, ?)").run(project, now);
  });
  const ch = await api(alice, "POST", `/api/chat/teams/${team}/channels`, { name: "general" });
  expect(ch.status, JSON.stringify(ch.body)).toBe(200);
  general = (ch.body as { id: string }).id;
});

let first: Ses;

test("a pushed session offers its diff, and its PR merging marks it merged ✓", async () => {
  test.setTimeout(120_000);
  await alice.goto(`${app}/p/${project}/chat/${general}`);
  await expect(alice.locator("[data-chat-control] [data-room-view]")).toBeVisible({ timeout: 15_000 });
  const asked = await api(alice, "POST", `/api/chat/rooms/${general}/messages`, { text: "@arugula speed up the login" });
  expect(asked.status, JSON.stringify(asked.body)).toBe(200);
  const parent = (asked.body as { seq: number }).seq;
  await expect.poll(() => sessionOn(general, parent)?.state, { timeout: 15_000 }).toBe("queued");
  first = sessionOn(general, parent)!;
  // Alice follows the thread she asked in; its card is a reply there.
  await expect.poll(async () => ((await api(alice, "GET", "/api/chat/threads")).body as { threads: unknown[] }).threads.length, { timeout: 15_000 }).toBe(1);

  const prompt = sql((db) => (db.prepare("SELECT id FROM session_prompts WHERE session_id = ?").get(first.id) as { id: string }).id);
  sql((db) => db.prepare("UPDATE session_prompts SET state = 'sent', sent_at = ? WHERE id = ?").run(Date.now(), prompt));
  moved(first.id, "state", "working", { prompts: [prompt], block: "3" }, true, ", block = '3'");
  moved(first.id, "turn", null, { branch: first.branch, pushed: true, commits: ["a1b2c3 cache the session lookup"], diffstat: " web/login.ts | 12 ++++++++----\n 1 file changed" }, true);
  moved(first.id, "state", "done", {}, true);
  const a = card(alice, first.id);
  await expect(a.locator("[data-session-label]")).toHaveText("done", { timeout: 15_000 });
  const diff = a.locator("[data-session-diff]");
  await expect(diff).toHaveAttribute("href", `https://github.com/o/app/compare/${first.branch}`);
  await a.scrollIntoViewIfNeeded();
  await alice.screenshot({ path: test.info().outputPath("before-merge.png") });

  // Someone opened its PR from the branch and merged it: the mirror has
  // the PR, and the platform stopped the session, saying so.
  const opened = Date.now() - 25 * 60_000;
  sql((db) => {
    db.prepare("UPDATE agent_sessions SET created_at = ? WHERE id = ?").run(opened - 14 * 60_000, first.id);
    db.prepare(
      `INSERT INTO forge_items (id, repo_id, number, kind, state, title, head_ref, created_at, updated_at, synced_at)
       VALUES ('fi_e2e12', 7, 12, 'pr', 'merged', 'Cache the session lookup', ?, ?, ?, ?)`,
    ).run(first.branch, opened, Date.now(), Date.now());
  });
  moved(first.id, "state", "stopped", { reason: "merged: its pull request merged", pr: 12 }, false, `, merged_at = ${Date.now()}, pr = 'https://github.com/o/app/pull/12'`);
  await expect(a.locator("[data-session-label]")).toHaveText("merged ✓", { timeout: 15_000 });
  await expect(a).toHaveClass(/tone-ok/);
  await expect(a.locator("[data-session-diff]")).toHaveCount(0);
  await expect(a.locator("a[data-session-pr]")).toHaveAttribute("href", "https://github.com/o/app/pull/12");
  await expect(a.locator("[data-session-stop]")).toHaveCount(0);
  await alice.screenshot({ path: test.info().outputPath("after-merge.png") });
  // The thread is done: it leaves the Threads view.
  const threads = (await api(alice, "GET", "/api/chat/threads")).body as { threads: unknown[] };
  expect(threads.threads).toHaveLength(0);

  // Its own page's history says why it ended.
  await alice.goto(`${app}/p/${project}/sessions/${first.id}`);
  await expect(alice.locator("[data-session-history] li").last()).toContainText("Its pull request merged: done", { timeout: 15_000 });
});

test("the project page counts its sessions, PRs and merges", async () => {
  test.setTimeout(60_000);
  await alice.goto(`${app}/p/${project}`);
  const stats = alice.locator("[data-session-stats]");
  await expect(stats).toBeVisible({ timeout: 15_000 });
  await expect(stats.locator("[data-session-stat=sessions] b")).toHaveText("1");
  await expect(stats.locator("[data-session-stat=opened] b")).toHaveText("1");
  await expect(stats.locator("[data-session-stat=merged] b")).toHaveText("1");
  await expect(stats.locator("[data-session-stat=median]")).toHaveText("Median from @arugula to its PR: 14 min.");
  await stats.scrollIntoViewIfNeeded();
  await alice.screenshot({ path: test.info().outputPath("project-stats.png") });
});

test("Agent on this starts a session in the issue's thread, named for the issue", async () => {
  test.setTimeout(90_000);
  const made = await api(alice, "POST", `/api/projects/${project}/issues`, { title: "Login is slow on Safari", body: "It takes 9 s to sign in.", labels: [], assignees: [] });
  expect(made.status, JSON.stringify(made.body)).toBe(200);
  const issue = made.body as { id: string; number: number };
  await alice.goto(`${app}/p/${project}/issues/${issue.id}`);
  const view = alice.locator("[data-issue]");
  await expect(view.locator("[data-issue-title]")).toHaveText("Login is slow on Safari", { timeout: 15_000 });
  await alice.screenshot({ path: test.info().outputPath("issue-before.png") });
  await view.locator("[data-issue-agent]").click();
  await expect(view.locator(".thread-msg", { hasText: "@arugula work on this issue" })).toBeVisible({ timeout: 15_000 });
  const room = (await api(alice, "POST", "/api/chat/targets", { kind: "object", target: issue.id })).body as { id: string };
  await expect.poll(() => sessionOn(room.id, 0)?.state, { timeout: 15_000 }).toBe("queued");
  const s = sessionOn(room.id, 0)!;
  expect(s.branch).toBe(`arugula/${s.id.slice(4, 12)}-i${issue.number}-login-is-slow-on-safari`);
  await expect(card(alice, s.id).locator("[data-session-branch]")).toHaveText(s.branch, { timeout: 15_000 });
  await alice.screenshot({ path: test.info().outputPath("issue-after.png") });
  // One at a time: a second press says one is going.
  await view.locator("[data-issue-agent]").click();
  await expect(alice.locator(".platform-toast")).toContainText("an agent session is going on this issue", { timeout: 10_000 });
});
