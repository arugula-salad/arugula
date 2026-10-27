// Hints from facts (DECISIONS.md "Hints come from facts, once per
// account"), with the port chip: a port listening in a pane is the fact;
// its pane's header says so once, with Open beside; once seen it never
// comes back, not after a reload either; × dismisses it; and with Tips off
// in Settings nothing is said until they're on again. Off control, "seen"
// is this browser's (localStorage); the account's is control's
// `/api/me/prefs` (crates/control/src/prefs.rs, its own tests).
//
// Then Discovery 3's, one test each, driving the fact and taking the
// action: a PR link printed, a path pasted or files dragged over a pane,
// `cd` in a fresh pane, a Doc block, the bell on a device that could be
// notified, Claude Code conversations here, and (last: it restarts the
// daemon) panes back after a restart. The second person's and the team's
// are in hints-people.spec.ts and hints-team.spec.ts.
//
// Hints keep quiet under a test runner unless a spec asks (hints/index.ts):
// this one does, `__arugulaHints`.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem, open, paneEl, panes, ready, reset, run, seedConversation, text, type } from "./helpers";
import type { PaneId } from "../src/shared/proto";
import { ANY, daemonPort } from "./ports";
import { FakePush } from "./phones";

test.afterAll(closeContexts);

let APP = "";
let PORT = 0;
let state = "";
/** Claude Code's directory, the daemon's own: no conversations until one
 * test seeds one. */
let claude = "";
let daemon: ChildProcess | undefined;
test.use({ baseURL: async ({}, use) => use(APP) });
test.describe.configure({ mode: "serial" });
test.beforeEach(({ page }) => page.addInitScript(() => Object.assign(window, { __arugulaHints: true })));

/** The daemon: on a port of its choosing, then (after a restart) the same. */
async function start() {
  // Block sites, so a port can open beside its terminal.
  daemon = spawn("../target/debug/arugulad", ["--listen", PORT ? `127.0.0.1:${PORT}` : ANY, "--block-listen", ANY, "--shell", "bash --norc --noprofile"], {
    stdio: "ignore",
    env: { ...process.env, ARUGULA_STATE_DIR: state, CLAUDE_CONFIG_DIR: claude },
  });
  PORT ||= await daemonPort(state, daemon);
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

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "ilg-e2e-hints-"));
  claude = mkdtempSync(join(tmpdir(), "ilg-e2e-hints-claude-"));
  mkdirSync(join(claude, "sessions"));
  await start();
});

test.afterAll(async () => {
  daemon?.kill("SIGKILL");
  for (const d of [state, claude]) if (d) rmSync(d, { recursive: true, force: true });
});

const chip = (page: Page, pane: PaneId) => page.locator(`[data-pane-header="${pane}"] [data-hint="port-listening"]`);
const seenHere = (page: Page) => page.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}"));

/** A dev server in `pane`, on a port of its own choosing: that port. */
async function serve(page: Page, pane: PaneId): Promise<number> {
  await type(page, pane, "python3 -m http.server 0 --bind 127.0.0.1\n");
  let port = 0;
  await expect
    .poll(async () => {
      port = Number(/Serving HTTP on \S+ port (\d+)/.exec(await text(page, pane))?.[1] ?? 0);
      return port;
    })
    .toBeGreaterThan(0);
  return port;
}

/** Long enough for the hints to have asked about the pane's ports, more
 * than once (hints/ports.ts asks at most every 2 s a pane). */
const quiet = (page: Page) => page.waitForTimeout(4500);

