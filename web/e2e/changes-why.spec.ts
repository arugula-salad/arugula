// #619's "done when", against the real chant (0.108.1, pinned in
// e2e/fixtures/chant-why's lock file): *Changes* on a workspace member
// shows, on each hunk, the decision and the run that made it, and a tap on
// the run opens the agent pane that made it. The workspace comes from
// crates/daemon/tests/fixtures/chant/why/make.sh: app/server.mjs has a line
// the member's agent committed in a recorded run (its commit carrying
// Chant-Agent and Chant-Run, #590), a line not committed yet, a decision
// constraining the file by path, and a work item whose lease the member's
// agent session holds. The run's id is the agent block's own, as an agent
// block picks it, so the run links to that block.
//
// The first run installs the fixture's chant (`npm ci`, a few seconds).

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { active, closeContexts, open } from "./helpers";
import { ANY, daemonPort } from "./ports";
import { labs } from "./labs";

test.afterAll(closeContexts);

const OWNER = "me@example.com";
const FIXTURE = fileURLToPath(new URL("./fixtures/chant-why", import.meta.url));
const MAKE = fileURLToPath(new URL("../../crates/daemon/tests/fixtures/chant/why/make.sh", import.meta.url));
const CHANT = join(FIXTURE, "node_modules/.bin/chant");

let PORT = 0;
let dir = "";
let ws = "";
let daemon: ChildProcess | null = null;

const base = () => `http://127.0.0.1:${PORT}`;
const cli = (...args: string[]) => execFileSync("../target/debug/arugula", ["--socket", join(dir, "state/sock"), ...args], { encoding: "utf8" });
const post = (path: string, body: unknown) =>
  fetch(base() + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });

test.use({ baseURL: async ({}, use) => use(base()) });

test.beforeAll(async () => {
  test.setTimeout(180_000);
  if (!existsSync(CHANT)) execFileSync("npm", ["ci", "--no-audit", "--no-fund"], { cwd: FIXTURE, stdio: "ignore" });
  // Resolved, as the daemon reports paths (macOS's temp dir is behind a symlink).
  dir = realpathSync(mkdtempSync(join(tmpdir(), "ilg-e2e-why-")));
  ws = join(dir, "why");
  mkdirSync(ws);
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", labs(join(dir, "state")), "--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--wisp-token-file", "/nonexistent"],
    ],
    // The workspace has no node_modules of its own: its chant is this one.
    { stdio: "ignore", env: { ...process.env, CHANT } },
  );
  PORT = await daemonPort(join(dir, "state"), daemon);
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${base()}/api/host`)).ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error("daemon did not start");
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

test("a hunk in Changes names its decision and run, and the run opens its agent", async ({ page }, info) => {
  test.setTimeout(120_000);
  // The member's agent, running as its session `app`: its run is the one
  // the workspace records.
  const agentRes = await post("/api/blocks", {
    type: "agent",
    config: { agent: "claude", cwd: dir, chant: { root: ws, member: "app", agent: "app", chant: CHANT } },
    local: true,
  });
  expect(agentRes.ok).toBe(true);
  const agent: number = (await agentRes.json()).block;
  execFileSync("sh", [MAKE, ws, CHANT], { env: { ...process.env, RUN: `arugula-${agent}-1790848800000` }, stdio: "ignore" });

  const out = cli("workspace", ws);
  const block = Number(/^%(\d+)/.exec(out)![1]);
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = page.locator(`[data-workspace-block="${block}"]`);
  await expect(shown.locator('[data-member="app"]')).toBeVisible({ timeout: 30_000 });
  await shown.locator('[data-member="app"] button', { hasText: "Changes" }).click();

  // Since the member's branch left main: the committed line and the one
  // that isn't, in one hunk.
  const diff = page.locator("[data-diff]").last();
  await expect(diff.locator(".review-what")).toHaveText(/^the working tree against where it left main \(/, { timeout: 15_000 });
  await diff.locator('[data-file="app/server.mjs"] .diff-file-head').click();
  const why = diff.locator("[data-why]").first();
  await expect(why).toBeVisible({ timeout: 20_000 });
  await expect(why.locator('[data-decision="why-001"]')).toHaveText("why-001 The server answers on one port");
  const run = why.locator(`[data-run="arugula-${agent}-1790848800000"]`);
  await expect(run).toHaveText(`Run arugula-${agent}-1790848800000 (app, claude-code)`);
  await expect(why.locator("[data-uncommitted]")).toHaveText("1 not committed yet; app holds the lease on W-001");
  await why.screenshot({ path: info.outputPath("hunk-why.png") });
  await diff.screenshot({ path: info.outputPath("changes.png") });

  // A tap on the run opens the agent that made it.
  expect(await active(page)).not.toBe(agent);
  await run.click();
  await expect.poll(() => active(page)).toBe(agent);
});
