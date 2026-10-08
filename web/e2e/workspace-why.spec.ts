// #617's "done when", against the real chant: a waiting gate's card names
// the decision it enforces, with the plan digest and the last release. The
// fixture (e2e/fixtures/chant-workspace-decisions, chant 0.108.1 pinned in
// its lock file: `graph --intent` came in 0.102) is the toy workspace with
// three decisions. toy-001 constrains member:delivery and supersedes
// toy-000; toy-002, proposed, constrains one file in delivery. A gate in
// delivery enforces toy-001 alone. The decisions are read from chant's
// intent graph when the gate is raised, so they come a moment after it: on
// the block's "Waiting on you", the swarm's card and the phone's sheet.
//
// The first run installs the fixture's chant (`npm ci`, a few seconds).

import { execFileSync, spawn, spawnSync, type ChildProcess } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { devices, expect, test, type Page } from "@playwright/test";
import { open, closeContexts } from "./helpers";
import type { PaneId, Reason } from "../src/proto";
import { ANY, daemonPort } from "./ports";
import { labs } from "./labs";

test.afterAll(closeContexts);

const OWNER = "me@example.com";
const FIXTURE = fileURLToPath(new URL("./fixtures/chant-workspace-decisions", import.meta.url));
const phone = (() => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  return { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch };
})();
const ENFORCES = "Enforces toy-001: A person approves each ship";

let PORT = 0;
let dir = "";
let ws = "";
let daemon: ChildProcess | null = null;
let block: PaneId = 0;

const base = () => `http://127.0.0.1:${PORT}`;
const chant = (cwd: string, ...args: string[]) =>
  spawnSync(join(FIXTURE, "node_modules/.bin/chant"), args, { cwd, encoding: "utf8", env: { ...process.env, NO_COLOR: "1" } });
const git = (...args: string[]) => execFileSync("git", ["-C", ws, "-c", "user.email=t@example.com", "-c", "user.name=t", ...args]);
const cli = (...args: string[]) => execFileSync("../target/debug/arugula", ["--socket", join(dir, "state/sock"), ...args], { encoding: "utf8" });
const panesOf = (page: Page) => page.evaluate(() => window.__arugula?.client.state?.panes ?? []);
const reasonOf = async (page: Page, id: PaneId): Promise<Reason | null> => (await panesOf(page)).find((p) => p.id === id)?.reason ?? null;

test.describe.configure({ mode: "serial" });
test.use({ baseURL: async ({}, use) => use(base()) });

test.beforeAll(async () => {
  test.setTimeout(180_000);
  if (!existsSync(join(FIXTURE, "node_modules/.bin/chant"))) {
    execFileSync("npm", ["ci", "--no-audit", "--no-fund"], { cwd: FIXTURE, stdio: "ignore" });
  }
  dir = realpathSync(mkdtempSync(join(tmpdir(), "ilg-e2e-workspace-why-")));
  ws = join(dir, "toy");
  cpSync(FIXTURE, ws, { recursive: true, filter: (src) => !src.includes("node_modules") });
  symlinkSync(join(FIXTURE, "node_modules"), join(ws, "node_modules"));
  git("init", "-q", "-b", "main");
  git("add", ".");
  git("commit", "-qm", "the toy workspace");
  // A change to delivery that no decision names: an undecided commit.
  writeFileSync(join(ws, "delivery", "notes.md"), "a note\n");
  git("add", ".");
  git("commit", "-qm", "delivery: a note");
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", labs(join(dir, "state")), "--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--wisp-token-file", "/nonexistent"],
    ],
    { stdio: "ignore" },
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

test("a waiting gate names the decision it enforces, its plan and the last release (#617)", async ({ page }, info) => {
  test.setTimeout(90_000);
  const out = cli("workspace", ws);
  block = Number(/^%(\d+)/.exec(out)![1]);
  expect(out).toContain("toy: 2 members, 2 records, 0 waiting at a gate");
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = page.locator(`[data-workspace-block="${block}"]`);
  await expect(shown.locator("[data-member]")).toHaveCount(2);

  // The op stops at its gate; the gate shows, then what it enforces.
  expect(chant(join(ws, "delivery"), "run", "ship").status).toBe(3);
  const gate = shown.locator('[data-gate="delivery/ship/approve-ship"]');
  await expect(gate).toBeVisible({ timeout: 8_000 });
  const t = Date.now();
  await expect(gate.locator("[data-gate-decision]")).toHaveText(ENFORCES, { timeout: 30_000 });
  console.log(`the gate on screen, then its decision: ${Date.now() - t} ms`);
  await expect(gate.locator("[data-gate-decision]")).toHaveAttribute("data-gate-decision", "toy-001");
  // No plan for this gate (the toy op has none) and no release yet.
  await expect(gate.locator(".gate-why-release")).toHaveText("no release yet");
  await gate.screenshot({ path: info.outputPath("gate-card.png") });
  // The block's text says so too.
  expect(cli("capture", `%${block}`)).toContain("enforces toy-001 A person approves each ship");

  // The gate's attention carries it, for the rail and the phone.
  await expect
    .poll(async () => (await reasonOf(page, block))?.gate?.why?.decisions?.map((d) => d.id) ?? null, { timeout: 15_000 })
    .toEqual(["toy-001", "toy-002", "toy-000"]);

  // The swarm's card.
  await page.goto("/#swarm");
  const card = page.locator('.swarm-card[data-kind="gate"]');
  await expect(card).toBeVisible({ timeout: 15_000 });
  await expect(card.locator(".cq")).toHaveText("delivery: ship waits at gate approve-ship");
  await expect(card.locator("[data-gate-decision]")).toHaveText(ENFORCES);
  await card.screenshot({ path: info.outputPath("rail-card.png") });
});

test("on a phone, the sheet's gate says what it enforces (#617)", async ({ browser }, info) => {
  const ctx = await browser.newContext({ ...phone, baseURL: base(), extraHTTPHeaders: { "tailscale-user-login": OWNER } });
  const mine = await ctx.newPage();
  await mine.goto("/");
  await expect.poll(async () => (await reasonOf(mine, block))?.kind ?? null, { timeout: 15_000 }).toBe("gate");
  await mine.locator(".sheet-button").click();
  const row = mine.locator(`.needs-you [data-wants="${block}"]`);
  await expect(row.locator("[data-gate-why]")).toHaveText(`${ENFORCES} · no release yet`);
  await row.screenshot({ path: info.outputPath("phone-sheet.png") });
  await ctx.close();
});
