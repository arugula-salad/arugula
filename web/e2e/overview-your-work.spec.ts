// Your work on the Overview (Arugula #1400): a new account's personal
// project, with one joined machine and Claude Code history on it, shows that
// history under the field where "Nothing in flight." was, by repo, read from
// the machine itself; Continue here opens the conversation's pane on its
// machine's page, as Open in Panes does. A team's project shows none.
//
// The app's frame (app-frame-page.tsx) over a daemon of this spec's own,
// with a Claude and a Codex directory of its own, so what it lists is only
// what it seeds. The platform's projects are the fixture's: Default is
// sam's personal project, Arugula a team's; no boards.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { ANY, daemonPort } from "./ports";

const PERSONAL = "prj_aaaa0001";
const TEAM = "prj_aaaa0002";

test.describe.configure({ mode: "serial" });
// A page a route fulfils has no address of its own, so Chrome counts it as
// public and won't let it reach the daemon on loopback without this.
test.use({ permissions: ["local-network-access"] });

let APP = "";
let state = "";
let claude = "";
let codex = "";
let work = "";
let daemon: ChildProcess | undefined;
test.use({ baseURL: async ({}, use) => use(APP) });

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "ilg-e2e-ovyw-"));
  claude = mkdtempSync(join(tmpdir(), "ilg-e2e-ovyw-claude-"));
  codex = mkdtempSync(join(tmpdir(), "ilg-e2e-ovyw-codex-"));
  work = mkdtempSync(join(tmpdir(), "ilg-e2e-ovyw-work-"));
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

/** A conversation Claude Code had at a terminal in `repo` (a folder under
 * the run's work), `ago` minutes ago. */
function seed(repo: string, prompt: string, ago: number): string {
  const cwd = join(work, repo);
  mkdirSync(cwd, { recursive: true });
  const id = crypto.randomUUID();
  const dir = join(claude, "projects", cwd.replace(/[/.]/g, "-"));
  mkdirSync(dir, { recursive: true });
  const at = new Date(Date.now() - ago * 60_000);
  const base = (type: string, uuid: string, parentUuid: string | null) => ({
    type, uuid, parentUuid, sessionId: id, cwd, gitBranch: "main", version: "2.1.288",
    entrypoint: "cli", timestamp: at.toISOString(), isSidechain: false,
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

/** The app on `project`'s Overview: no boards, and no sessions by card. */
async function overview(page: Page, project: string) {
  for (const p of [PERSONAL, TEAM]) {
    await page.route(`**/api/projects/${p}/boards`, (r) => r.fulfill({ json: { project: p, boards: [] } }));
    await page.route(`**/api/projects/${p}/boards/views`, (r) => r.fulfill({ json: [] }));
    await page.route(`**/api/projects/${p}/sessions/by-card`, (r) => r.fulfill({ status: 404, json: { error: "not found" } }));
  }
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(/\/p\/prj_/, (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(`/p/${project}`);
  await expect(page.locator("[data-overview]")).toBeVisible({ timeout: 20_000 });
}

test("a personal project's Overview shows its machine's Claude Code history, and Continue here opens it in a pane there", async ({ page }) => {
  const older = seed("ledgerly", "Add a --dry-run flag to bin/reconcile", 30);
  const newest = seed("ledgerly", "CSV import drops the first row when the file starts with a BOM", 3);
  const kv = seed("tiny-kv", "Implement leveled compaction", 60);

  await overview(page, PERSONAL);
  const yw = page.locator("[data-ov-your-work]");
  await expect(yw.locator(".ov-loose-head b")).toHaveText("Your work");
  // By repo, newest first, read from the machine: where "Nothing in flight." was.
  await expect(yw.locator(".yw-repo-name")).toHaveText(["ledgerly", "tiny-kv"]);
  await expect(yw.locator(`[data-repo="${join(work, "ledgerly")}"] [data-conversation]`)).toHaveCount(2);
  expect(await yw.locator("[data-conversation]").evaluateAll((els) => els.map((e) => e.getAttribute("data-conversation")))).toEqual([newest, older, kv]);
  await expect(yw.locator(`[data-conversation="${kv}"] .yw-first`)).toHaveText("Implement leveled compaction");
  await expect(page.locator("[data-ov-empty]")).toHaveCount(0);
  // One machine: no machine headings, the repo is the only one.
  await expect(yw.locator("[data-host-head]")).toHaveCount(0);

  // Continue here: a terminal tab named after it on the machine's page, on its pane.
  const row = yw.locator(`[data-conversation="${older}"]`);
  await row.click();
  await row.locator("[data-continue-here]").click();
  await expect(page).toHaveURL(new RegExp(`/p/${PERSONAL}[^/]*/machines.*pane=\\d+`));
  const pane = Number(new URL(page.url()).searchParams.get("pane"));
  await expect
    .poll(() =>
      page.evaluate((pane) => {
        const c = (window as unknown as { __app: { client: { state: { tabs: { id: number; name: string; layout: { panes: [number, unknown][] } }[] } | null } } }).__app.client;
        return c.state?.tabs.find((t) => t.layout.panes.some(([p]) => p === pane))?.name ?? null;
      }, pane),
    )
    .toBe("Add a --dry-run flag to bin/reconcile");
});

test("a team's project lists no one's conversations", async ({ page }) => {
  seed("ledgerly", "Bump Rails to 7.2", 5);
  await overview(page, TEAM);
  await expect(page.locator("[data-ov-empty]")).toBeVisible();
  await expect(page.locator("[data-ov-your-work]")).toHaveCount(0);
});
