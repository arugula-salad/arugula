// #1734: an nvim editor block beside VS Code's, with a real nvim. "New
// editor block" (Add a pane > Editor) offers VS Code and nvim, picked by
// the Editor preference, with no Labs flag on. nvim makes a terminal typed
// as an editor, running nvim in the folder with arugula.nvim loaded from
// the daemon (it joins as the pane: followable) and the page's colors. Then
// Open in editor (from a file block) opens a file into it at its line; a
// palette change recolors it without a restart; it survives a daemon
// restart, and a reboot (its nvim gone too), on its file and line, still
// an editor; and the swarm shows it as one, with the nvim mark.
//
// The daemon is the spec's own, with no Labs flag; its nvims read a config
// of the spec's (no swap files) and keep their sockets in the spec's
// XDG_RUNTIME_DIR, where the spec asks nvim itself what it shows. Skips,
// saying so, where nvim is missing or older than 0.10.

import { execFile, execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, menuItem, panes, reset, run } from "./helpers";
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
if (why) console.log(`SKIPPED: nvim-block.spec.ts: ${why}`);
test.skip(!!why, why ?? "");

test.afterAll(closeContexts);
test.describe.configure({ mode: "serial" });

let APP = "";
test.use({ baseURL: async ({}, use) => use(APP) });

let dir = "";
let state = "";
let proj = "";
let runDir = "";
let PORT = 0;
let BLOCKS = 0;
let daemon: ChildProcess | undefined;
const sh = promisify(execFile);
const lineText = (name: string, n: number) => `${name} line ${n}`;
/** The palettes' terminal backgrounds (web/src/shared/appearance.ts):
 * an nvim block's Normal is drawn on them. */
const SLATE_BG = "#0c1118";
const LATTE_BG = "#eff1f5";

