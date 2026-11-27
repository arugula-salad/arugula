// Your work (Arugula #1396): on the daemon's own page, the owner's home with
// no session open is their Claude Code and Codex conversations, by repo,
// newest first, three a repo until "N more", every origin a person had
// (not `claude -p` runs); Continue here resumes one in a terminal tab. With
// none, it says so, and New terminal makes one. Once a pane is open, it's
// a dialog from the palette. At 360px a row's facts go under its prompts
// and wrap.
//
// Under a test runner the home is the plain empty layout (as Getting
// started and the hints keep quiet there): this spec opts in to the first
// screen a person sees, `__arugulaFirstRun`. Its daemon has a Claude and a
// Codex directory of its own, so what it lists is only what it seeds.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, open, panes, tab } from "./helpers";
import { ANY, daemonPort } from "./ports";

test.afterAll(closeContexts);
test.describe.configure({ mode: "serial" });

let APP = "";
let state = "";
let claude = "";
let codex = "";
let work = "";
let daemon: ChildProcess | undefined;
test.use({ baseURL: async ({}, use) => use(APP) });
test.beforeEach(({ page }) => page.addInitScript(() => Object.assign(window, { __arugulaFirstRun: true })));

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "ilg-e2e-yw-"));
  claude = mkdtempSync(join(tmpdir(), "ilg-e2e-yw-claude-"));
  codex = mkdtempSync(join(tmpdir(), "ilg-e2e-yw-codex-"));
  work = mkdtempSync(join(tmpdir(), "ilg-e2e-yw-work-"));
  mkdirSync(join(claude, "sessions"));
  daemon = spawn("../target/debug/arugulad", ["--listen", ANY, "--shell", "bash --norc --noprofile"], {
    stdio: "ignore",
    env: { ...process.env, ARUGULA_STATE_DIR: state, CLAUDE_CONFIG_DIR: claude, CODEX_HOME: codex },
  });
  APP = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  for (const d of [state, claude, codex, work]) if (d) rmSync(d, { recursive: true, force: true });
});

/** A conversation Claude Code had in `repo` (a folder under the run's
 * work), `ago` minutes ago, started from `entrypoint` (none: unsaid). */
function seed(repo: string, prompt: string, ago: number, entrypoint: string | null = "cli"): string {
  const cwd = join(work, repo);
  mkdirSync(cwd, { recursive: true });
  const id = crypto.randomUUID();
  const dir = join(claude, "projects", cwd.replace(/[/.]/g, "-"));
  mkdirSync(dir, { recursive: true });
  const at = new Date(Date.now() - ago * 60_000);
  const base = (type: string, uuid: string, parentUuid: string | null) => ({
    type, uuid, parentUuid, sessionId: id, cwd, gitBranch: "main", version: "2.1.288",
    ...(entrypoint === null ? {} : { entrypoint }), timestamp: at.toISOString(), isSidechain: false,
  });
  const lines = [
    { ...base("user", "u1", null), message: { role: "user", content: prompt } },
    { ...base("assistant", "a1", "u1"), message: { id: "m1", role: "assistant", model: "claude-haiku-4-5-20251001", content: [{ type: "text", text: "Done." }] } },
  ];
  const file = join(dir, `${id}.jsonl`);
  writeFileSync(file, lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
  utimesSync(file, at, at);
  return id;
}

/** This test's process holds the session, as a running Claude Code would
 * (Linux's start time: the specs run there). */
function hold(id: string) {
  const stat = readFileSync(`/proc/${process.pid}/stat`, "utf8");
  const procStart = stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/)[19];
  writeFileSync(
    join(claude, "sessions", `${process.pid}.json`),
    JSON.stringify({ pid: process.pid, sessionId: id, procStart, kind: "interactive", entrypoint: "cli", status: "idle" }),
  );
}

/** The page with every session closed: the home with nothing open. */
async function nothingOpen(page: Page) {
  await open(page);
  await page.evaluate(() => {
    const c = window.__arugula.client;
    for (const s of c.state!.sessions) c.intent({ op: "close_session", session: s.id });
  });
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.sessions.length)).toBe(0);
}

const home = (page: Page) => page.locator("[data-your-work-home]");

test("no conversations: Your work says so, and New terminal opens one", async ({ page }) => {
  await nothingOpen(page);
  await expect(home(page).locator(".page-head h1")).toHaveText("Your work");
  const empty = home(page).locator("[data-your-work-empty]");
  await expect(empty.locator(".empty-state-title")).toHaveText("No conversations yet.");
  await empty.getByRole("button", { name: "New terminal" }).click();
  await expect.poll(() => panes(page)).toHaveLength(1);
  await expect(home(page)).toHaveCount(0);
});

