// M34's "done when", against the real chant: a chant workspace (the fixture
// in e2e/fixtures/chant-workspace, chant 0.87.0 pinned in its lock file) as
// a block. `arugula workspace` shows its members; a gated op (`chant run
// ship` exits 3) shows as attention while the block is drawn, within a few
// seconds; the owner approves on the desktop, an editor on a phone from the
// sheet's gates-first list (chant's ledger names each), a viewer sees the
// gate with no Approve; the next `chant run` walks through. *Expire* (#310)
// turns a gate down from the swarm's card and the phone: the attention
// clears and the next `chant run` stops there again. On a phone, *Run op*
// on a member starts a gated op in a pane, *Approve* clears it, and *Run
// op* again walks through (#309). The pane menu and the picker offer "Open
// as workspace" in a workspace's directory. The owner sets who approvals
// are recorded as from the block's bar (#302). *Graph* on a member frames
// behold on it, and behold's pick opens that member's Shell or Changes
// (#620; needs behold: $ARUGULA_BEHOLD or `behold` on PATH, else skipped).
//
// The first run installs the fixture's chant (`npm ci`, a few seconds).

import { execFileSync, spawn, spawnSync, type ChildProcess } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { devices, expect, test, type Page } from "@playwright/test";
import { menu, open, paneEl, closeContexts } from "./helpers";
import type { PaneId, Reason } from "../src/proto";
import { ANY, daemonPort } from "./ports";
import { labs } from "./labs";

test.afterAll(closeContexts);

const OWNER = "me@example.com";
const FRIEND = "friend@example.com";
const FIXTURE = fileURLToPath(new URL("./fixtures/chant-workspace", import.meta.url));
const phone = (() => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  return { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch };
})();

let PORT = 0;
let dir = "";
let ws = "";
let daemon: ChildProcess | null = null;

const base = () => `http://127.0.0.1:${PORT}`;
const chant = (cwd: string, ...args: string[]) =>
  spawnSync(join(FIXTURE, "node_modules/.bin/chant"), args, { cwd, encoding: "utf8", env: { ...process.env, NO_COLOR: "1" } });
const git = (...args: string[]) => execFileSync("git", ["-C", ws, "-c", "user.email=t@example.com", "-c", "user.name=t", ...args]);
const cli = (...args: string[]) => execFileSync("../target/debug/arugula", ["--socket", join(dir, "state/sock"), ...args], { encoding: "utf8" });
const post = (path: string, body: unknown, headers: Record<string, string> = {}) =>
  fetch(base() + path, { method: "POST", headers: { "content-type": "application/json", ...headers }, body: JSON.stringify(body) });
// A page just opened has no client (or state) yet: no panes.
const panesOf = (page: Page) => page.evaluate(() => window.__arugula?.client.state?.panes ?? []);
const reasonOf = async (page: Page, id: PaneId): Promise<Reason | null> => (await panesOf(page)).find((p) => p.id === id)?.reason ?? null;
/** Who approved delivery's gates, by chant's own `status`. */
const approvers = () => {
  const st = JSON.parse(chant(ws, "workspace", "status", "local", "--json").stdout);
  const delivery = st.members.find((m: { name: string }) => m.name === "delivery");
  return delivery.gates.flatMap((g: { approvals: { principal: string }[] }) => g.approvals.map((a) => a.principal));
};
/** `chant run <op>` in delivery: 3 at the gate, 0 through it. */
const run = (op = "ship") => chant(join(ws, "delivery"), "run", op).status;

test.describe.configure({ mode: "serial" });
test.use({ baseURL: async ({}, use) => use(base()) });

let block: PaneId = 0;

