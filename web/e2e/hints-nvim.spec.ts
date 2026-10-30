// nvim's hints (#1735, shell/hints/nvim.ts), against a real nvim in a
// pane: the "nvim in Arugula" help card, a pane modal the first time nvim
// runs; the chips nvim-connect (Add arugula.nvim), nvim-open-here (Use
// nvim) and nvim-diff (Claude Code proposes an edit while nvim is
// connected); and the key tip on every nvim start, whose button opens
// `:help` in that nvim. Each chip is seen once answered, not once shown:
// one hidden by a tab switch shows again. `editor-beside` no longer offers
// VS Code to nvim. Tips off hides the chips and the card's first opening,
// not the menus that open the card.
//
// Each nvim gets data and config directories of its own, empty, so it has
// no arugula.nvim (the daemon offers it) until a test adds it. Hints keep
// quiet under a test runner unless a spec asks: this one does
// (`__arugulaHints`), and says the hints that could take the one place
// first were seen already (the daemon's Claude Code directory is the
// machine's, which may hold conversations).

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem, paneEl, panes, reset } from "./helpers";
import type { PaneId } from "../src/shared/proto";
import { ANY, daemonPort } from "./ports";

test.afterAll(closeContexts);
test.describe.configure({ mode: "serial" });

const OWNER = "me@example.com";
const fake = resolve("../crates/daemon/tests/fake_claude.py");
let APP = "";
let dir = "";
let daemon: ChildProcess | undefined;
test.use({ baseURL: async ({}, use) => use(APP) });

const NVIM_CHIPS = ["nvim-help", "nvim-connect", "nvim-open-here", "nvim-diff"];
/** The real hints that could take the one place on screen first. */
const SAID = ["command-mark", "claude-conversations", "second-person", "push-device", "cd-picker", "after-restart"];

/** Hints on, `seen` said already (this browser's first page only), and
 * window.open recorded instead of opened. */
async function setUp(page: Page, seen: string[], more: Record<string, unknown> = {}) {
  await page.addInitScript(
    ({ seen, more }) => {
      const w = window as unknown as Record<string, unknown>;
      w.__arugulaHints = true;
      w.__opened = [];
      window.open = ((url: string) => {
        (w.__opened as string[]).push(url);
        return {} as Window;
      }) as typeof window.open;
      if (!localStorage.getItem("arugula.seen")) localStorage.setItem("arugula.seen", JSON.stringify({ ...Object.fromEntries(seen.map((id) => [`seen:${id}`, 1])), ...more }));
    },
    { seen: [...SAID, ...seen], more },
  );
}

const seenHere = (page: Page) => page.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}"));
const opened = (page: Page) => page.evaluate(() => (window as unknown as { __opened: string[] }).__opened);
const chip = (page: Page, pane: PaneId, id: string) => page.locator(`[data-pane-header="${pane}"] [data-hint="${id}"]`);
const card = (page: Page, pane: PaneId) => page.locator(`[data-pane="${pane}"] [data-pane-tip]`);
/** Longer than the hints hold back after a key (hints/index.ts, 1.5 s). */
const settle = (page: Page) => page.waitForTimeout(2000);

/** Typed into `pane` by this page, as its own keys: the pane stays driven
 * by this person (an API call's keys would make someone else its driver,
 * and a pane modal never opens over a pane someone else drives). */
const send = (page: Page, pane: PaneId, text: string) =>
  page.evaluate(({ pane, text }) => window.__arugula.client.input(pane, new TextEncoder().encode(text + "\r")), { pane, text });
const capture = async (pane: PaneId) => (await fetch(`${APP}/api/panes/${pane}/capture`)).text();
const editor = (page: Page, pane: PaneId) => page.evaluate((p) => window.__arugula.client.info(p)?.editor ?? null, pane);

let n = 0;
/** nvim in `pane`, with empty directories of its own; once the daemon
 * reads it (its editor has a pid), the pid. */
