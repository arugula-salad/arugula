// Fleet 5 on the platform page: a session's card shows live on an issue
// page that's open (#242's edge: it showed only after a reload), and a
// project's Sessions list a wave of them. Alice (owner) and Bob (editor) in
// team Acme, chat on, one hosted workspace written into control's database,
// against an E2B nobody answers; what the box reports is written into the
// database with its live event, as in sessions.spec.ts.
//
// Bob has issue #1 open when Alice presses Agent on this on it (through the
// platform): the card comes to Bob's page without a reload. Bob presses it
// on #2 himself: its card is there when the button's done. Then Sessions:
// both sessions, narrowed by label, and Bob's own once one asks him a
// question, live.

import { DatabaseSync } from "node:sqlite";
import { randomBytes } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("session-wave");

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
let bobAccount = "";
const machine = randomBytes(16).toString("hex");
const issues: { id: string; number: number }[] = [];
const sessions: string[] = [];

function sql<T>(f: (db: DatabaseSync) => T): T {
  const db = new DatabaseSync(t.db);
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    return f(db);
  } finally {
    db.close();
  }
}

/** The box's report on `ses`, as the platform keeps it, and its live event. */
function boxSays(ses: string, to: string, detail: Record<string, unknown>) {
  sql((db) => {
    const s = db
      .prepare(
        "SELECT state, room_id, last_seq, daemon_n FROM agent_sessions WHERE id = ?",
      )
      .get(ses) as {
      state: string;
      room_id: string;
      last_seq: number;
      daemon_n: number;
    };
    const now = Date.now();
    const seq = s.last_seq + 1;
    const n = s.daemon_n + 1;
    db.prepare(
      `INSERT INTO session_events (session_id, seq, kind, from_state, to_state, at, detail, daemon, daemon_n, sig)
       VALUES (?, ?, 'state', ?, ?, ?, ?, ?, ?, '00')`,
    ).run(ses, seq, s.state, to, now, JSON.stringify(detail), machine, n);
    db.prepare(
      "UPDATE agent_sessions SET state = ?, last_seq = ?, daemon_n = ?, heard_at = ?, updated_at = ?, machine = ? WHERE id = ?",
    ).run(to, seq, n, now, now, machine, ses);
    const prompts = Array.isArray(detail.prompts)
      ? (detail.prompts as string[])
      : [];
    for (const p of prompts)
      db.prepare(
        "UPDATE session_prompts SET state = 'sent', sent_at = ? WHERE id = ?",
      ).run(now, p);
    db.prepare(
      "INSERT INTO chat_events (kind, body, to_account, at) VALUES ('live', ?, NULL, ?)",
    ).run(JSON.stringify({ type: "session", room: s.room_id, ses }), now);
  });
}

async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, {
        method: m as string,
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: b === undefined ? undefined : JSON.stringify(b),
      });
      return {
        status: r.status,
        body: (await r.json().catch(() => null)) as unknown,
      };
    },
    [method, path, body] as const,
  );
}

async function issuePage(p: Page, i: { id: string; number: number }) {
  await p.bringToFront();
  await p.goto(`${app}/p/${project}/issues/${i.id}`);
  // Its thread is open: the composer is there.
  await expect(
    p
      .locator(`[data-issue="${i.id}"] [data-issue-thread]`)
      .getByRole("textbox", { name: `Message #${i.number}` }),
  ).toBeVisible({ timeout: 15_000 });
}

