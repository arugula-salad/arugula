// Accessibility (#401) on a machine's own page: panes (a terminal and a Doc
// beside it), the swarm, the Docs page, the Agents page's two tabs, and the
// settings (Appearance and Developer settings), each at 1440 and 400 px, light and dark. A daemon
// of its own, so what earlier specs in the shard left on the suite's daemon
// doesn't change what's scanned. What's checked and the known list:
// a11y.ts.

import type { ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { everyWay, prepare } from "./a11y";
import { fill, newDoc, type Post } from "./doc-steps";
import { closeContexts, open, panes, ready, reset } from "./helpers";
import { keyHeaders } from "./local-token";
import { daemon } from "./phones";

let dir = "";
let url = "";
let proc: ChildProcess | undefined;

// Four loads and scans a test.
test.describe.configure({ timeout: 90_000 });
test.use({ baseURL: async ({}, use) => use(url) });

const post: Post = async (path, body) => {
  const r = await fetch(url + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const text = await r.text();
  expect(r.ok, `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

// One session: a terminal with a Doc beside it, which the Docs page lists.
test.beforeAll(async ({ browser }) => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-a11y-"));
  ({ proc, url } = await daemon(join(dir, "state"), ["--tailscale-socket", "/nonexistent/sock"]));
  const page = await browser.newPage({ baseURL: url, extraHTTPHeaders: keyHeaders });
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post, term, "Release plan");
  await fill(post, doc, "s1", "Ship the installer this week.");
  await expect(page.locator(`[data-doc-block="${doc}"] .doc-title`)).toHaveText("Release plan", { timeout: 15_000 });
  await page.context().close();
});

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  proc?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

/** The page, connected, with the terminal drawn. */
async function home(page: Page) {
  await open(page);
  const [term] = await panes(page);
  await ready(page, term);
}

test("panes", async ({ page }) => {
  await prepare(page);
  await everyWay(page, "panes", async (width) => {
    await home(page);
    if (width === "desktop") await expect(page.locator("[data-doc-block] .doc-title")).toHaveText("Release plan");
    else await expect(page.locator("[data-pane]").first()).toBeVisible();
  });
});

// With the sidebar closed the header names the session instead, with its menu's caret (#457).
test("panes, sessions sidebar closed", async ({ page }) => {
  await prepare(page);
  await everyWay(page, "panes-sidebar-closed", async (width) => {
    await home(page);
    if (width !== "desktop") return;
    if (await page.locator("[data-session-sidebar]").count()) await page.locator(".ws-head [data-open-sessions]").click();
    await expect(page.locator(".ws-crumb-caret")).toBeVisible();
  });
});

test("swarm", async ({ page }) => {
  await prepare(page);
  await everyWay(page, "swarm", async () => {
    await page.goto("/#swarm");
    await expect(page.locator(".swarm")).toBeVisible();
    await expect.poll(() => page.evaluate(() => window.__arugula.client.connected)).toBe(true);
  });
});

test("docs", async ({ page }) => {
  await prepare(page);
  await everyWay(page, "docs", async () => {
    await page.goto("/#docs");
    await expect(page.locator("[data-docs-page] [data-doc]")).toHaveCount(1);
  });
});

test("agents: Recipes", async ({ page }) => {
  // A project of its own, so the home directory's recipes aren't what's made.
  const project = join(dir, "project");
  mkdirSync(project);
  await post("/api/a2a/recipes/projects", { dir: project });
  await post("/api/a2a/recipes/save", { dir: project, name: "fixer", description: "Fixes failing tests", tools: ["Read"], prompt: "You fix tests." });
  await prepare(page);
  await everyWay(page, "agents-recipes", async () => {
    await page.goto("/#agents");
    await expect(page.locator("[data-agents-page=recipes] [data-recipe=fixer]")).toBeVisible();
  });
});

test("agents: Team, with a task waiting", async ({ page }) => {
  // Other people and machines, as the page is told of them.
  await page.route(/\/api\/a2a\/catalog/, (r) =>
    r.fulfill({
      json: {
        machines: [
          { machine: "m1", name: "buildbox", owner: "", here: true, online: true, note: null, fetched_ms: null, agents: [{ name: "reviewer", description: "Reviews diffs" }] },
          { machine: "m2", name: "spare", owner: "Dana", here: false, online: false, note: "offline", fetched_ms: null, agents: [] },
        ],
      },
    }),
  );
  await page.route(/\/api\/a2a\/waiting$/, (r) => r.fulfill({ json: { waiting: [{ id: "t1", history: [{ parts: [{ text: "Check the nightly build" }] }], metadata: { arugula: { agent: "reviewer", caller: "Dana" } } }] } }));
  await page.route(/\/api\/a2a\/grants$/, (r) => r.fulfill({ json: [{ account: "a9", name: "Sam", agent: "reviewer", until_ms: Date.now() + 3_600_000 }] }));
  await prepare(page);
  await everyWay(page, "agents-team", async () => {
    await page.goto("/#agents=team");
    await expect(page.locator("[data-agents-page=team] [data-waiting=t1]")).toBeVisible();
    await expect(page.locator("[data-agents-page=team] [data-agent=reviewer]")).toBeVisible();
  });
});

test("settings: Appearance", async ({ page }) => {
  await prepare(page);
  await everyWay(page, "appearance", async () => {
    await home(page);
    // The command palette has the settings menu's items at any width.
    await page.keyboard.press("ControlOrMeta+Shift+KeyP");
    const palette = page.getByRole("dialog", { name: "Command palette" });
    await palette.getByRole("combobox", { name: "Command" }).fill("Appearance");
    await expect(palette.getByRole("option").first()).toContainText("Appearance");
    await palette.getByRole("combobox", { name: "Command" }).press("Enter");
    await expect(page.locator("[data-appearance]")).toBeVisible();
  });
});

test("settings: Developer settings", async ({ page }) => {
  await prepare(page);
  await everyWay(page, "developer", async () => {
    await page.goto("/#developer");
    await expect(page.locator(".prompt [data-flag]").first()).toBeVisible();
  });
});