test.beforeAll(async () => {
  test.setTimeout(180_000);
  if (!existsSync(join(FIXTURE, "node_modules/.bin/chant"))) {
    execFileSync("npm", ["ci", "--no-audit", "--no-fund"], { cwd: FIXTURE, stdio: "ignore" });
  }
  // Resolved, as the daemon reports paths (macOS's temp dir is behind a symlink).
  dir = realpathSync(mkdtempSync(join(tmpdir(), "ilg-e2e-workspace-")));
  ws = join(dir, "toy");
  cpSync(FIXTURE, ws, { recursive: true, filter: (src) => !src.includes("node_modules") });
  symlinkSync(join(FIXTURE, "node_modules"), join(ws, "node_modules"));
  git("init", "-q", "-b", "main");
  git("add", ".");
  git("commit", "-qm", "the toy workspace");
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--block-listen", ANY, "--state-dir", labs(join(dir, "state")), "--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"],
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

test("arugula workspace shows its members; a gate reached while it's drawn is attention in seconds", async ({ page }) => {
  test.setTimeout(90_000);
  const out = cli("workspace", ws);
  block = Number(/^%(\d+)/.exec(out)![1]);
  expect(out).toContain("toy: 2 members, 0 records, 0 waiting at a gate");
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = page.locator(`[data-workspace-block="${block}"]`);
  await expect(shown.locator("[data-member]")).toHaveCount(2);
  await expect(shown.locator('[data-member="app"] .ws-kind')).toHaveText("other");
  await expect(shown.locator(".review-live")).toHaveText("live");

  // The op stops at its gate, elsewhere (a terminal, CI): the block sees
  // chant's ledger move and reads again.
  expect(run()).toBe(3);
  const t = Date.now();
  await expect(shown.locator('[data-gate="delivery/ship/approve-ship"]')).toBeVisible({ timeout: 8_000 });
  const took = Date.now() - t;
  console.log(`chant run exits 3 → the gate on screen: ${took} ms`);
  // The lifecycle ref is looked at every second while drawn, then a read:
  // about 1.3 s here (#311 asks about 2).
  expect(took).toBeLessThan(3_000);
  // The pane's attention follows the block's state: poll it, as it may
  // come a moment after the gate is drawn.
  await expect
    .poll(async () => {
      const r = await reasonOf(page, block);
      return r && [r.kind, r.headline, r.bundle];
    })
    .toEqual(["gate", "delivery: ship waits at gate approve-ship", `gate:${ws}`]);
  const r = (await reasonOf(page, block))!;
  expect(r.actions).toEqual(["allow", "expire", "dismiss"]);
  await expect(shown.locator('[data-member="delivery"]')).toHaveClass(/waits/);
});

test("the owner approves; the next run walks through", async ({ page }) => {
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = page.locator(`[data-workspace-block="${block}"]`);
  await shown.locator('[data-gate="delivery/ship/approve-ship"] [data-approve]').click();
  // Approving runs chant, which on a busy machine takes seconds.
  await expect(shown.locator("[data-ws-said]")).toContainText("Approved approve-ship", { timeout: 15_000 });
  await expect(shown.locator("[data-gate]")).toHaveCount(0);
  await expect.poll(async () => (await panesOf(page)).find((p) => p.id === block)?.attention).toBe("idle");
  // chant's ledger names the owner by their Arugula name.
  expect(approvers()).toEqual([OWNER]);
  expect(run()).toBe(0);
});

test("a burst of edits in a member costs one full read, once it holds still (#311)", async ({ page }) => {
  test.setTimeout(90_000);
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  await expect(page.locator(`[data-workspace-block="${block}"] .review-live`)).toHaveText("live");
  const shown = async (): Promise<{ updated_ms: number; loading: boolean }> => (await (await fetch(`${base()}/api/blocks/${block}`)).json()).state;
  const readAt = async () => (await shown()).updated_ms;
  // The last test's `chant run` changed the workspace, and its read may
  // not have come yet (it would land in the burst): first, the block
  // holds still, no read for two polls and then some.
  let before = 0;
  for (;;) {
    const now = await shown();
    if (!now.loading && now.updated_ms === before) break;
    before = now.updated_ms;
    await new Promise((r) => setTimeout(r, 7_000));
  }
  // An agent at work in a member: the tree changes every second, faster
  // than the block polls (3 s), for ten seconds. No full read meanwhile.
  // A tracked file (its diff is in the fingerprint; an untracked one's
  // mtime and size are too).
  const file = join(ws, "app", "README.md");
  const seen = new Set<number>();
  for (let i = 0; i < 10; i++) {
    writeFileSync(file, `edit ${i}\n`);
    seen.add(await readAt());
    await new Promise((r) => setTimeout(r, 1_000));
  }
  expect([...seen]).toEqual([before]);
  // It holds still: one read, within two polls and the read itself.
  await expect.poll(readAt, { timeout: 15_000 }).not.toBe(before);
  const after = await readAt();
  await new Promise((r) => setTimeout(r, 7_000));
  expect(await readAt()).toBe(after);
  git("checkout", "-q", "--", "app/README.md");
});

test("Expire turns a gate down, from the card and the phone: the next run stops there again", async ({ browser, page }) => {
  test.setTimeout(90_000);
  await open(page);
  const pending = async () => {
    await expect.poll(async () => (await reasonOf(page, block))?.headline ?? null, { timeout: 15_000 }).toBe(
      "delivery: release waits at gate approve-release",
    );
  };
  const cleared = () => expect.poll(async () => (await reasonOf(page, block))?.kind ?? null, { timeout: 15_000 }).toBeNull();
  expect(run("release")).toBe(3);
  await pending();

  // The swarm's card: Approve, Expire, Dismiss.
  await page.goto("/#swarm");
  const card = page.locator('.swarm-card[data-kind="gate"]');
  await expect(card.locator("[data-expire-gate]")).toHaveText("Expire", { timeout: 15_000 });
  await card.locator("[data-expire-gate]").click();
  await cleared();
  // Not approved: chant's ledger has no approval for it, and the next run
  // stops at the gate again.
  expect(approvers()).toEqual([OWNER]);
  expect(run("release")).toBe(3);
  await pending();
  const hist = await (await fetch(`${base()}/api/history?pane=${block}`)).json();
  expect(hist.some((h: { text: string; by: string }) => h.text === "expired delivery: release at gate approve-release" && h.by === OWNER)).toBe(true);

  // On a phone, from the sheet.
  const ctx = await browser.newContext({ ...phone, baseURL: base() });
  const mine = await ctx.newPage();
  await mine.goto("/");
  await expect.poll(async () => (await reasonOf(mine, block))?.kind ?? null, { timeout: 15_000 }).toBe("gate");
  await mine.locator(".sheet-button").click();
  await mine.locator(`.needs-you [data-wants="${block}"] [data-expire-gate]`).tap();
  await cleared();
  expect(run("release")).toBe(3);
  await pending();
  expect(approvers()).toEqual([OWNER]);
  await ctx.close();
});

test("on a phone, an editor approves from the sheet, gates first; a viewer sees it and can't", async ({ browser, page }) => {
  test.setTimeout(90_000);
  await open(page);
  // Something else wanting attention too, to come after the gate.
  await post("/api/run", { command: "sleep 1; false" });
  // Another op stops at its gate.
  expect(run("release")).toBe(3);
  const session = await page.evaluate((b) => {
    const c = window.__arugula.client;
    return c.sessionOfTab(c.tabOfPane(b)!.id);
  }, block);

  // On the swarm's rail: a gate card, with Approve.
  await page.goto("/#swarm");
  const card = page.locator('.swarm-card[data-kind="gate"]');
  await expect(card).toBeVisible({ timeout: 15_000 });
  await expect(card.locator(".ch b")).toHaveText("Waits at a gate");
  await expect(card.locator(".cq")).toHaveText("delivery: release waits at gate approve-release");
  await expect(card.locator("[data-approve-gate]")).toHaveText("Approve");
  expect(await card.getAttribute("data-bundle")).toBe(`gate:${ws}`);

  // A viewer: the gate, no Approve.
  expect((await post("/api/acl", { session, principal: `tailnet:${FRIEND}`, role: "viewer" })).ok).toBe(true);
  const ctx = await browser.newContext({ ...phone, baseURL: base(), extraHTTPHeaders: { "tailscale-user-login": FRIEND } });
  const friend = await ctx.newPage();
  await friend.goto("/");
  await expect.poll(() => friend.evaluate(() => window.__arugula?.client.role())).toBe("viewer");
  await expect.poll(async () => (await reasonOf(friend, block))?.kind ?? null, { timeout: 15_000 }).toBe("gate");
  await friend.locator(".sheet-button").click();
  await expect(friend.locator(".needs-you [data-wants]").first()).toHaveAttribute("data-wants", String(block));
  await expect(friend.locator("[data-approve-gate]")).toHaveCount(0);
  await friend.locator(".sheet-backdrop").click({ position: { x: 5, y: 5 } });
  await friend.evaluate((b) => window.__arugula.client.setActive(b), block);
  const theirs = friend.locator(`[data-workspace-block="${block}"]`);
  await expect(theirs.locator('[data-gate="delivery/release/approve-release"]')).toBeVisible();
  await expect(theirs.locator("[data-approve]")).toHaveCount(0);
  expect((await post(`/api/blocks/${block}/call/approve`, {}, { "tailscale-user-login": FRIEND })).status).toBe(403);

  // Made an editor: Approve, first in the sheet.
  expect((await post("/api/acl", { session, principal: `tailnet:${FRIEND}`, role: "editor" })).ok).toBe(true);
  await expect.poll(() => friend.evaluate(() => window.__arugula.client.role())).toBe("editor");
  await friend.locator(".sheet-button").click();
  const first = friend.locator(".needs-you [data-wants]").first();
  await expect(first).toHaveAttribute("data-wants", String(block));
  await expect(friend.locator(".needs-you [data-wants]")).toHaveCount(2, { timeout: 10_000 });
  await first.locator("[data-approve-gate]").tap();
  await expect.poll(async () => (await reasonOf(page, block))?.kind ?? null, { timeout: 15_000 }).toBeNull();
  expect(approvers().sort()).toEqual([FRIEND, OWNER].sort());
  expect(run("release")).toBe(0);
  await ctx.close();
});

test("the block says which env it watches and switches it (#312)", async ({ page }) => {
  test.setTimeout(60_000);
  const state = async () => (await (await fetch(`${base()}/api/blocks/${block}`)).json()).state;
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const env = page.locator(`[data-workspace-block="${block}"] [data-ws-env]`);
  await expect(env).toHaveValue("local");
  // From the API (and so `arugula call %N env`), a name that isn't one is refused.
  expect((await post(`/api/blocks/${block}/call/env`, { name: "--json" })).ok).toBe(false);
  expect((await post(`/api/blocks/${block}/call/env`, { name: "staging" })).ok).toBe(true);
  await expect.poll(async () => (await state()).env).toBe("staging");
  await expect(env).toHaveValue("staging");
  await expect(env.locator("option")).toContainText(["local", "staging"]);
  // Back, from the menu.
  await env.selectOption("local");
  await expect.poll(async () => (await state()).env).toBe("local");
  await expect.poll(async () => (await state()).loading).toBe(false);
  // `arugula workspace --env` opens one on that env.
  const other = Number(/^%(\d+)/.exec(cli("workspace", ws, "--env", "staging"))![1]);
  expect((await (await fetch(`${base()}/api/blocks/${other}`)).json()).state.env).toBe("staging");
});

test("on a phone, Run op on a member starts a gated op; Approve clears its gate; Run op again walks through (#309)", async ({ browser, page }) => {
  test.setTimeout(90_000);
  await open(page);
  const ctx = await browser.newContext({ ...phone, baseURL: base(), extraHTTPHeaders: { "tailscale-user-login": OWNER } });
  const mine = await ctx.newPage();
  await open(mine);
  await mine.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = mine.locator(`[data-workspace-block="${block}"]`);
  const delivery = shown.locator('[data-member="delivery"]');
  const terminals = async () => (await panesOf(page)).filter((p) => p.type === "terminal").map((p) => p.id);

  // Ops status names (the gated ones so far) are offered; deploy, never
  // run, is typed.
  await delivery.locator("[data-run-op]").tap();
  const ops = shown.locator('[data-run-ops="delivery"]');
  await expect(ops.locator("[data-op]")).toHaveText(["release", "ship"]);
  const before = await terminals();
  await ops.locator("input").fill("deploy");
  await ops.locator("button", { hasText: "Run" }).tap();
  await expect(ops).toHaveCount(0);
  // A pane beside the block, in the member, running `chant run deploy`,
  // which stops at the gate: attention, through the usual read.
  await expect.poll(async () => (await terminals()).length, { timeout: 10_000 }).toBe(before.length + 1);
  // The phone shows the new pane; back to the block.
  await expect.poll(() => mine.evaluate(() => window.__arugula.client.active())).not.toBe(block);
  await mine.evaluate((b) => window.__arugula.client.setActive(b), block);
  await expect(shown.locator('[data-gate="delivery/deploy/approve-deploy"]')).toBeVisible({ timeout: 20_000 });
  await expect
    .poll(async () => {
      const r = await reasonOf(mine, block);
      return r && [r.kind, r.headline];
    })
    .toEqual(["gate", "delivery: deploy waits at gate approve-deploy"]);

  // Approve, on the phone.
  await shown.locator('[data-gate="delivery/deploy/approve-deploy"] [data-approve]').tap();
  await expect(shown.locator("[data-ws-said]")).toContainText("Approved approve-deploy", { timeout: 15_000 });
  await expect.poll(async () => (await reasonOf(mine, block))?.kind ?? null, { timeout: 15_000 }).toBeNull();

  // Run op again: deploy is offered now (its gate is in the ledger), and
  // the run walks through the approved gate.
  await delivery.locator("[data-run-op]").tap();
  const again = await terminals();
  await ops.locator('[data-op="deploy"]').tap();
  await expect.poll(async () => (await terminals()).length, { timeout: 10_000 }).toBe(again.length + 1);
  const pane = (await terminals()).find((t) => !again.includes(t))!;
  await expect.poll(() => cli("capture", `%${pane}`), { timeout: 30_000 }).toContain("deployed");
  await expect(shown.locator("[data-gate]")).toHaveCount(0);
  await ctx.close();
});

test("Open as workspace, from a pane's menu and the picker, in a workspace's directory", async ({ page }) => {
  await open(page);
  const term = (await panesOf(page)).find((p) => p.type === "terminal")!.id;
  await page.evaluate((t) => window.__arugula.client.setActive(t), term);
  await post(`/api/panes/${term}/send`, { text: `cd ${ws}`, enter: true });
  await expect.poll(async () => (await panesOf(page)).find((p) => p.id === term)?.cwd).toBe(ws);
  const before = (await panesOf(page)).filter((p) => p.type === "workspace").length;
  await menu(page, paneEl(page, term), "Open as workspace");
  await expect.poll(async () => (await panesOf(page)).filter((p) => p.type === "workspace").length).toBe(before + 1);

  // The picker, from a terminal elsewhere.
  await post(`/api/panes/${term}/send`, { text: `cd ${dir}`, enter: true });
  await expect.poll(async () => (await panesOf(page)).find((p) => p.id === term)?.cwd).toBe(dir);
  await page.evaluate((t) => window.__arugula.client.setActive(t), term);
  await page.keyboard.press("Control+Shift+G");
  await expect(page.locator(".picker")).toBeVisible();
  await expect(page.locator("[data-open-workspace]")).toHaveCount(0);
  await page.locator(`.picker-row:not(.recent)[data-path="${ws}"]`).click();
  await page.locator("[data-open-workspace]").click();
  await expect.poll(async () => (await panesOf(page)).filter((p) => p.type === "workspace").length).toBe(before + 2);
});

test("the owner sets who approvals are recorded as, from the block's bar (#302)", async ({ page }) => {
  const state = async () => (await (await fetch(`${base()}/api/blocks/${block}`)).json()).state;
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = page.locator(`[data-workspace-block="${block}"]`);
  await expect(shown.locator("[data-ws-actor]")).toHaveText("as you");
  await shown.locator("[data-ws-actor]").click();
  const form = shown.locator("[data-ws-principals]");
  await form.locator("input").fill("github:me-x");
  await form.locator("textarea").fill("friend=github:friend-y");
  await form.locator("[data-ws-principals-save]").click();
  await expect(form).toHaveCount(0);
  await expect(shown.locator("[data-ws-actor]")).toHaveText("as github:me-x");
  await expect.poll(async () => (await state()).principals).toEqual({ friend: "github:friend-y" });
  // A principal is one word, not a flag; refused, and nothing changes.
  expect((await post(`/api/blocks/${block}/call/principals`, { actor: "--sign" })).ok).toBe(false);
  expect((await state()).actor).toBe("github:me-x");
  // Cleared again: approvals name the owner by their Arugula name.
  expect((await post(`/api/blocks/${block}/call/principals`, { actor: null, principals: {} })).ok).toBe(true);
  await expect(shown.locator("[data-ws-actor]")).toHaveText("as you");
});

// #620: behold for the graph, as the daemon finds it.
const BEHOLD = process.env.ARUGULA_BEHOLD || (spawnSync("sh", ["-c", "command -v behold"]).status === 0 ? "behold" : "");

test("Graph on a member frames behold on it; behold's pick opens that member's Shell or Changes (#620)", async ({ page }) => {
  test.skip(!BEHOLD, "no behold: set ARUGULA_BEHOLD or put behold on PATH");
  test.setTimeout(120_000);
  await open(page);
  await page.evaluate((b) => window.__arugula.client.setActive(b), block);
  const shown = page.locator(`[data-workspace-block="${block}"]`);
  const before = (await panesOf(page)).map((p) => p.id);
  await shown.locator('[data-member="delivery"] [data-graph]').click();
  // behold reads the workspace through chant before it answers.
  const frame = shown.locator("[data-ws-graph] iframe");
  await expect(frame).toBeVisible({ timeout: 90_000 });
  const src = new URL((await frame.getAttribute("src"))!);
  const app = new URL(base()).origin;
  expect(src.hostname).toMatch(new RegExp(`^b-${block}-\\w+\\.localhost$`));
  expect(Object.fromEntries(src.searchParams)).toEqual({ member: "delivery", env: "", gates: "local", embed: "1", host: app, theme: "dark" });
  // Started with no env, so behold reads nothing live by itself.
  const cmdline = spawnSync("pgrep", ["-lf", `serve ${ws} `], { encoding: "utf8" }).stdout;
  expect(cmdline).toContain("--port 0");
  expect(cmdline).not.toContain("--env");
  // It's behold answering, through the block's site.
  await expect.poll(async () => (await fetch(`${base()}/api/blocks/${block}`).then((r) => r.json())).state.graph.is).toBe("running");
  const behold = page.frameLocator(`[data-workspace-block="${block}"] [data-ws-graph] iframe`);
  await expect(behold.locator("#panel")).toBeAttached({ timeout: 30_000 });
  // behold answers only its own loopback name and refuses cross-site writes:
  // the block's site hands it `Host: localhost:<port>` and, for a write,
  // `Origin: http://localhost:<port>`, so a write from the frame goes through
  // with no --allow-host.
  const wrote = await page
    .frames()
    .find((f) => f.url().startsWith(src.origin))!
    .evaluate(async () => (await fetch("/api/refresh?notify=1", { method: "POST" })).status);
  expect(wrote).toBe(200);
  // What behold posts when a member's box is picked, from behold's origin.
  // The toy's delivery has no lexicon for behold to draw boxes from, so it's
  // posted the way behold's embedSelected posts it.
  const pick = (member: string) =>
    page
      .frames()
      .find((f) => f.url().startsWith(src.origin))!
      .evaluate(([m, host]) => window.parent.postMessage({ type: "behold:select", member: m, node: null }, host), [member, app]);
  // The same message from anywhere else (the app page itself) opens nothing.
  await page.evaluate(() => window.postMessage({ type: "behold:select", member: "delivery", node: null }, "*"));
  await pick("delivery");
  await expect
    .poll(async () => (await panesOf(page)).filter((p) => !before.includes(p.id)).map((p) => [p.type, p.cwd]))
    .toEqual([["terminal", join(ws, "delivery")]]);
  // Changes, when the frame's bar says so: a diff block on the member.
  const opened = (await panesOf(page)).map((p) => p.id);
  await shown.locator("[data-ws-graph-clicks]").selectOption("changes");
  await pick("delivery");
  await expect
    .poll(async () => (await panesOf(page)).filter((p) => !opened.includes(p.id)).map((p) => p.type))
    .toEqual(["diff"]);
  // An env switch moves behold's gate strip with a behold:view message: the
  // frame doesn't load again, behold reads the new env's gates and keeps
  // `gates` in its own URL, and the same run keeps going.
  const beholdFrame = () => page.frames().find((f) => f.url().startsWith(src.origin))!;
  await beholdFrame().evaluate(() => ((window as unknown as { __stay: number }).__stay = 1));
  const gatesRead = page.waitForRequest((r) => r.url().startsWith(src.origin) && new URL(r.url()).pathname === "/api/workspace/gates" && new URL(r.url()).searchParams.get("env") === "staging");
  expect((await post(`/api/blocks/${block}/call/env`, { name: "staging" })).ok).toBe(true);
  await expect.poll(async () => (await fetch(`${base()}/api/blocks/${block}`).then((r) => r.json())).state.env).toBe("staging");
  await gatesRead;
  await expect.poll(() => new URL(beholdFrame().url()).searchParams.get("gates")).toBe("staging");
  expect(new URL((await frame.getAttribute("src"))!).searchParams.get("gates")).toBe("local");
  expect(await beholdFrame().evaluate(() => (window as unknown as { __stay?: number }).__stay)).toBe(1);
  expect((await fetch(`${base()}/api/blocks/${block}`).then((r) => r.json())).state.graph).toMatchObject({ is: "running", run: 1 });
  expect((await post(`/api/blocks/${block}/call/env`, { name: "local" })).ok).toBe(true);
  await shown.locator("[data-ws-graph] button[title='Close the graph']").click();
  await expect(shown.locator("[data-ws-graph]")).toHaveCount(0);
  // behold is the block's: closing the block stops it.
  expect(spawnSync("pgrep", ["-f", `serve ${ws} `]).status).toBe(0);
  expect((await post(`/api/panes/${block}/close`, {})).ok).toBe(true);
  await expect.poll(() => spawnSync("pgrep", ["-f", `serve ${ws} `]).status, { timeout: 10_000 }).toBe(1);
});