test("Acme with chat on, a hosted workspace, and two issues", async ({
  browser,
}) => {
  test.setTimeout(180_000);
  await t.start([
    "--platform-host",
    "app.localhost",
    "--hosted-accounts",
    "*",
    "--e2b-api-key",
    "e2e-nobody",
    "--e2b-url",
    "http://127.0.0.1:9",
  ]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const link = await alice.evaluate(
    (tm) => window.__arugula.control!.invite(tm, "editor", true),
    team,
  );
  bob = await t.person(browser, "bob");
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await expect(bob.locator("[data-invite-pending]")).toBeVisible();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({
    timeout: 15_000,
  });
  await alice.locator("[data-admit-yes]").click();
  await expect
    .poll(
      () =>
        alice.evaluate(
          () => window.__arugula.control!.teams[0].roster.members.length,
        ),
      { timeout: 15_000 },
    )
    .toBe(2);
  bobAccount = await alice.evaluate(
    () =>
      window.__arugula.control!.teams[0].roster.members.find(
        (m) => m.name === "bob",
      )!.account,
  );
  await alice.evaluate(
    (tm) => window.__arugula.control!.setLabsFlag("chat", true, tm),
    team,
  );

  await alice.goto(`${app}/`);
  await expect(
    alice.locator("section[data-platform-home] [data-project]", {
      hasText: "Acme",
    }),
  ).toBeVisible({ timeout: 15_000 });
  const projects = (await api(alice, "GET", "/api/projects")).body as {
    me: { account: string };
    projects: { id: string; team?: { id: string } | null }[];
  };
  project = projects.projects.find((p) => p.team?.id === team)!.id;
  const now = Date.now();
  sql((db) =>
    db
      .prepare(
        `INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, lease_until, created_by, created_at, updated_at)
         VALUES (?, ?, 'dev', '{}', 'h', 'ready', ?, ?, ?, ?, ?)`,
      )
      .run(
        `hw_${randomBytes(16).toString("hex")}`,
        project,
        machine,
        now + 86_400_000,
        projects.me.account,
        now,
        now,
      ),
  );
  for (const [title, labels] of [
    ["Fix the login page", ["fleet"]],
    ["Add a health check", []],
  ] as const) {
    const r = await api(alice, "POST", `/api/projects/${project}/issues`, {
      title,
      labels,
    });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    issues.push(r.body as { id: string; number: number });
  }
  await bob.goto(`${app}/`);
  await expect(
    bob.locator("section[data-platform-home] [data-project]", {
      hasText: "Acme",
    }),
  ).toBeVisible({ timeout: 15_000 });
});

test("a card comes to an open issue page live, and to the page that pressed Agent on this", async () => {
  test.setTimeout(90_000);
  // Bob watches #1; Alice's Agent on this starts its session elsewhere.
  await issuePage(bob, issues[0]);
  const r = await api(alice, "POST", `/api/issues/${issues[0].id}/agent`);
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const ses = (r.body as { session: string }).session;
  expect(ses).toMatch(/^ses_/);
  sessions.push(ses);
  const card = bob.locator(`[data-issue-thread] [data-session-card="${ses}"]`);
  await expect(card).toHaveAttribute("data-session-state", "queued", {
    timeout: 15_000,
  });

  // Bob presses it on #2: the card is there once the button's done.
  await issuePage(bob, issues[1]);
  await bob.locator("button[data-issue-agent]").click();
  await expect
    .poll(
      () =>
        sql(
          (db) =>
            (
              db
                .prepare(
                  "SELECT id FROM agent_sessions WHERE asker = ? ORDER BY created_at DESC",
                )
                .get(bobAccount) as { id: string } | undefined
            )?.id,
        ),
      { timeout: 15_000 },
    )
    .toBeTruthy();
  const mine = sql(
    (db) =>
      (
        db
          .prepare(
            "SELECT id FROM agent_sessions WHERE asker = ? ORDER BY created_at DESC",
          )
          .get(bobAccount) as { id: string }
      ).id,
  );
  sessions.push(mine);
  await expect(
    bob.locator(`[data-issue-thread] [data-session-card="${mine}"]`),
  ).toBeVisible({ timeout: 5_000 });
});

test("Sessions: the wave, narrowed by label, says what waits on Bob, live", async () => {
  test.setTimeout(90_000);
  await bob.goto(`${app}/p/${project}`);
  await bob.locator("[data-platform-sessions]").click();
  await expect(bob).toHaveURL(new RegExp(`/p/${project}[^/]*/sessions$`));
  const rows = bob.locator("[data-sessions-list] [data-session-row]");
  await expect(rows).toHaveCount(2, { timeout: 15_000 });
  await expect(rows.first()).toHaveAttribute("data-session-row", sessions[1]);
  await expect(rows.first()).toContainText(
    `#${issues[1].number} Add a health check`,
  );
  await expect(rows.first()).toContainText("on dev");
  await expect(rows.first()).toContainText("not pushed");
  await expect(rows.first()).toContainText("asked by bob");

  await bob.locator("[data-sessions-label]").click();
  await bob.getByRole("menuitemradio", { name: "fleet" }).click();
  await expect(bob).toHaveURL(/sessions\?label=fleet$/);
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toHaveAttribute("data-session-row", sessions[0]);
  await bob.locator("[data-sessions-clear]").click();
  await expect(rows).toHaveCount(2);

  // Alice's session asks: it waits on Bob (an editor), without a reload.
  const prompt = sql(
    (db) =>
      (
        db
          .prepare("SELECT id FROM session_prompts WHERE session_id = ?")
          .get(sessions[0]) as { id: string }
      ).id,
  );
  boxSays(sessions[0], "working", { prompts: [prompt], block: "3" });
  boxSays(sessions[0], "needs_input", {
    question: "Which login page: web or admin?",
    request: "req_1",
    ask: "question",
  });
  const row = bob.locator(`[data-session-row="${sessions[0]}"]`);
  await expect(row.locator("[data-session-waiting]")).toHaveText(
    "Waits on you: its question",
    { timeout: 15_000 },
  );
  await expect(row).toContainText("It asks: Which login page: web or admin?");
  await expect(bob.locator("[data-sessions-stats]")).toContainText(
    "1 waiting on you",
  );
});
