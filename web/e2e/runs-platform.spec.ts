// Runs on the platform page (M215): a team's runs list, a run started by
// hand, its view (who started it, the exact command and its operation's
// hash, its log, its timeline) and its thread; a running run's live pane on
// control's page; a run that fails in the inbox, which opens it; the specs'
// list with a schedule added and removed; and the run at a phone's width.
// A local control (team-fixture.ts, `--platform-host app.localhost`), with
// hosted workspaces on for everyone against an E2B nobody answers, and a
// hosted workspace written into its database: no box runs here, so the
// run that's running is written in too, and the one started by hand fails
// once its workspace's machine changes (the job's "not_hosted"). Runs' logs
// and artifacts (M213) go to a fake S3 here: the running run's log chunks
// and its artifact are put there and recorded in the database, as its
// machine's uploads would leave them.

import { DatabaseSync } from "node:sqlite";
import { createHash, randomBytes } from "node:crypto";
import { createServer, type Server } from "node:http";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("runs");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
  s3?.close();
});

/** The run-log bucket: path-style `GET /<bucket>/<key>` of what the spec
 * put, whatever the signature (control's is checked by M213's own tests). */
const objects = new Map<string, Buffer>();
let s3: Server | undefined;
async function fakeS3(): Promise<string> {
  s3 = createServer((req, res) => {
    const key = decodeURIComponent(new URL(req.url!, "http://s3").pathname).replace(/^\/runs\//, "");
    const body = req.method === "GET" ? objects.get(key) : undefined;
    if (!body) return res.writeHead(404).end();
    res.writeHead(200, { "content-type": "application/octet-stream", "content-length": body.length }).end(body);
  });
  await new Promise<void>((r) => s3!.listen(0, "127.0.0.1", r));
  return `http://127.0.0.1:${(s3.address() as { port: number }).port}`;
}

const sha = (b: Buffer) => createHash("sha256").update(b).digest("hex");

/** A run's log chunk `n` from `offset`, in the bucket and recorded. */
function logChunk(run: string, n: number, offset: number, text: string) {
  const body = Buffer.from(text);
  const key = `${project}/${run}/log/${n}`;
  objects.set(key, body);
  sql((db) =>
    db
      .prepare("INSERT INTO run_log_chunks (run_id, n, byte_offset, size, sha256, bucket_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
      .run(run, n, offset, body.length, sha(body), key, Date.now()),
  );
  return offset + body.length;
}

let alice: Page;
let app = "";
let project = "";
let machine = "";
let started = "";
let running = "";

const hex = () => randomBytes(16).toString("hex");

/** Write to control's database beside it. */
function sql(f: (db: DatabaseSync) => void) {
  const db = new DatabaseSync(t.db);
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    f(db);
  } finally {
    db.close();
  }
}

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

test("a team's runs: one started by hand, its view and its thread", async ({ browser }) => {
  test.setTimeout(180_000);
  const bucket = await fakeS3();
  await t.start([
    ...["--platform-host", "app.localhost", "--hosted-accounts", "*", "--e2b-api-key", "e2e-nobody", "--e2b-url", "http://127.0.0.1:9"],
    ...["--run-logs-bucket", "runs", "--run-logs-endpoint", bucket, "--run-logs-access-key-id", "e2e", "--run-logs-secret-access-key", "e2e-nobody"],
  ]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);

  await platform(alice);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];
  const me = await alice.evaluate(async () => ((await (await fetch("/api/projects")).json()) as { me: { account: string } }).me.account);

  // A hosted workspace whose box enrolled; its lease is far off, so the
  // hosted worker leaves it be.
  machine = `m${hex()}`;
  const now = Date.now();
  sql((db) =>
    db
      .prepare(
        `INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, lease_until, created_by, created_at, updated_at)
         VALUES (?, ?, 'dev', '{}', 'h', 'ready', ?, ?, ?, ?, ?)`,
      )
      .run(`hw_${hex()}`, project, machine, now + 86_400_000, me, now, now),
  );

  await alice.locator("[data-platform-runs]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/runs$/);
  await expect(alice.locator("[data-runs-none]")).toContainText("No runs yet");

  // A spec, with a schedule added and taken off again.
  await alice.locator("[data-run-spec-new]").click();
  await alice.locator("[data-run-spec-name]").fill("nightly");
  await alice.locator("[data-run-spec-command]").fill("make nightly");
  await alice.locator("[data-run-spec-form] button[type=submit]").click();
  const spec = alice.locator('[data-run-spec="nightly"]');
  await expect(spec).toContainText("make nightly");
  await spec.locator("[data-run-schedule-add]").click();
  await spec.locator("[data-run-schedule-cron]").fill("15 4 * * *");
  await spec.locator("form button[type=submit]").click();
  await expect(spec.locator("[data-run-schedule]")).toContainText("15 4 * * *");
  await spec.locator("[data-run-schedule-delete]").click();
  // It asks first (UI audit #158).
  await alice.locator("[data-confirm-remove]").click();
  await expect(spec.locator("[data-run-schedule]")).toHaveCount(0);

  // A command, by hand: its view says what it runs and who started it.
  await alice.locator("[data-run-start-open]").click();
  await alice.locator("[data-run-start-command-mode]").click();
  await alice.locator("[data-run-start-command]").fill(`sh -c "echo hi && exit 2"`);
  await alice.locator("[data-run-start-go]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/runs\/run_[0-9a-f]{32}$/, { timeout: 15_000 });
  started = /\/runs\/(run_[0-9a-f]{32})/.exec(alice.url())![1];
  const view = alice.locator(`[data-run="${started}"]`);
  await expect(view.locator("[data-run-state-label]")).toHaveText("Pending");
  await expect(view.locator("[data-run-command]")).toHaveText(`sh -c 'echo hi && exit 2'`);
  await expect(view.locator("[data-run-op-hash]")).toHaveText(/^[0-9a-f]{64}$/);
  await expect(view.locator("[data-run-started-by]")).toContainText("By hand, by alice");
  await expect(view.locator("[data-run-live]")).toContainText("Waiting for its cloud workspace");
  await expect(view.locator("[data-run-event]")).toHaveCount(1);
  // No box runs here, so the platform keeps no log of it: the page says so.
  await expect(view.locator("[data-run-log-note]")).toContainText("keeps no log");

  // Its thread.
  const box = view.locator("[data-run-thread] .thread-compose textarea");
  await box.fill("watching this one");
  await box.press("Enter");
  await expect(view.locator("[data-run-thread] .thread-msg").last()).toContainText("watching this one");
});

test("a running run's pane is on control's page; the list narrows by state", async () => {
  test.setTimeout(60_000);
  // One its machine took: written as its report would leave it.
  running = `run_${hex()}`;
  const now = Date.now();
  const op = `arugula op v1\ndaemon ${machine}\nkind run\nsession -\npane - -\ncwd -\nargv ${Buffer.from("cargo").toString("base64")}\nargv ${Buffer.from("test").toString("base64")}\n`;
  sql((db) => {
    db.prepare(
      `INSERT INTO runs (id, project_id, workspace_id, trigger_kind, trigger_source, op, op_hash, daemon, state, started_by, last_seq, created_at, started_at)
       VALUES (?, ?, NULL, 'manual', '{}', ?, ?, ?, 'running', '', 2, ?, ?)`,
    ).run(running, project, op, "ab".repeat(32), machine, now - 60_000, now - 50_000);
    db.prepare("INSERT INTO run_events (run_id, seq, from_state, to_state, at, detail) VALUES (?, 1, NULL, 'pending', ?, '{}')").run(running, now - 60_000);
    db.prepare("INSERT INTO run_events (run_id, seq, from_state, to_state, at, detail, daemon, sig) VALUES (?, 2, 'pending', 'running', ?, ?, ?, 'sig')").run(
      running,
      now - 50_000,
      JSON.stringify({ pane: 3, epoch: 1 }),
      machine,
    );
  });
  await alice.locator("[data-run-back]").click();
  await expect(alice.locator("[data-run-row]")).toHaveCount(2);
  await alice.locator("[data-runs-state]").click();
  await alice.getByRole("menuitemradio", { name: "Running" }).click();
  await expect(alice).toHaveURL(/\/runs\?state=running$/);
  await expect(alice.locator("[data-run-row]")).toHaveCount(1);
  await alice.locator(`[data-run-row="${running}"] a`).click();
  const view = alice.locator(`[data-run="${running}"]`);
  await expect(view.locator("[data-run-command]")).toHaveText("cargo test");
  const pane = view.locator("[data-run-pane]");
  await expect(pane).toHaveText(/pane %3/);
  expect(await pane.getAttribute("href")).toMatch(new RegExp(`/#pane=${machine}\\.3$`));
  await expect(view.locator("[data-run-event]")).toHaveText([/made it/, /its machine \(signed\) moved it to running · in pane %3/]);
});

test("a run's kept log, its tail first then earlier, followed while it runs; its artifact downloads", async () => {
  test.setTimeout(60_000);
  // 80 KB of log in two chunks: more than a page (64 KiB), so the tail
  // comes first and Earlier reads the rest.
  const lines = Array.from({ length: 4000 }, (_, i) => `test case ${String(i).padStart(4, "0")} ... ok\n`).join("");
  let end = logChunk(running, 0, 0, lines);
  end = logChunk(running, 1, end, "running 4000 tests: so far so good\n");
  const report = Buffer.from("<testsuite tests=\"4000\" failures=\"0\"/>\n");
  objects.set(`${project}/${running}/artifacts/junit.xml`, report);
  sql((db) =>
    db
      .prepare("INSERT INTO run_artifacts (run_id, name, size, digest, bucket_key, created_at) VALUES (?, 'junit.xml', ?, ?, ?, ?)")
      .run(running, report.length, `sha256:${sha(report)}`, `${project}/${running}/artifacts/junit.xml`, Date.now()),
  );
  await alice.reload();
  const view = alice.locator(`[data-run="${running}"]`);
  const log = view.locator("[data-run-log-text]");
  await expect(view.locator("[data-run-log]")).toHaveAttribute("data-run-log", "kept");
  await expect(log).toContainText("so far so good");
  await expect(log).not.toContainText("test case 0000 ");
  await view.locator("[data-run-log-earlier]").click();
  await expect(log).toContainText("test case 0000 ... ok");
  await expect(view.locator("[data-run-log-earlier]")).toHaveCount(0);
  // It follows what its machine sends next, shown as text: its colors go.
  logChunk(running, 2, end, "test result: \x1b[32mok\x1b[0m. 4000 passed\r\n");
  await expect(log).toContainText("test result: ok. 4000 passed", { timeout: 15_000 });
  expect((await log.textContent())!.indexOf("test case 0000")).toBe(0);

  // Its artifact: listed, and a download through a signed GET of the bucket.
  const artifact = view.locator('[data-run-artifact="junit.xml"]');
  await expect(artifact).toContainText(`${report.length} B`);
  const href = (await artifact.locator("[data-run-artifact-download]").getAttribute("href"))!;
  const r = await alice.request.get(`${app}${href}`, { maxRedirects: 0 });
  expect(r.status()).toBe(307);
  const to = r.headers().location;
  expect(to).toContain(`/runs/${project}/${running}/artifacts/junit.xml?`);
  expect(decodeURIComponent(to)).toContain('attachment; filename="junit.xml"');
  expect(Buffer.from(await (await fetch(to)).arrayBuffer()).equals(report)).toBe(true);
});

test("a run that fails is in the inbox, and opens there; a phone reads it", async () => {
  test.setTimeout(120_000);
  await alice.goto(`${app}/p/${project}/runs/${started}`);
  const view = alice.locator(`[data-run="${started}"]`);
  await expect(view.locator("[data-run-state-label]")).toHaveText("Pending");
  // Its workspace's box enrolled again as another machine: the job won't
  // send the run there, and fails it. The open view hears it move.
  sql((db) => db.prepare("UPDATE hosted_workspaces SET machine = ? WHERE project_id = ?").run(`m${hex()}`, project));
  await expect(view.locator("[data-run-state-label]")).toHaveText("Failed", { timeout: 30_000 });
  await expect(view.locator("[data-run-event]").last()).toContainText("Arugula moved it to failed · runs go only to their project's cloud workspaces");

  await alice.locator("[data-platform-inbox]").click();
  const entry = alice.locator(`[data-inbox-group="run_failed"] [data-inbox-entry="${started}"]`);
  await expect(entry).toContainText("Acme ·");
  await expect(entry).toContainText(`sh -c echo hi && exit 2`);
  await expect(entry).toContainText("Run · Failed · started by alice");
  await entry.locator("a").click();
  await expect(alice).toHaveURL(new RegExp(`/runs/${started}$`));
  // The thread kept what was said.
  await expect(view.locator("[data-run-thread] .thread-msg").last()).toContainText("watching this one");

  // A phone's width: nothing runs off the side.
  await alice.setViewportSize({ width: 390, height: 844 });
  await expect(view.locator("[data-run-command]")).toBeVisible();
  expect(await alice.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await alice.goto(`${app}/p/${project}/runs`);
  await expect(alice.locator("[data-run-row]")).toHaveCount(2);
  expect(await alice.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  // An id-only address, as a push's tap opens, lands in its project.
  await alice.goto(`${app}/runs/${started}`);
  await expect(alice).toHaveURL(new RegExp(`/p/${project}[^/]*/runs/${started}$`), { timeout: 15_000 });
});
