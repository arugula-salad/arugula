// #617's "done when", against the real chant: a waiting gate's card names
// the decision it enforces, with the plan digest and the last release. The
// fixture (e2e/fixtures/chant-workspace-decisions, chant 0.108.1 pinned in
// its lock file: `graph --intent` came in 0.102) is the toy workspace with
// three decisions and a work item. toy-001 constrains member:delivery and
// supersedes toy-000; toy-002, proposed, constrains one file in delivery;
// W-001 carries out toy-001 in delivery. A gate in delivery enforces
// toy-001 alone. The decisions are read from chant's intent graph when the
// gate is raised, so they come a moment after it: on the block's "Waiting
// on you", the swarm's card and the phone's sheet.
//
// #618's: an opened member card shows its decisions (the proposed one
// links to hud), the lease on W-001 and the run under it, each linked to
// the pane that ran it, and how many commits no decision covers.
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
const FAKE_ACP = fileURLToPath(new URL("../../crates/daemon/tests/fake_acp.py", import.meta.url));
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
  expect(out).toContain("toy: 2 members, 3 records, 0 waiting at a gate");
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

test("a gate dismissed stays dismissed when the workspace moves and its decisions are read again (#617)", async ({ page }) => {
  test.setTimeout(60_000);
  await open(page);
  const state = async () => (await (await fetch(`${base()}/api/blocks/${block}`)).json()).state;
  await expect.poll(async () => (await reasonOf(page, block))?.kind ?? null, { timeout: 15_000 }).toBe("gate");
  const act = await fetch(`${base()}/api/attention/act`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ action: "dismiss", pane: block }) });
  expect(act.ok).toBe(true);
  await expect.poll(async () => (await panesOf(page)).find((p) => p.id === block)?.attention).toBe("idle");
  // A commit in delivery: a full read (the gate's `why` starts over) and a
  // new intent read, whose decisions land on the block's card.
  const before = (await state()).updated_ms;
  writeFileSync(join(ws, "delivery", "notes.md"), "another note\n");
  git("commit", "-qam", "delivery: another note");
  await expect.poll(async () => (await state()).updated_ms, { timeout: 20_000 }).not.toBe(before);
  await expect.poll(async () => (await state()).gates[0]?.why?.decisions?.[0]?.id ?? null, { timeout: 20_000 }).toBe("toy-001");
  await new Promise((r) => setTimeout(r, 2_000));
  // Not asked again.
  expect((await panesOf(page)).find((p) => p.id === block)?.attention).toBe("idle");
});

