// #1731: the Editor preference (VS Code, nvim, Ask each time), with a real
// nvim. Settings shows it with no Labs flag on; VS Code opens an editor
// block as before; nvim opens the file at its line in the nvim already in
// the pane or its tab (over nvim's RPC socket, #1730) and shows that pane,
// else a new pane beside running nvim there; Ask offers both with
// Remember this; and nvim seen in a pane makes the first Open in editor
// ask, once. Each from the pane menu and from Claude Code's diff card (the
// stand-in `fake_claude.py`, in a terminal pane).
//
// The daemon is the spec's own, with no Labs flag, and its nvims read a
// config of the spec's (no swap files) rather than the person's. Skips,
// saying so, where nvim is missing or older than 0.10 (#1730's reader
// needs 0.10).

import { execFile, execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, menuItem, paneEl, panes, ready, reset, run } from "./helpers";
import type { PaneId } from "../src/shared/proto";
import { ANY, blockPort, daemonPort } from "./ports";
import "./local-token";

/** Why nvim can't be used here, or null. */
function noNvim(): string | null {
  try {
    const v = /NVIM v(\d+)\.(\d+)/.exec(execFileSync("nvim", ["--version"]).toString());
    if (!v) return "nvim doesn't say its version";
    return Number(v[1]) > 0 || Number(v[2]) >= 10 ? null : `nvim ${v[1]}.${v[2]} is older than 0.10, which panes need`;
  } catch {
    return "no nvim on PATH";
  }
}
const why = noNvim();
if (why) console.log(`SKIPPED: editor-pref.spec.ts: ${why}`);
test.skip(!!why, why ?? "");

test.afterAll(closeContexts);
test.describe.configure({ mode: "serial" });

let APP = "";
test.use({ baseURL: async ({}, use) => use(APP) });

let dir = "";
let state = "";
let proj = "";
let daemon: ChildProcess | undefined;
const sh = promisify(execFile);
const CLI = resolve("../target/debug/arugula");
const cli = (...args: string[]) => sh(CLI, ["--socket", join(state, "sock"), ...args], { cwd: proj });
/** Line `n` of the spec's files, as written. */
const lineText = (name: string, n: number) => `${name} line ${n}`;