test("a port listening in a pane: a chip in its header, Open beside opens it, and only once", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const port = await serve(page, term);

  await expect(chip(page, term)).toContainText(`:${port} is listening`);
  await expect(page.locator("[data-hint]")).toHaveCount(1);
  expect(await seenHere(page)).toHaveProperty("seen:port-listening");

  await chip(page, term).locator("[data-hint-act]").click();
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const block = (await panes(page)).find((p) => p !== term)!;
  expect(await page.evaluate((b) => window.__arugula.client.info(b)?.type, block)).toBe("browser");
  await expect(paneEl(page, block).locator(".browser-url input")).toHaveValue(`:${port}/`);
  await expect(chip(page, term)).toHaveCount(0);

  // The server still listens, but it's been said.
  await page.reload();
  await open(page);
  await quiet(page);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "close_pane", pane: p }), block);
});

test("Tips off says nothing; turned on, the chip comes; × dismisses it for good", async ({ page }) => {
  // A browser that turned Tips off and hasn't seen the port chip.
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("hints-spec")) {
      localStorage.setItem("arugula.seen", JSON.stringify({ tips: false }));
      sessionStorage.setItem("hints-spec", "1");
    }
  });
  await reset(page);
  const [term] = await panes(page);
  await serve(page, term);
  await quiet(page);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  expect(await seenHere(page)).toEqual({ tips: false });

  // Settings › Tips.
  await page.locator("[data-account-menu]").click();
  const tips = await menuItem(page, "Tips", { exact: true });
  await expect(tips).toHaveAttribute("aria-checked", "false");
  await tips.click();
  await expect(chip(page, term)).toBeVisible();
  expect(await seenHere(page)).not.toHaveProperty("tips");

  await chip(page, term).locator("[data-hint-dismiss]").click();
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  expect(await seenHere(page)).toHaveProperty("seen:port-listening");
  await quiet(page);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  expect((await panes(page)).length).toBe(1);
});

/** Hint `id`'s chip in `pane`'s header. */
const chipOf = (page: Page, pane: PaneId, id: string) => page.locator(`[data-pane-header="${pane}"] [data-hint="${id}"]`);
const toast = (page: Page, id: string) => page.locator(`.hint-toast[data-hint="${id}"]`);
const blockOf = (page: Page, type: string) =>
  page.evaluate((t) => window.__arugula.client.state!.panes.find((p) => p.type === t)?.id ?? null, type);

test("a PR link printed in a pane: Click opens it here, and Open it does", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  // An Enterprise host's shape (Labs off: GitHub's `/pull/N`), that never answers.
  const url = "https://github.invalid/o/r/pull/7";
  await run(page, term, `printf 'Created %s\\n' ${url}`, `Created ${url}`);
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.link, term)).toBe(url);
  await expect(chipOf(page, term, "pr-link")).toContainText("Click opens it here. Shift-click opens your browser");
  expect(await seenHere(page)).toHaveProperty("seen:pr-link");
  await chipOf(page, term, "pr-link").locator("[data-hint-act]").click();
  await expect.poll(() => blockOf(page, "forge")).not.toBeNull();
  await expect(page.locator("[data-hint]")).toHaveCount(0);
});

test("a path pasted into a pane, or files dragged over it: Attach file…", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  await ready(page, term);
  // Pasted, as Finder's Copy gives a file's path.
  await paneEl(page, term).locator("textarea").evaluate((el) => {
    const data = new DataTransfer();
    data.setData("text/plain", "/Users/me/Desktop/report.pdf");
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  });
  const chip = chipOf(page, term, "attach-file");
  await expect(chip).toContainText("Attach file puts it on this pane's machine");
  const chooser = page.waitForEvent("filechooser");
  await chip.locator("[data-hint-act]").click();
  await (await chooser).setFiles({ name: "hint.txt", mimeType: "text/plain", buffer: Buffer.from("hi") });
  // Its path on the pane's machine, under a name the daemon chose (upload.rs).
  await expect.poll(() => text(page, term)).toMatch(/arugula-uploads\/\d+\/\w+\.txt/);
  await expect(page.locator("[data-hint]")).toHaveCount(0);

  // A new browser: dragging files over the pane is the same fact.
  await page.evaluate(() => localStorage.removeItem("arugula.seen"));
  await page.reload();
  await open(page);
  await paneEl(page, term).evaluate((el) => {
    const data = new DataTransfer();
    data.items.add(new File(["x"], "x.txt"));
    el.querySelector(".xterm")!.dispatchEvent(new DragEvent("dragover", { dataTransfer: data, bubbles: true, cancelable: true }));
  });
  await expect(chipOf(page, term, "attach-file")).toBeVisible();
});