async function nvim(page: Page, pane: PaneId, home = join(dir, `nvim-${++n}`)): Promise<number> {
  for (const d of ["data", "config", "state"]) mkdirSync(join(home, d), { recursive: true });
  // The daemon's socket said outright: arugula.nvim looks for it under
  // XDG_STATE_HOME, which is nvim's own here.
  await send(page, pane, `ARUGULA_SOCK=${join(dir, "state/sock")} XDG_DATA_HOME=${home}/data XDG_CONFIG_HOME=${home}/config XDG_STATE_HOME=${home}/state nvim -n -i NONE`);
  await expect.poll(async () => (await editor(page, pane))?.pid ?? null, { timeout: 20_000 }).not.toBeNull();
  return (await editor(page, pane))!.pid!;
}

/** nvim in `pane` quits, and the pane is a shell again. */
async function quit(page: Page, pane: PaneId) {
  await send(page, pane, "\u001b:qa!");
  await expect.poll(() => editor(page, pane), { timeout: 15_000 }).toBeNull();
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.command ?? null, pane)).toBeNull();
}

/** Another tab, then back: what was on screen without an answer comes back. */
async function awayAndBack(page: Page) {
  const first = await page.evaluate(() => window.__arugula.client.tab!);
  const session = await page.evaluate(() => window.__arugula.client.session!);
  await page.evaluate((s) => window.__arugula.client.intent({ op: "new_tab", session: s, from_pane: null }), session);
  await expect.poll(() => page.evaluate((t) => window.__arugula.client.tab !== t, first)).toBe(true);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  await page.evaluate((t) => window.__arugula.client.selectTab(t), first);
}

/** One session with one fresh pane (helpers' reset), again if that pane's
 * shell exits before it draws: on a busy Mac a new bash now and then exits
 * at once (2 of 70 tests here, before any nvim of the test ran), and
 * that's not what these tests are about. */
async function fresh(page: Page) {
  try {
    await reset(page);
  } catch {
    await reset(page);
  }
}