test.beforeAll(async () => {
  if (why) return;
  dir = realpathSync(mkdtempSync(join(tmpdir(), "ilg-ep-")));
  // Short: a Unix socket's path (the IDE relay's, under it) has a limit.
  state = join(dir, "s");
  proj = join(dir, "shop");
  mkdirSync(state, { recursive: true });
  mkdirSync(join(proj, "src"), { recursive: true });
  for (const f of ["a", "b"]) writeFileSync(join(proj, `src/${f}.txt`), Array.from({ length: 60 }, (_, i) => lineText(f, i + 1)).join("\n") + "\n");
  // Every nvim in the daemon's panes: this config, and no swap files (a
  // killed nvim's would stop the next one with a question).
  mkdirSync(join(dir, "config/nvim"), { recursive: true });
  writeFileSync(join(dir, "config/nvim/init.lua"), "vim.opt.swapfile = false\nvim.opt.shortmess:append('I')\n");
  daemon = spawn(
    "../target/debug/arugulad",
    [
      // No labs(): Settings › Editor is everyone's.
      ...["--listen", ANY, "--block-listen", ANY, "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
    ],
    {
      stdio: "ignore",
      env: { ...process.env, ARUGULA_WISP_TOKEN_FILE: "/nonexistent", XDG_CONFIG_HOME: join(dir, "config"), XDG_DATA_HOME: join(dir, "data"), XDG_STATE_HOME: join(dir, "xstate") },
    },
  );
  APP = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
  await blockPort(state, daemon);
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${APP}/api/host`)).ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
});

test.afterAll(async () => {
  const d = daemon;
  if (d && d.exitCode === null) {
    const gone = new Promise((r) => d.once("exit", r));
    d.kill("SIGTERM");
    await gone;
  }
  if (dir) rmSync(dir, { recursive: true, force: true });
});

const info = (page: Page, p: PaneId) => page.evaluate((p) => window.__arugula.client.info(p) ?? null, p);
/** What #1730's reader says the pane's nvim shows: `a.txt:12 (nvim)`. */
const nvimAt = async (page: Page, p: PaneId) => {
  const i = await info(page, p);
  return i?.editor?.app === "nvim" ? i.title : null;
};
const pref = (page: Page) => page.evaluate(() => (JSON.parse(localStorage.getItem("arugula.seen") ?? "{}") as Record<string, unknown>).editor ?? null);

/** Settings › Editor › `label`. */
async function choose(page: Page, label: "VS Code" | "nvim" | "Ask each time") {
  await page.locator("[data-account-menu]").click();
  await (await menuItem(page, label, { exact: true })).click();
}

/** Editor blocks asked for (VS Code), answered here so code-server isn't
 * fetched: what the page asked is what's checked. */
async function vscodeAsks(page: Page): Promise<Record<string, unknown>[]> {
  const asked: Record<string, unknown>[] = [];
  await page.route("**/api/blocks", async (route) => {
    const body = route.request().postDataJSON() as Record<string, unknown>;
    if (route.request().method() !== "POST" || body.type !== "editor") return route.fallback();
    asked.push(body);
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ block: 999999 }) });
  });
  return asked;
}

/** The pane menu's Open in editor, from its header (a right-click in a
 * pane running nvim would be nvim's). */
async function openInEditor(page: Page, pane: PaneId) {
  await page.locator(`[data-pane-more="${pane}"]`).click();
  await (await menuItem(page, "Open in editor")).click();
}

/** Type into nvim in `pane`. */
async function inNvim(page: Page, pane: PaneId, keys: string) {
  await paneEl(page, pane).click({ position: { x: 40, y: 40 } });
  await page.keyboard.type(keys, { delay: 2 });
}

/** A fresh tab with one shell, in the spec's project. */
async function shell(page: Page): Promise<PaneId> {
  await reset(page);
  const [term] = await panes(page);
  await run(page, term, `cd ${proj} && echo in-$((40+2))`, "in-42");
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.cwd(p), term)).toBe(proj);
  return term;
}

/** nvim at `file` in `pane`, once #1730's reader has it. */
async function nvimIn(page: Page, pane: PaneId, file: string) {
  await run(page, pane, `nvim ${file}`, lineText(file.replace(/^.*\/(\w)\.txt$/, "$1"), 1));
  await expect.poll(() => nvimAt(page, pane), { timeout: 10_000 }).toBe(`${file.split("/").pop()}:1 (nvim)`);
}

test("Settings › Editor is there with no Labs flag: VS Code until chosen, and kept", async ({ page }) => {
  await reset(page);
  expect(existsSync(join(state, "flags.json"))).toBe(false);
  await page.locator("[data-account-menu]").click();
  await expect(await menuItem(page, "VS Code", { exact: true })).toHaveAttribute("aria-checked", "true");
  await expect(await menuItem(page, "Ask each time", { exact: true })).toHaveAttribute("aria-checked", "false");
  await (await menuItem(page, "nvim", { exact: true })).click();
  expect(await pref(page)).toBe("nvim");
  await page.reload();
  await page.locator("[data-account-menu]").click();
  await expect(await menuItem(page, "nvim", { exact: true })).toHaveAttribute("aria-checked", "true");
});

test("VS Code, from the pane menu: an editor block on the pane's directory, as before", async ({ page }) => {
  const term = await shell(page);
  await choose(page, "VS Code");
  const asked = await vscodeAsks(page);
  await openInEditor(page, term);
  await expect.poll(() => asked).toEqual([{ type: "editor", config: {}, from_pane: term }]);
  expect(await panes(page)).toEqual([term]);
});

test("nvim, from the pane menu: a new nvim pane beside it, then that one", async ({ page }) => {
  const term = await shell(page);
  await choose(page, "nvim");
  await openInEditor(page, term);
  await expect.poll(() => panes(page)).toHaveLength(2);
  const nv = (await panes(page)).find((p) => p !== term)!;
  await expect.poll(() => active(page)).toBe(nv);
  await expect.poll(() => info(page, nv).then((i) => i?.editor?.app ?? null), { timeout: 10_000 }).toBe("nvim");
  expect((await info(page, nv))?.cwd).toBe(proj);
  // Again from the shell: its tab has an nvim now, so that one, shown.
  await page.evaluate((p) => window.__arugula.client.setActive(p), term);
  await openInEditor(page, term);
  await expect.poll(() => active(page)).toBe(nv);
  expect(await panes(page)).toHaveLength(2);
});

test("nvim, with nvim in the pane: that nvim, at the file and line, no new pane, in under a second", async ({ page }) => {
  const term = await shell(page);
  await choose(page, "nvim");
  await nvimIn(page, term, "src/a.txt");
  await openInEditor(page, term);
  await expect.poll(() => active(page)).toBe(term);
  expect(await panes(page)).toEqual([term]);
  // A file, as a card asks for one: the operation, on that pane.
  // It answers once nvim has run `:drop`: that's the time to open it.
  const t0 = Date.now();
  const { res, ms } = await page.evaluate(async (p) => {
    const t = performance.now();
    const res = await window.__arugula.client.request("POST", `/api/panes/${p}/nvim`, { file: "src/b.txt", line: 37 }).then((r) => r.json());
    return { res, ms: performance.now() - t };
  }, term);
  expect(res).toEqual({ pane: term, how: "pane" });
  await expect.poll(() => nvimAt(page, term), { intervals: [25] }).toBe("b.txt:37 (nvim)");
  console.log(`opened in the pane's nvim in ${Math.round(ms)} ms; the page showed it after ${Date.now() - t0} ms`);
  expect(ms).toBeLessThan(1000);
  expect(await panes(page)).toEqual([term]);
});

test("Ask each time: VS Code or nvim, and Remember this keeps the answer", async ({ page }) => {
  const term = await shell(page);
  await choose(page, "Ask each time");
  const asked = await vscodeAsks(page);
  await openInEditor(page, term);
  await expect(await menuItem(page, "Remember this")).toHaveAttribute("aria-checked", "false");
  await (await menuItem(page, "nvim", { exact: true })).click();
  await expect.poll(() => panes(page)).toHaveLength(2);
  const nv = (await panes(page)).find((p) => p !== term)!;
  await expect.poll(() => info(page, nv).then((i) => i?.editor?.app ?? null), { timeout: 10_000 }).toBe("nvim");
  expect(await pref(page)).toBe("ask");
  // Asked again; this time remembered.
  await openInEditor(page, term);
  await (await menuItem(page, "Remember this")).click();
  await expect(await menuItem(page, "Remember this")).toHaveAttribute("aria-checked", "true");
  await (await menuItem(page, "VS Code", { exact: true })).click();
  await expect.poll(() => asked).toEqual([{ type: "editor", config: {}, from_pane: term }]);
  expect(await pref(page)).toBe("vscode");
  // And not asked again.
  await openInEditor(page, term);
  await expect.poll(() => asked).toHaveLength(2);
  await expect(page.getByRole("menu")).toHaveCount(0);
});

test("nothing chosen, nvim seen in a pane: the first Open in editor asks, and remembers", async ({ page }) => {
  const term = await shell(page);
  expect(await pref(page)).toBeNull();
  await nvimIn(page, term, "src/a.txt");
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}")["editor:nvim-seen"] ?? null)).not.toBeNull();
  await openInEditor(page, term);
  await expect(await menuItem(page, "Remember this")).toHaveAttribute("aria-checked", "true");
  await (await menuItem(page, "nvim", { exact: true })).click();
  expect(await pref(page)).toBe("nvim");
  await expect.poll(() => active(page)).toBe(term);
});