test("with history, the first screen is it: by repo, newest first, three a repo; Continue here resumes one", async ({ page }) => {
  // ledgerly: four, the newest 3 minutes ago; tiny-kv: newer, one of them
  // from an entrypoint Claude Code doesn't name; and a `claude -p` run.
  const ledgerly = [3, 10, 20, 30].map((ago, i) => seed("ledgerly", `ledgerly task ${i + 1}`, ago));
  const kv = seed("tiny-kv", "Implement leveled compaction", 1);
  const other = seed("tiny-kv", "Profile the zsh start", 2, null);
  const run = seed("tiny-kv", "a scripted run", 0, "sdk-cli");

  await nothingOpen(page);
  const yw = home(page);
  await expect(yw.locator(".page-head h1")).toHaveText("Your work");
  await expect(yw.locator(".yw-repo-name")).toHaveText(["tiny-kv", "ledgerly"]);
  await expect(yw.locator(`[data-repo="${join(work, "tiny-kv")}"] [data-conversation]`)).toHaveCount(2);
  await expect(yw.locator(`[data-conversation="${other}"] .yw-first`)).toHaveText("Profile the zsh start");
  await expect(yw.locator(`[data-conversation="${run}"]`)).toHaveCount(0);

  // Three of ledgerly's, newest first, then the fourth on asking.
  const rows = yw.locator(`[data-repo="${join(work, "ledgerly")}"] [data-conversation]`);
  await expect(rows).toHaveCount(3);
  expect(await rows.evaluateAll((els) => els.map((e) => e.getAttribute("data-conversation")))).toEqual(ledgerly.slice(0, 3));
  await yw.getByRole("button", { name: "1 more in ledgerly" }).click();
  await expect(rows).toHaveCount(4);

  // The newest is where the keyboard starts; Continue here on another.
  await expect(yw.locator(`[data-conversation="${kv}"]`)).toHaveAttribute("aria-selected", "true");
  const row = yw.locator(`[data-conversation="${ledgerly[1]}"]`);
  await row.click();
  await expect(row).toHaveAttribute("aria-selected", "true");
  await row.locator("[data-continue-here]").click();
  await expect(home(page)).toHaveCount(0);
  await expect.poll(async () => (await tab(page))?.name).toBe("ledgerly task 2");
  await expect.poll(() => panes(page)).toHaveLength(1);

  // With a pane open, it's one click away: the palette's Your work.
  await page.keyboard.press("Control+Shift+KeyK");
  await page.locator('[data-command="Places/your-work"]').click();
  const dialog = page.getByRole("dialog", { name: "Your work" });
  // The folder of the pane it's opened from first.
  await expect(dialog.locator(".yw-repo-name")).toHaveText(["ledgerly", "tiny-kv"]);
  await expect(dialog.locator(`[data-conversation="${ledgerly[1]}"]`)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  rmSync(join(claude, "projects"), { recursive: true, force: true });
});

test.describe("at 360px", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("a row's facts and its button go under its prompts and wrap, not squeeze", async ({ page }) => {
    const id = seed("a-repository-with-a-long-name", "The receipt camera crashes on iOS 18 when you deny photo access; reproduce it in the simulator", 1);
    hold(id);
    try {
      await nothingOpen(page);
      const row = home(page).locator(`[data-conversation="${id}"]`);
      await expect(row.locator("[data-live]")).toHaveText("● running in another terminal");
      await row.click();
      await expect(row.locator("[data-move-here]")).toHaveText("Move here");
      // Nothing runs off the screen: the page doesn't scroll sideways, and
      // the mark and the button sit inside the row, under its prompts.
      expect(await home(page).evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
      const box = (await row.boundingBox())!;
      const first = (await row.locator(".yw-first").boundingBox())!;
      for (const part of [row.locator("[data-live]"), row.locator("[data-move-here]")]) {
        const b = (await part.boundingBox())!;
        expect(b.x).toBeGreaterThanOrEqual(box.x);
        expect(b.x + b.width).toBeLessThanOrEqual(box.x + box.width + 0.5);
        expect(b.y).toBeGreaterThan(first.y);
      }
    } finally {
      rmSync(join(claude, "sessions", `${process.pid}.json`), { force: true });
    }
  });
});