async function reload(page: Page) {
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.state)).toBe(true);
}

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-hints-nvim-"));
  const state = join(dir, "state");
  mkdirSync(join(dir, "ide"));
  // Claude Code's IDE lockfiles in a directory of the spec's; and not a
  // pane of whatever Arugula runs this spec, so arugula.nvim joins ours.
  const env: NodeJS.ProcessEnv = { ...process.env, ARUGULA_CLAUDE_IDE_DIR: join(dir, "ide") };
  delete env.ARUGULA_PANE;
  delete env.ARUGULA_SOCK;
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--block-listen", ANY, "--state-dir", state, "--owner", OWNER],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
    ],
    { stdio: "ignore", env },
  );
  APP = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${APP}/api/host`)).ok) break;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
});

test.afterAll(async () => {
  daemon?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

test("the help card: once, over the first nvim pane, until it's closed; the pane menu and Help open it again", async ({ page }) => {
  await setUp(page, []);
  await fresh(page);
  const [term] = await panes(page);
  await nvim(page, term);

  await expect(card(page, term)).toBeVisible({ timeout: 15_000 });
  await expect(card(page, term).locator("h2")).toHaveText("nvim in Arugula");
  // Short: a title and one sentence; the long text is on the docs page.
  await expect(card(page, term).locator("p")).toHaveText("Arugula follows this nvim, opens files in it, and shows Claude Code's edits here as a vimdiff.");
  await expect(card(page, term).locator("h3")).toHaveCount(0);
  // Near the whole pane, with its dimmed pane around it, and centred in it.
  const [slot, box, inner] = [await paneEl(page, term).boundingBox(), await card(page, term).boundingBox(), await card(page, term).locator(".pane-tip-inner").boundingBox()];
  expect(box!.width / slot!.width).toBeGreaterThan(0.88);
  expect(box!.width / slot!.width).toBeLessThan(0.92);
  expect(box!.x - slot!.x).toBeGreaterThan(slot!.width * 0.04);
  expect(Math.abs(inner!.x + inner!.width / 2 - (box!.x + box!.width / 2))).toBeLessThan(2);
  if (process.env.SHOT_DIR) await page.screenshot({ path: join(process.env.SHOT_DIR, "card-normal.png") });
  await expect(card(page, term).getByLabel("Don't show tips like this again")).not.toBeChecked();
  // A pane modal is seen once answered: a tab switch doesn't use it up.
  expect(await seenHere(page)).not.toHaveProperty("seen:nvim-help");
  await awayAndBack(page);
  await expect(card(page, term)).toBeVisible();

  await card(page, term).locator("[data-hint-dismiss]").click();
  await expect(card(page, term)).toHaveCount(0);
  expect(await seenHere(page)).toHaveProperty("seen:nvim-help");

  // Never again on its own: not after a reload, not over a second nvim.
  await reload(page);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), term);
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const other = (await panes(page)).find((p) => p !== term)!;
  await nvim(page, other);
  await settle(page);
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);

  // The pane menu's "nvim help" opens it over that pane.
  await page.locator(`[data-pane-header="${other}"]`).click({ button: "right", position: { x: 6, y: 6 } });
  await (await menuItem(page, "nvim help", { exact: true })).click();
  await expect(card(page, other)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(card(page, other)).toHaveCount(0);

  // Help in the account menu, too: over an nvim pane of the tab.
  await page.locator("[data-account-menu]").click();
  await (await menuItem(page, "nvim in Arugula", { exact: true })).click();
  await expect(page.locator("[data-pane-tip]")).toHaveCount(1);
  await page.locator("[data-pane-tip] [data-hint-second]").click();
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);

  // A pane that doesn't run nvim has no "nvim help".
  await quit(page, other);
  await page.locator(`[data-pane-header="${other}"]`).click({ button: "right", position: { x: 6, y: 6 } });
  await expect(page.getByRole("menu")).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "nvim help", exact: true })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await quit(page, term);
});

test("the help card honours Don't show tips like this again; Tips off hides the card and the chips, not the menus", async ({ page, browser }) => {
  await setUp(page, []);
  await fresh(page);
  const [term] = await panes(page);
  await nvim(page, term);
  await expect(card(page, term)).toBeVisible({ timeout: 15_000 });
  await card(page, term).getByLabel("Don't show tips like this again").check();
  await card(page, term).locator("[data-hint-dismiss]").click();
  expect(await seenHere(page)).toMatchObject({ "tips.modal": "off", "seen:nvim-help": expect.any(Number) });
  // The chips carry on: nvim without the plugin.
  await expect(chip(page, term, "nvim-connect")).toBeVisible({ timeout: 15_000 });
  await quit(page, term);

  // Tips off, in a browser that has seen none of nvim's: nothing on its own.
  const off = await (await browser.newContext()).newPage();
  await setUp(off, [], { tips: false });
  await fresh(off);
  const [mine] = await panes(off);
  await nvim(off, mine);
  await settle(off);
  await expect(off.locator("[data-hint]")).toHaveCount(0);
  await expect(off.locator("[data-pane-tip]")).toHaveCount(0);
  // But the pane menu opens the card.
  await off.locator(`[data-pane-header="${mine}"]`).click({ button: "right", position: { x: 6, y: 6 } });
  await (await menuItem(off, "nvim help", { exact: true })).click();
  await expect(card(off, mine)).toBeVisible();
  await off.keyboard.press("Escape");
  await quit(off, mine);
  await off.context().close();
});

test("nvim-connect: Add arugula.nvim installs it into that nvim, which joins; hidden unanswered it comes back; answered, never again", async ({ page }) => {
  await setUp(page, ["nvim-help"]);
  await fresh(page);
  const [term] = await panes(page);
  const home = join(dir, "connect");
  await nvim(page, term, home);
  await expect.poll(async () => (await editor(page, term))?.offer_plugin ?? false).toBe(true);

  const c = chip(page, term, "nvim-connect");
  await expect(c).toContainText("Arugula can follow this nvim and open files in it", { timeout: 15_000 });
  expect(await seenHere(page)).not.toHaveProperty("seen:nvim-connect");
  await awayAndBack(page);
  await expect(c).toBeVisible();

  await c.locator("[data-hint-act]").click();
  await expect(c).toHaveCount(0);
  expect(await seenHere(page)).toHaveProperty("seen:nvim-connect");
  // Installed in that nvim's data dir, loaded, and joined as the pane: it
  // can be followed now, and isn't offered the plugin any more.
  await expect.poll(async () => { const e = await editor(page, term); return e && (e.no_follow ? "read over its socket" : "the plugin"); }, { timeout: 15_000 }).toBe("the plugin");
  expect((await editor(page, term))?.offer_plugin ?? false).toBe(false);
  expect(readFileSync(join(home, "data/nvim/site/pack/arugula/start/arugula.nvim/ARUGULA_INSTALLED"), "utf8")).toBeTruthy();

  // Settings > Editor > Remove arugula.nvim (the owner's) deletes that copy
  // and says so; asked again, it says there is none.
  const copy = join(home, "data/nvim/site/pack/arugula/start/arugula.nvim");
  const remove = async () => {
    await page.locator("[data-account-menu]").click();
    await (await menuItem(page, "Remove arugula.nvim", { exact: true })).click();
  };
  await remove();
  await expect(page.getByText(/^Removed arugula\.nvim from /)).toBeVisible();
  expect(existsSync(copy)).toBe(false);
  await remove();
  await expect(page.getByText("No copy of arugula.nvim that Arugula installed is on this machine")).toBeVisible();

  // Another nvim without it, after a reload: answered, it never returns.
  await quit(page, term);
  await reload(page);
  await nvim(page, term);
  await expect.poll(async () => (await editor(page, term))?.offer_plugin ?? false).toBe(true);
  await settle(page);
  await expect(chip(page, term, "nvim-connect")).toHaveCount(0);
  await quit(page, term);
});

test("the help card's buttons: Edit in nvim, Add arugula.nvim and Read the docs show under their conditions and do their parts", async ({ page }) => {
  await setUp(page, []);
  await fresh(page);
  const [term] = await panes(page);
  const home = join(dir, "card-buttons");
  await nvim(page, term, home);
  await expect.poll(async () => (await editor(page, term))?.offer_plugin ?? false).toBe(true);
  await expect(card(page, term)).toBeVisible({ timeout: 15_000 });
  const names = () => card(page, term).locator("[data-hint-button]").allTextContents();
  // Preference not nvim, plugin missing: all four, the first the primary.
  await expect.poll(names).toEqual(["Edit in nvim", "Add arugula.nvim", "Read the docs", "Close"]);
  await expect(card(page, term).locator(".primary")).toHaveText("Edit in nvim");
  await expect(card(page, term).locator(".primary")).toHaveCount(1);

  // Read the docs opens the page, and answers the card.
  await card(page, term).getByRole("button", { name: "Read the docs" }).click();
  await expect(card(page, term)).toHaveCount(0);
  expect(await opened(page)).toContain("https://docs.arugula.io/nvim/");
  expect(await seenHere(page)).toHaveProperty("seen:nvim-help");

  // Opened again (the pane menu), Add arugula.nvim installs it and the card closes.
  const reopen = async () => {
    await page.locator(`[data-pane-header="${term}"]`).click({ button: "right", position: { x: 6, y: 6 } });
    await (await menuItem(page, "nvim help", { exact: true })).click();
    await expect(card(page, term)).toBeVisible();
  };
  await reopen();
  await card(page, term).getByRole("button", { name: "Add arugula.nvim" }).click();
  await expect(card(page, term)).toHaveCount(0);
  await expect.poll(async () => { const e = await editor(page, term); return e && (e.no_follow ? "read over its socket" : "the plugin"); }, { timeout: 15_000 }).toBe("the plugin");
  expect(readFileSync(join(home, "data/nvim/site/pack/arugula/start/arugula.nvim/ARUGULA_INSTALLED"), "utf8")).toBeTruthy();

  // The plugin is there now: no Add button. Edit in nvim sets the preference.
  await reopen();
  await expect.poll(names).toEqual(["Edit in nvim", "Read the docs", "Close"]);
  await card(page, term).getByRole("button", { name: "Edit in nvim" }).click();
  await expect(card(page, term)).toHaveCount(0);
  expect(await seenHere(page)).toMatchObject({ editor: "nvim" });

  // The preference is nvim: no Edit in nvim. Close is last.
  await reopen();
  await expect.poll(names).toEqual(["Read the docs", "Close"]);
  await expect(card(page, term).locator(".primary")).toHaveText("Read the docs");
  await card(page, term).getByRole("button", { name: "Close", exact: true }).click();
  await expect(card(page, term)).toHaveCount(0);
  await quit(page, term);
});

test("the help card in a narrow split: inside its pane, its content scrolls in the panel", async ({ page }) => {
  await setUp(page, []);
  await page.setViewportSize({ width: 900, height: 420 });
  await fresh(page);
  const [term] = await panes(page);
  await nvim(page, term);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), term);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), term);
  await expect.poll(async () => (await panes(page)).length).toBe(3);
  await expect(card(page, term)).toBeVisible({ timeout: 15_000 });
  const [slot, box] = [await paneEl(page, term).boundingBox(), await card(page, term).boundingBox()];
  expect(box!.x).toBeGreaterThanOrEqual(slot!.x);
  expect(box!.x + box!.width).toBeLessThanOrEqual(slot!.x + slot!.width);
  expect(box!.y + box!.height).toBeLessThanOrEqual(slot!.y + slot!.height);
  if (process.env.SHOT_DIR) await page.screenshot({ path: join(process.env.SHOT_DIR, "card-narrow.png") });
  // The buttons stay in the panel's box without any scrolling: the text
  // scrolls, they're pinned at the bottom.
  for (const el of [
    card(page, term).locator(".primary"),
    card(page, term).getByRole("button", { name: "Close", exact: true }),
    card(page, term).getByLabel("Don't show tips like this again"),
  ]) {
    await expect(el).toBeVisible();
    const b = await el.boundingBox();
    expect(b!.y).toBeGreaterThanOrEqual(box!.y);
    expect(b!.y + b!.height).toBeLessThanOrEqual(box!.y + box!.height);
    expect(b!.x).toBeGreaterThanOrEqual(box!.x);
    expect(b!.x + b!.width).toBeLessThanOrEqual(box!.x + box!.width);
  }
  await page.keyboard.press("Escape");
  await expect(card(page, term)).toHaveCount(0);
  await quit(page, term);
});

test("nvim-open-here: Use nvim sets the Editor preference; dismissed, it never returns", async ({ page, browser }) => {
  await setUp(page, ["nvim-help", "nvim-connect"]);
  await fresh(page);
  const [term] = await panes(page);
  await nvim(page, term);
  const c = chip(page, term, "nvim-open-here");
  await expect(c).toContainText("Open files from Arugula in this nvim?", { timeout: 15_000 });
  await awayAndBack(page);
  await expect(c).toBeVisible();
  await c.locator("[data-hint-act]").click();
  await expect(c).toHaveCount(0);
  expect(await seenHere(page)).toMatchObject({ editor: "nvim", "seen:nvim-open-here": expect.any(Number) });
  await quit(page, term);

  // Another browser dismisses it: seen, and the preference left alone.
  const other = await (await browser.newContext()).newPage();
  await setUp(other, ["nvim-help", "nvim-connect"]);
  await fresh(other);
  const [mine] = await panes(other);
  await nvim(other, mine);
  await expect(chip(other, mine, "nvim-open-here")).toBeVisible({ timeout: 15_000 });
  await chip(other, mine, "nvim-open-here").locator("[data-hint-dismiss]").click();
  const s = await seenHere(other);
  expect(s).toHaveProperty("seen:nvim-open-here");
  expect(s).not.toHaveProperty("editor");
  await quit(other, mine);
  await reload(other);
  await nvim(other, mine);
  await settle(other);
  await expect(other.locator('[data-hint="nvim-open-here"]')).toHaveCount(0);
  await quit(other, mine);
  await other.context().close();
});

test("nvim-diff: Claude Code's edit beside a connected nvim; Show me opens the help card at Claude's edits", async ({ page }) => {
  test.setTimeout(90_000);
  await setUp(page, ["nvim-help", "nvim-connect", "nvim-open-here"]);
  await fresh(page);
  const [term] = await panes(page);
  await nvim(page, term);
  // This start's key tip, answered first: on screen, it would stay.
  await expect(chip(page, term, "nvim-key-tip")).toBeVisible({ timeout: 15_000 });
  await chip(page, term, "nvim-key-tip").locator("[data-hint-dismiss]").click();
  // Claude Code (a stand-in) in a split of the same tab, proposing an edit.
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), term);
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const claude = (await panes(page)).find((p) => p !== term)!;
  const file = join(dir, "cart.js");
  writeFileSync(file, "module.exports = 1;\n");
  await send(page, claude, `python3 ${fake}`);
  await expect.poll(() => capture(claude), { timeout: 15_000 }).toContain("connected");
  await send(page, claude, `edit ${file} module.exports = 2;\\n`);
  await expect.poll(() => page.evaluate((p) => !!window.__arugula.client.info(p)?.diff, claude), { timeout: 15_000 }).toBe(true);

  const c = chip(page, term, "nvim-diff");
  await expect(c).toContainText("Review Claude's edits in nvim", { timeout: 15_000 });
  await awayAndBack(page);
  await expect(c).toBeVisible();
  await c.locator("[data-hint-act]").click();
  await expect(c).toHaveCount(0);
  expect(await seenHere(page)).toHaveProperty("seen:nvim-diff");
  // The card, over the nvim pane, at Claude's edits.
  await expect(card(page, term)).toBeVisible();
  await expect(card(page, term)).toContainText("vimdiff");
  await page.keyboard.press("Escape");
  await expect(card(page, term)).toHaveCount(0);
  await send(page, claude, "quit");
  await quit(page, term);
});

test("the key tip: on every nvim start a different tip; its button opens :help in that nvim, or the online docs; × hides it for that start", async ({ page }) => {
  test.setTimeout(120_000);
  await setUp(page, NVIM_CHIPS);
  await fresh(page);
  const [term] = await panes(page);
  const tips = page.locator(`[data-pane-header="${term}"] [data-hint="nvim-key-tip"]`);
  const copy = () => tips.locator(".hint-copy").textContent();

  // The first start: the list's first tip, its :help in this nvim.
  const pid1 = await nvim(page, term);
  await expect(tips).toContainText("% jumps to the bracket", { timeout: 15_000 });
  await expect(tips.locator("[data-hint-act]")).toHaveText(":help %");
  // Hidden by a tab switch, unanswered: the same tip comes back.
  await awayAndBack(page);
  await expect(tips).toContainText("% jumps to the bracket");
  await tips.locator("[data-hint-act]").click();
  await expect(tips).toHaveCount(0);
  await expect.poll(() => capture(term), { timeout: 10_000 }).toContain("motion.txt");
  expect(await opened(page)).toEqual([]);
  expect((await seenHere(page))["seen:nvim-key-tip"]).toEqual([`${term}:${pid1}`]);
  expect((await seenHere(page))["nvim.tips.next"]).toBe(1);
  // Answered for this nvim: not again while it runs, after a reload too.
  await reload(page);
  await settle(page);
  await expect(page.locator('[data-hint="nvim-key-tip"]')).toHaveCount(0);

  // The next start: the next tip. × hides it for that start.
  await quit(page, term);
  const pid2 = await nvim(page, term);
  expect(pid2).not.toBe(pid1);
  await expect(tips).toContainText("<C-o> goes back", { timeout: 15_000 });
  const second = await copy();
  await tips.locator("[data-hint-dismiss]").click();
  await expect(tips).toHaveCount(0);
  await settle(page);
  await expect(tips).toHaveCount(0);

  // The third: another tip again. Arugula can't reach this nvim's help
  // (the daemon refuses), so the button opens nvim's online docs.
  await quit(page, term);
  await nvim(page, term);
  await expect(tips).toContainText("g; steps back", { timeout: 15_000 });
  expect(await copy()).not.toBe(second);
  await page.route("**/api/panes/*/nvim/help", (r) => r.fulfill({ status: 409, contentType: "application/json", body: '{"error":"no"}' }));
  await tips.locator("[data-hint-act]").click();
  await expect.poll(() => opened(page)).toEqual(["https://neovim.io/doc/user/motion/#g%3B"]);
  await page.unroute("**/api/panes/*/nvim/help");
  expect((await seenHere(page))["seen:nvim-key-tip"]).toHaveLength(3);
  await quit(page, term);
});

test("editor-beside never offers VS Code to a pane running nvim", async ({ page }) => {
  // Every nvim chip said, and the key tip answered for this nvim: nothing
  // left that would take the place before it.
  await setUp(page, NVIM_CHIPS);
  await fresh(page);
  const [term] = await panes(page);
  const pid = await nvim(page, term);
  const tips = page.locator('[data-hint="nvim-key-tip"]');
  await expect(tips).toBeVisible({ timeout: 15_000 });
  await tips.locator("[data-hint-dismiss]").click();
  expect((await seenHere(page))["seen:nvim-key-tip"]).toContain(`${term}:${pid}`);
  await settle(page);
  await expect(page.locator('[data-hint="editor-beside"]')).toHaveCount(0);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  await quit(page, term);
});