test("from Claude Code's diff card: VS Code at the line, nvim in the tab's nvim, Ask, and a new nvim pane", async ({ page }) => {
  test.setTimeout(90_000);
  await reset(page);
  const fake = resolve("../crates/daemon/tests/fake_claude.py");
  const claude = Number((await cli("run", "--cwd", proj, "--", "python3", fake)).stdout.trim().replace("%", ""));
  await expect.poll(async () => (await fetch(`${APP}/api/panes/${claude}/capture`)).text()).toContain("connected");
  // Line 30 of a.txt changes.
  const text = Array.from({ length: 60 }, (_, i) => (i === 29 ? "a line 30, changed" : lineText("a", i + 1))).join("\\n") + "\\n";
  await fetch(`${APP}/api/panes/${claude}/send`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ text: `edit ${join(proj, "src/a.txt")} ${text}`, enter: true }),
  });
  await page.evaluate((p) => window.__arugula.client.setActive(p), claude);
  const card = page.locator(`[data-pane="${claude}"] .diff-card`);
  await expect(card).toBeVisible({ timeout: 15_000 });
  const open = card.locator("[data-open-editor]");

  // VS Code: an editor block on the file, at the changed line.
  await choose(page, "VS Code");
  const asked = await vscodeAsks(page);
  await open.click();
  await expect.poll(() => asked).toEqual([{ type: "editor", config: { path: join(proj, "src/a.txt"), line: 30 }, from_pane: claude }]);

  // nvim, none running: a new pane beside, nvim at the file and line.
  await choose(page, "nvim");
  await open.click();
  await expect.poll(() => panes(page)).toHaveLength(2);
  const nv = (await panes(page)).find((p) => p !== claude)!;
  await expect.poll(() => nvimAt(page, nv), { timeout: 10_000 }).toBe("a.txt:30 (nvim)");
  await expect.poll(() => active(page)).toBe(nv);
  expect((await info(page, nv))?.cwd).toBe(join(proj, "src"));

  // nvim, running in the tab: that one, at the line, without a new pane.
  await ready(page, nv);
  await inNvim(page, nv, ":e b.txt\n");
  await expect.poll(() => nvimAt(page, nv)).toBe("b.txt:1 (nvim)");
  await page.evaluate((p) => window.__arugula.client.setActive(p), claude);
  const answered = page.waitForResponse((r) => r.url().endsWith(`/api/panes/${claude}/nvim`));
  const t0 = Date.now();
  await open.click();
  expect(await (await answered).json()).toEqual({ pane: nv, how: "tab" });
  const took = Date.now() - t0;
  await expect.poll(() => nvimAt(page, nv), { intervals: [25] }).toBe("a.txt:30 (nvim)");
  console.log(`opened in the tab's nvim ${took} ms after the click; the page showed it after ${Date.now() - t0} ms`);
  expect(took).toBeLessThan(1000);
  await expect.poll(() => active(page)).toBe(nv);
  expect(await panes(page)).toHaveLength(2);

  // Ask each time: nvim, from the menu.
  await choose(page, "Ask each time");
  await inNvim(page, nv, ":e b.txt\n");
  await expect.poll(() => nvimAt(page, nv)).toBe("b.txt:1 (nvim)");
  await page.evaluate((p) => window.__arugula.client.setActive(p), claude);
  await open.click();
  await (await menuItem(page, "nvim", { exact: true })).click();
  await expect.poll(() => nvimAt(page, nv)).toBe("a.txt:30 (nvim)");
  expect(await panes(page)).toHaveLength(2);
  expect(asked).toHaveLength(1);
  expect(await pref(page)).toBe("ask");

  await fetch(`${APP}/api/panes/${claude}/send`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ text: "quit", enter: true }) });
});