test("cd typed in a fresh pane: the shortcut for Go to directory, and it opens", async ({ page }) => {
  await reset(page);
  // A page that started with this pane: it's old to the hints.
  await page.reload();
  await open(page);
  const [first] = await panes(page);
  // An old pane's cd says nothing; a new one's does.
  await run(page, first, "cd /tmp && echo old-$((1+1))", "old-2");
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), first);
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const fresh = (await panes(page)).find((p) => p !== first)!;
  await ready(page, fresh);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  await type(page, fresh, "cd /tmp\n");
  const chip = chipOf(page, fresh, "cd-picker");
  await expect(chip).toContainText("goes to a directory");
  await chip.locator("[data-hint-act]").click();
  await expect(page.locator(".picker")).toBeVisible();
});

test("the first Doc block: kept on the Docs page, Go to Docs", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await page.evaluate(
    (p) => window.__arugula.client.openBlock({ type: "doc", config: { title: "Notes" }, split: p, local: true }),
    term,
  );
  expect(doc).not.toBeNull();
  const chip = chipOf(page, doc!, "docs-page");
  await expect(chip).toContainText("Kept on the Docs page. Ask any agent for a Doc");
  await chip.locator("[data-hint-act]").click();
  await expect.poll(() => page.evaluate(() => location.hash)).toBe("#docs");
  await page.evaluate((p) => window.__arugula.client.intent({ op: "close_pane", pane: p }), doc!);
});

test("the bell on a device that could be notified: Notify this device turns it on", async ({ page, context }) => {
  const push = await FakePush.start();
  try {
    await push.stub(page, "laptop");
    await context.grantPermissions(["notifications"]);
    await reset(page);
    const [term] = await panes(page);
    await expect(toast(page, "push-device")).toHaveCount(0);
    // A program asks for attention (OSC 9): the bell counts it.
    await type(page, term, "printf '\\033]9;%s\\007' look\n");
    await expect(toast(page, "push-device")).toContainText("Get these on this device, or your phone");
    await toast(page, "push-device").locator("[data-hint-act]").click();
    expect((await push.next("laptop", (p) => p.pane === 0)).body).toBe("Notifications work.");
    await expect(page.locator("[data-hint]")).toHaveCount(0);
  } finally {
    push.close();
  }
});

test("Claude Code conversations here and none open: the picker", async ({ page }) => {
  const { cwd } = seedConversation(claude, "heron", "Heron notes");
  try {
    await reset(page);
    await expect(toast(page, "claude-conversations")).toContainText("Pick up a Claude Code conversation here");
    await toast(page, "claude-conversations").locator("[data-hint-act]").click();
    await expect(page.getByRole("dialog", { name: "Claude Code conversations" })).toContainText("Heron notes");
  } finally {
    rmSync(join(claude, "projects"), { recursive: true, force: true });
    rmSync(cwd, { recursive: true, force: true });
  }
});

test("panes back after a restart: Pane settings › After a restart", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  await run(page, term, "echo before-$((3+3))", "before-6");
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  // Stopped the way a reboot stops it: its shell goes with it.
  const exited = new Promise((r) => daemon!.once("exit", r));
  daemon!.kill("SIGTERM");
  await exited;
  await start();
  await open(page);
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.restored, term)).toBe(true);
  const chip = chipOf(page, term, "after-restart");
  await expect(chip).toContainText("when this machine restarts");
  await chip.locator("[data-hint-act]").click();
  await (await menuItem(page, "Re-run the command, asking first")).click();
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.policy, term)).toEqual({ kind: "rerun", confirm: true });
});