async function start() {
  daemon = spawn(
    "../target/debug/arugulad",
    [
      // No labs(): editors, and nvim's, are everyone's.
      ...["--listen", PORT ? `127.0.0.1:${PORT}` : ANY, "--block-listen", BLOCKS ? `127.0.0.1:${BLOCKS}` : ANY, "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
    ],
    {
      stdio: "ignore",
      env: {
        ...process.env,
        ARUGULA_WISP_TOKEN_FILE: "/nonexistent",
        XDG_CONFIG_HOME: join(dir, "config"),
        XDG_DATA_HOME: join(dir, "data"),
        XDG_STATE_HOME: join(dir, "xstate"),
        XDG_RUNTIME_DIR: runDir,
      },
    },
  );
  PORT ||= await daemonPort(state, daemon);
  BLOCKS ||= await blockPort(state, daemon);
  APP = `http://127.0.0.1:${PORT}`;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${APP}/api/host`)).ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
}

/** Stop the daemon as a restart does: it saves, then exits. */
async function stop() {
  const d = daemon;
  daemon = undefined;
  if (!d || d.exitCode !== null) return;
  const gone = new Promise((r) => d.once("exit", r));
  d.kill("SIGTERM");
  await gone;
}

test.beforeAll(async () => {
  if (why) return;
  dir = realpathSync(mkdtempSync(join(tmpdir(), "ilg-nb-")));
  // Short: Unix sockets' paths have a limit.
  state = join(dir, "s");
  runDir = join(dir, "r");
  proj = join(dir, "shop");
  mkdirSync(state, { recursive: true });
  mkdirSync(runDir, { recursive: true, mode: 0o700 });
  mkdirSync(join(proj, "src"), { recursive: true });
  for (const f of ["a", "b"]) writeFileSync(join(proj, `src/${f}.txt`), Array.from({ length: 60 }, (_, i) => lineText(f, i + 1)).join("\n") + "\n");
  mkdirSync(join(dir, "config/nvim"), { recursive: true });
  writeFileSync(join(dir, "config/nvim/init.lua"), "vim.opt.swapfile = false\nvim.opt.shortmess:append('I')\n");
  await start();
});

test.afterAll(async () => {
  await stop();
  if (dir) {
    // Its nvims, by the bootstrap they started with.
    await sh("pkill", ["-f", join(state, "editor/nvim/block.lua")]).catch(() => {});
    rmSync(dir, { recursive: true, force: true });
  }
});

const info = (page: Page, p: PaneId) => page.evaluate((p) => window.__arugula.client.info(p) ?? null, p);

/** What the nvim in pane `pane` says about itself, asked over its own
 * socket: its file and line, its Normal colors, colorscheme and
 * background, and whether arugula.nvim is loaded. */
async function nvimSays(pane: PaneId): Promise<{ file: string; line: number; bg: string; scheme: string; background: string; plugin: boolean } | null> {
  const expr = `luaeval("vim.json.encode({ pane = vim.env.ARUGULA_PANE, file = vim.fn.expand('%:p'), line = vim.fn.line('.'), bg = string.format('#%06x', vim.api.nvim_get_hl(0, { name = 'Normal' }).bg or 0), scheme = vim.g.colors_name or '', background = vim.o.background, plugin = vim.g.loaded_arugula == true })")`;
  for (const s of existsSync(runDir) ? readdirSync(runDir).filter((n) => n.startsWith("nvim.")) : []) {
    try {
      const { stdout } = await sh("nvim", ["--headless", "--clean", "--server", join(runDir, s), "--remote-expr", expr], { timeout: 5000 });
      const v = JSON.parse(stdout.trim()) as { pane: string; file: string; line: number; bg: string; scheme: string; background: string; plugin: boolean };
      if (Number(v.pane) === pane) return v;
    } catch {
      // gone, or not that one
    }
  }
  return null;
}

/** The Add a pane drawer, its Editor kind picked. */
async function editorDrawer(page: Page) {
  await page.locator("[data-add-pane]").first().click();
  const drawer = page.locator("[data-add-pane-drawer]");
  await expect(drawer).toBeVisible();
  await drawer.locator('[data-kind="editor"]').click();
  return drawer;
}

/** Settings › Editor › `label`. */
async function choose(page: Page, label: "VS Code" | "nvim" | "Ask each time") {
  await page.locator("[data-account-menu]").click();
  await (await menuItem(page, label, { exact: true })).click();
}

/** Appearance › a palette. */
async function paletteTo(page: Page, id: "latte" | "slate") {
  await page.locator("[data-account-menu]").click();
  await page.getByRole("menuitem", { name: "Appearance…" }).click();
  await page.locator(`[data-palette-choice="${id}"]`).click();
  await page.keyboard.press("Escape");
}

/** VS Code editor blocks are refused here, so code-server is never
 * fetched: nothing in this spec should ask for one. */
async function noVsCode(page: Page): Promise<unknown[]> {
  const asked: unknown[] = [];
  await page.route("**/api/blocks", async (route) => {
    const body = route.request().postDataJSON() as Record<string, unknown> | null;
    if (route.request().method() !== "POST" || body?.type !== "editor") return route.fallback();
    asked.push(body);
    await route.fulfill({ status: 400, contentType: "application/json", body: JSON.stringify({ error: "not in this spec" }) });
  });
  return asked;
}

let term: PaneId = 0;
let block: PaneId = 0;

test("New editor block offers VS Code and nvim, picked by the Editor preference, with no Labs flag", async ({ page }) => {
  await noVsCode(page);
  await reset(page);
  expect(existsSync(join(state, "flags.json"))).toBe(false);
  [term] = await panes(page);
  await run(page, term, `cd ${proj} && echo in-$((40+2))`, "in-42");
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.cwd(p), term)).toBe(proj);
  // Nothing chosen: VS Code.
  let drawer = await editorDrawer(page);
  await expect(drawer.locator('[data-editor-app="vscode"]')).toHaveAttribute("aria-checked", "true");
  await expect(drawer.locator('[data-editor-app="nvim"]')).toHaveAttribute("aria-checked", "false");
  await expect(drawer.locator("[data-add-pane-destination]")).toContainText("VS Code on");
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  // The preference nvim: nvim.
  await choose(page, "nvim");
  drawer = await editorDrawer(page);
  await expect(drawer.locator('[data-editor-app="nvim"]')).toHaveAttribute("aria-checked", "true");
  await expect(drawer.locator("[data-add-pane-destination]")).toContainText(`nvim on`);
  await expect(drawer.locator("[data-add-pane-destination]")).toContainText(`in ${proj}`);
  await drawer.locator("[data-add-pane-create]").click();
  await expect(drawer).toBeHidden();
  await expect.poll(() => panes(page)).toHaveLength(2);
  block = (await panes(page)).find((p) => p !== term)!;
  await expect.poll(() => active(page)).toBe(block);
  // A terminal typed as an editor, in the folder, with arugula.nvim
  // joined as the pane (so it can be followed), in the page's colors.
  await expect.poll(() => info(page, block).then((i) => i?.editor?.app ?? null), { timeout: 15_000 }).toBe("nvim");
  const i = (await info(page, block))!;
  expect([i.type, i.app, i.kind, i.cwd]).toEqual(["terminal", "nvim", "editor", proj]);
  await expect.poll(() => info(page, block).then((i) => !!i?.editor && !i.editor.no_follow), { timeout: 15_000 }).toBe(true);
  await expect(page.locator(`[data-pane-header="${block}"] [data-pane-glyph="nvim"]`)).toBeVisible();
  await expect.poll(() => nvimSays(block).then((v) => v && [v.plugin, v.scheme, v.background, v.bg]), { timeout: 15_000 }).toEqual([true, "arugula", "dark", SLATE_BG]);
});

test("Open in editor opens a file into the nvim block, at its line", async ({ page }) => {
  const vscode = await noVsCode(page);
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  // A new browser: the preference again.
  await choose(page, "nvim");
  await page.evaluate((p) => window.__arugula.client.setActive(p), block);
  // A file block beside it, in its tab; its Open in editor goes to the
  // tab's nvim, the block.
  const file = await page.evaluate(
    ([b, path]) => window.__arugula.client.openBlock({ type: "file", config: { path, line: 37 }, from_pane: b, split: b }, "couldn't open that file"),
    [block, join(proj, "src/b.txt")] as const,
  );
  expect(file).not.toBeNull();
  await expect(page.locator(`[data-file-block="${file}"] [data-open-editor]`)).toBeVisible({ timeout: 10_000 });
  const before = (await panes(page)).length;
  await page.locator(`[data-file-block="${file}"] [data-open-editor]`).click();
  await expect.poll(() => active(page)).toBe(block);
  await expect.poll(() => info(page, block).then((i) => i?.title ?? null), { timeout: 10_000 }).toBe("b.txt:37 (nvim)");
  expect((await panes(page)).length).toBe(before);
  const v = (await nvimSays(block))!;
  expect([v.file, v.line]).toEqual([join(proj, "src/b.txt"), 37]);
  expect(vscode).toEqual([]);
  // The file block's done its part.
  await page.evaluate((f) => window.__arugula.client.intent({ op: "close_pane", pane: f! }), file);
  await expect.poll(() => panes(page)).toHaveLength(before - 1);
});

test("a palette change recolors it at once, light and dark, without a restart", async ({ page }) => {
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  const pid = (await nvimSays(block))!;
  await paletteTo(page, "latte");
  await expect.poll(() => nvimSays(block).then((v) => v && [v.scheme, v.background, v.bg]), { timeout: 10_000 }).toEqual(["arugula", "light", LATTE_BG]);
  // The same nvim, still on its file.
  const v = (await nvimSays(block))!;
  expect([v.file, v.line]).toEqual([pid.file, pid.line]);
  await paletteTo(page, "slate");
  await expect.poll(() => nvimSays(block).then((v) => v && [v.background, v.bg]), { timeout: 10_000 }).toEqual(["dark", SLATE_BG]);
  await paletteTo(page, "latte");
  await expect.poll(() => nvimSays(block).then((v) => v && v.bg), { timeout: 10_000 }).toBe(LATTE_BG);
});

test("it survives a daemon restart, and a reboot, on its file and line, still an editor", async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  // A new browser has its own palette, which it sends: Latte, as before.
  // After the restart this page sends nothing (it's seen the block), so
  // the colors that come back are the ones the daemon kept.
  await paletteTo(page, "latte");
  await expect.poll(() => nvimSays(block).then((v) => v && v.bg), { timeout: 10_000 }).toBe(LATTE_BG);
  // Somewhere else in the file first: the restart follows the cursor.
  await page.evaluate((p) => window.__arugula.client.setActive(p), block);
  await page.evaluate((p) => window.__arugula.client.request("POST", `/api/panes/${p}/send`, { text: "\x1b45G" }), block);
  await expect.poll(() => info(page, block).then((i) => i?.title ?? null), { timeout: 10_000 }).toBe("b.txt:45 (nvim)");
  // ...which its restart policy has caught up with.
  await expect.poll(() => info(page, block).then((i) => (i?.policy.kind === "hook" ? i.policy.command : "")), { timeout: 10_000 }).toContain("+45 -- ");

  const back = async () => {
    await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected), { timeout: 20_000 }).toBe(true);
    await expect.poll(() => info(page, block).then((i) => i && [i.app, i.kind, i.title]), { timeout: 20_000 }).toEqual(["nvim", "editor", "b.txt:45 (nvim)"]);
    await expect.poll(() => info(page, block).then((i) => !!i?.editor && !i.editor.no_follow), { timeout: 20_000 }).toBe(true);
    await expect.poll(() => nvimSays(block).then((v) => v && [v.file, v.line, v.plugin, v.scheme, v.bg]), { timeout: 20_000 }).toEqual([
      join(proj, "src/b.txt"),
      45,
      true,
      "arugula",
      LATTE_BG,
    ]);
  };
  // A restart.
  await stop();
  await start();
  await back();
  // A reboot: its nvim is gone too, and comes back by its restart policy.
  await stop();
  await sh("pkill", ["-f", join(state, "editor/nvim/block.lua")]).catch(() => {});
  await expect.poll(() => readdirSync(runDir).filter((n) => n.startsWith("nvim.")).length, { timeout: 10_000 }).toBe(0);
  await start();
  await page.reload();
  await back();
});

test("the swarm shows it as an editor, with the nvim mark", async ({ page }) => {
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  await expect.poll(() => page.evaluate((b) => window.__arugula.fleet.panes.find((p) => p.id === b && p.info.app === "nvim")?.info.kind ?? null, block)).toBe("editor");
  const key = await page.evaluate((b) => window.__arugula.fleet.panes.find((p) => p.id === b && p.info.app === "nvim")!.key, block);
  await page.waitForTimeout(1500);
  const pos = (await page.evaluate((k) => (window.__arugula.swarm as { screenOf(k: string): { x: number; y: number } }).screenOf(k), key))!;
  await page.mouse.move(pos.x, pos.y);
  await expect(page.locator(".swarm-peek [data-swarm-nvim]")).toBeVisible({ timeout: 10_000 });
  await expect(page.locator(".swarm-peek .ph b")).toContainText("b.txt:45");
});

test("Open in editor with no nvim open, the preference nvim: an nvim block, the plugin joined", async ({ page }) => {
  const vscode = await noVsCode(page);
  await reset(page);
  const [shell] = await panes(page);
  await run(page, shell, `cd ${proj} && echo in-$((40+2))`, "in-42");
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.cwd(p), shell)).toBe(proj);
  await choose(page, "nvim");
  await page.locator(`[data-pane-more="${shell}"]`).click();
  await (await menuItem(page, "Open in editor")).click();
  await expect.poll(() => panes(page)).toHaveLength(2);
  const made = (await panes(page)).find((p) => p !== shell)!;
  await expect.poll(() => active(page)).toBe(made);
  await expect.poll(() => info(page, made).then((i) => i && [i.app, i.kind, i.cwd]), { timeout: 15_000 }).toEqual(["nvim", "editor", proj]);
  await expect.poll(() => info(page, made).then((i) => !!i?.editor && !i.editor.no_follow), { timeout: 15_000 }).toBe(true);
  await expect.poll(() => nvimSays(made).then((v) => v && [v.plugin, v.scheme]), { timeout: 15_000 }).toEqual([true, "arugula"]);
  expect(vscode).toEqual([]);
});
