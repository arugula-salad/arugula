// Hints from facts, batch 2 (DECISIONS.md "Hints come from facts, once per
// account"; hints.spec.ts has the first): the chips in a pane's header, one
// test each, driving the fact and taking the action. A command's mark (its
// menu), a pane the Share dialog warned may show secrets (Private), a
// commit or a checkout (Changes), vim or `code` (the editor beside it),
// and, last, another machine in the host menu (Split right on it).
//
// Every test but the mark's has seen the mark's hint already: any command
// makes one. Hints keep quiet under a test runner unless a spec asks
// (hints/index.ts): this one does, `__arugulaHints`.

import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem, panes, reset, run, text, type } from "./helpers";
import type { PaneId } from "../src/shared/proto";
import { ANY, daemonPort } from "./ports";

test.afterAll(closeContexts);

let homeUrl = "";
const dirs: string[] = [];
const daemons: ChildProcess[] = [];
const temp = (what: string) => {
  const d = mkdtempSync(join(tmpdir(), `arugula-e2e-hints-panes-${what}-`));
  dirs.push(d);
  return d;
};

test.use({ baseURL: async ({}, use) => use(homeUrl) });
test.describe.configure({ mode: "serial" });

/** A daemon named `name` (block sites on, so blocks open beside a pane). */
async function start(name: string, extra: string[] = []): Promise<string> {
  const state = temp(name);
  const d = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--block-listen", ANY, "--name", name, "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/tailscaled.sock"],
      ...extra,
    ],
    { stdio: "ignore" },
  );
  daemons.push(d);
  const url = `http://127.0.0.1:${await daemonPort(state, d)}`;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${url}/api/host`)).ok) return url;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`daemon ${name} did not start`);
}

test.beforeAll(async () => {
  // An editor block opens; its VS Code never starts (and isn't downloaded).
  homeUrl = await start("home", ["--code-server", "/bin/false"]);
});

test.afterAll(() => {
  for (const d of daemons) d.kill("SIGKILL");
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
});

/** This browser has seen `ids` already (its first page only). */
const seenAlready = (page: Page, ids: string[]) =>
  page.addInitScript((ids) => {
    Object.assign(window, { __arugulaHints: true });
    if (!localStorage.getItem("arugula.seen")) localStorage.setItem("arugula.seen", JSON.stringify(Object.fromEntries(ids.map((id) => [`seen:${id}`, 1]))));
  }, ids);

const chipOf = (page: Page, pane: PaneId, id: string) => page.locator(`[data-pane-header="${pane}"] [data-hint="${id}"]`);
const blockOf = (page: Page, type: string) =>
  page.evaluate((t) => window.__arugula.client.state!.panes.find((p) => p.type === t)?.id ?? null, type);

test("a command's first mark: right-click it to copy its output or run it again", async ({ page }) => {
  await seenAlready(page, []);
  await reset(page);
  const [term] = await panes(page);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  await run(page, term, "echo mark-$((2+3))", "mark-5");
  const chip = chipOf(page, term, "command-mark");
  await expect(chip).toContainText("Right-click the mark: copy its output, run it again");
  await chip.locator("[data-hint-act]").click();
  await expect(page.getByRole("menu")).toContainText("Copy output");
  await (await menuItem(page, "Run again")).click();
  await expect.poll(async () => (await text(page, term)).match(/^mark-5$/gm)?.length).toBe(2);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
});

test("the Share dialog warns a pane may show secrets: Make this pane private", async ({ page }) => {
  await seenAlready(page, ["command-mark"]);
  await reset(page);
  const [term] = await panes(page);
  // A made-up token, printed (gitleaks:allow).
  await run(page, term, "printf 'GH=%s\\n' ghp_0123456789abcdefghijABCDEFGHIJ012345", "GH=ghp_");
  await page.locator(`[data-pane="${term}"]`).click({ button: "right", position: { x: 60, y: 60 } });
  await (await menuItem(page, "Share this pane…")).click();
  await expect(page.locator(`[data-secret="${term}"]`)).toContainText("a GitHub token");
  // Not while the dialog is open.
  await page.waitForTimeout(500);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  await page.getByRole("button", { name: "Done" }).click();

  const chip = chipOf(page, term, "private-pane");
  await expect(chip).toContainText("Make this pane private: nobody you share with sees it");
  await chip.locator("[data-hint-act]").click();
  await (await menuItem(page, "Private (only you see it)")).click();
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.private, term)).toBe(true);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
});

test("a commit in a pane: See what changed beside it, Changes", async ({ page }) => {
  const repo = temp("repo");
  const git = (...args: string[]) => {
    const r = spawnSync("git", ["-c", "user.name=a", "-c", "user.email=a@example.com", ...args], { cwd: repo });
    expect(r.status, String(r.stderr)).toBe(0);
  };
  git("init", "-q", "-b", "main");
  git("commit", "-q", "--allow-empty", "-m", "one");
  // Its `cd` in a fresh pane is the directory hint's fact.
  await seenAlready(page, ["command-mark", "cd-picker"]);
  await reset(page);
  const [term] = await panes(page);
  await type(page, term, `cd ${repo}\n`);
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.branch, term)).toBe("main");
  await page.waitForTimeout(500);
  await expect(page.locator("[data-hint]")).toHaveCount(0);

  await type(page, term, "git -c user.name=a -c user.email=a@example.com commit -q --allow-empty -m two\n");
  const chip = chipOf(page, term, "repo-changes");
  await expect(chip).toContainText("See what changed beside it");
  await chip.locator("[data-hint-act]").click();
  await expect.poll(() => blockOf(page, "diff")).not.toBeNull();
  await expect(page.locator("[data-hint]")).toHaveCount(0);
});

test("vim in a pane: Open the editor beside it", async ({ page }) => {
  // No vim here: `sleep` under nvim's name is the foreground command.
  const bin = temp("bin");
  symlinkSync("/bin/sleep", join(bin, "nvim"));
  await seenAlready(page, ["command-mark"]);
  await reset(page);
  const [term] = await panes(page);
  await type(page, term, `${join(bin, "nvim")} 60\n`);
  const chip = chipOf(page, term, "editor-beside");
  await expect(chip).toContainText("Open the editor beside it");
  await chip.locator("[data-hint-act]").click();
  await expect.poll(() => blockOf(page, "editor")).not.toBeNull();
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  // Stopped, so the next page doesn't find it running.
  await page.evaluate((p) => window.__arugula.client.input(p, new TextEncoder().encode("\x03")), term);
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.command ?? null, term)).toBeNull();
});

test("`code` typed in a pane is the same fact", async ({ page }) => {
  await seenAlready(page, ["command-mark"]);
  await reset(page);
  const [term] = await panes(page);
  // VS Code's command line, as a shell function: nothing to install.
  await run(page, term, "code() { echo opened-$((4+4)); }", "code()");
  await run(page, term, "code .", "opened-8");
  await expect(chipOf(page, term, "editor-beside")).toContainText("Open the editor beside it");
});

test("another machine in the host menu: Split right on it", async ({ page }) => {
  const other = await start("other", ["--allow-origin", homeUrl]);
  const res = await fetch(`${homeUrl}/api/hosts`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: "other", urls: [other] }),
  });
  expect(res.ok).toBe(true);
  await seenAlready(page, ["command-mark"]);
  await reset(page);
  const [term] = await panes(page);
  const chip = chipOf(page, term, "remote-split");
  await expect(chip).toContainText("Split a shell on that machine into this tab");
  await expect(chip.locator("[data-hint-act]")).toHaveText("Split right on other");
  await chip.locator("[data-hint-act]").click();
  await expect.poll(() => blockOf(page, "remote")).not.toBeNull();
  await expect(page.locator("[data-hint]")).toHaveCount(0);
});