test("an opened member card: its decisions, the lease and run on it with their pane, and undecided commits (#618)", async ({ page }, info) => {
  test.setTimeout(120_000);
  await open(page);
  const state = async () => (await (await fetch(`${base()}/api/blocks/${block}`)).json()).state;
  const post = (path: string, body: unknown) => fetch(base() + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  // An agent block started from delivery, as the card's Agent button
  // starts one (on the fake ACP agent): its turn is a run in the ledger,
  // as the agent session the declaration binds to delivery.
  const config = {
    agent: "acp",
    command: ["python3", FAKE_ACP],
    cwd: join(ws, "delivery"),
    prompt: "look at the ship op",
    chant: { root: ws, member: "delivery", agent: "shipper", chant: join(ws, "node_modules/.bin/chant") },
  };
  const agentPane: PaneId = (await (await post("/api/blocks", { type: "agent", config })).json()).block;
  const runs = () => JSON.parse(chant(ws, "workspace", "runs", "--json").stdout).runs as { id: string; agent: string }[];
  await expect.poll(() => runs().find((r) => r.id.startsWith(`arugula-${agentPane}-`))?.agent ?? null, { timeout: 30_000 }).toBe("shipper");
  const run = runs().find((r) => r.id.startsWith(`arugula-${agentPane}-`))!.id;
  // Someone claims W-001 and records a run under its lease whose id names
  // that pane, but no block there wrote it: not linked.
  const claim = JSON.parse(chant(ws, "workspace", "work", "claim", "W-001", "--holder", "shipper", "--json").stdout);
  const other = `arugula-${agentPane}-1`;
  const fields = { id: other, startedAt: new Date().toISOString(), harness: "claude-code", agent: "shipper", lease: claim.lease.token, unit: { id: "W-001", kind: "work" }, instruction: { sha256: "a".repeat(64) } };
  const started = spawnSync(join(FIXTURE, "node_modules/.bin/chant"), ["workspace", "runs", "start", "--from", "-"], { cwd: ws, input: JSON.stringify(fields), encoding: "utf8" });
  expect(started.status, started.stdout + started.stderr).toBe(0);
  // The block reads the lease (status) once the fingerprint settles.
  await expect.poll(async () => (await state()).leases?.map((l: { item: string }) => l.item) ?? [], { timeout: 20_000 }).toEqual(["W-001"]);

  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = page.locator(`[data-workspace-block="${block}"]`);
  const card = shown.locator('[data-member="delivery"]');
  await card.locator('[data-why-toggle="delivery"]').click();
  const why = card.locator('[data-why="delivery"]');
  await expect(why.locator('[data-why-decision="toy-001"]')).toContainText("toy-001 decided A person approves each ship", { timeout: 30_000 });
  await expect(why.locator('[data-why-decision="toy-002"]')).toContainText("proposed");
  await expect(why).toContainText("Replaced: toy-000 Ship on every merge");
  await expect(why.locator("[data-why-undecided]")).toHaveText(/^\d+ commits? changed delivery while no decision covered it\.$/);
  await expect(card.locator("[data-tag-undecided]")).toHaveText(/^\d+ undecided$/);
  // The agent block's run links to its pane; the other run and the lease
  // under it don't.
  await expect(why.locator(`[data-why-run="${run}"] [data-why-pane="${agentPane}"]`)).toBeVisible({ timeout: 20_000 });
  await expect(why.locator(`[data-why-run="${run}"]`)).toContainText("as shipper");
  await expect(why.locator(`[data-why-run="${other}"]`)).toContainText("running · as shipper · W-001");
  await expect(why.locator(`[data-why-run="${other}"] [data-why-pane]`)).toHaveCount(0);
  await expect(why.locator('[data-why-lease="W-001"]')).toContainText("W-001 held by shipper · until");
  await expect(why.locator('[data-why-lease="W-001"] [data-why-pane]')).toHaveCount(0);
  await expect(card.locator("[data-tag-leases]")).toHaveText("1 leased");
  await card.screenshot({ path: info.outputPath("member-card.png") });

  // Another client closes the card: this one, still showing it, asks again.
  expect((await post(`/api/blocks/${block}/call/why`, { member: "delivery", open: false })).ok).toBe(true);
  await expect.poll(async () => (await state()).members.find((m: { name: string }) => m.name === "delivery").why?.decisions?.[0]?.id ?? null, { timeout: 15_000 }).toBe("toy-001");
  await expect(why.locator('[data-why-decision="toy-001"]')).toBeVisible();

  // The proposed decision is reviewed in hud: its decisions page, at the id.
  expect((await post(`/api/blocks/${block}/call/hud`, { url: "javascript:alert(1)" })).ok).toBe(false);
  expect((await post(`/api/blocks/${block}/call/hud`, { url: "https://hud.example/?q=1" })).ok).toBe(false);
  expect((await post(`/api/blocks/${block}/call/hud`, { url: "https://hud.example/" })).ok).toBe(true);
  await page.evaluate(() => {
    (window as unknown as { opened: string[] }).opened = [];
    window.open = (u?: string | URL) => ((window as unknown as { opened: string[] }).opened.push(String(u)), null);
  });
  await why.locator('[data-why-review="toy-002"]').click();
  expect(await page.evaluate(() => (window as unknown as { opened: string[] }).opened)).toEqual(["https://hud.example/decisions#toy-002"]);
  // The block's text has it all.
  const text = cli("capture", `%${block}`);
  expect(text).toContain("lease W-001 held by shipper (active)");
  expect(text).toContain(`run ${other} running`);
  // The run's pane link shows the agent's pane.
  await why.locator(`[data-why-run="${run}"] [data-why-pane="${agentPane}"]`).click();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(agentPane);
});
