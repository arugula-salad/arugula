import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fakeManagoat } from "../fixtures/fakes.ts";
import { controlPort, listen } from "./ports";

let base: string, directory: string, control: ChildProcess, github: Server;
// The GitHub account signing in: each test is a fresh control account.
let user = { id: 7, login: "cloud-tester" };
// Managoat (S40): the next setups fail while `failSetup` is set (M120).
let failSetup = false;
const managoat = fakeManagoat(0, "127.0.0.1", { fails: () => (failSetup ? "setup_script exited 1: pnpm: command not found" : null) });
const fake = () => managoat.state();
test.beforeAll(async () => {
  github = createServer((req, res) => {
    const url = new URL(req.url!, "http://github");
    if (url.pathname === "/login/oauth/authorize") {
      const back = new URL(url.searchParams.get("redirect_uri")!);
      back.searchParams.set("code", "test"); back.searchParams.set("state", url.searchParams.get("state")!);
      res.writeHead(302, { location: back.href }).end();
    } else {
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify(url.pathname === "/user" ? user : { access_token: "test-token" }));
    }
  });
  const gh = `http://127.0.0.1:${await listen(github)}`;
  if (!managoat.server.listening) await new Promise((r) => managoat.server.once("listening", r));
  const upstream = `http://127.0.0.1:${(managoat.server.address() as AddressInfo).port}`;
  directory = mkdtempSync(join(tmpdir(), "arugula-cloud-e2e-")); const db = join(directory, "control.db");
  control = spawn("../target/debug/arugula-control", ["--listen", "127.0.0.1:0", "--public-url", "http://127.0.0.1:0", "--db", db, "--static-dir", "dist", "--github-client-id", "test", "--github-client-secret", "test", "--github-url", gh, "--github-api", gh, "--cloud-provider-url", upstream, "--cloud-provider-key", "test", "--cloud-secret-key", "01".repeat(32), "--cloud-model", "anthropic/test", "--cloud-accounts", "*"], { stdio: "ignore" });
  base = `http://127.0.0.1:${await controlPort(db, control)}`;
});
test.afterAll(() => { control?.kill(); github?.close(); managoat.server.close(); if (directory) rmSync(directory, { recursive: true, force: true }); });

// A run from before runs were retired (S40, M119): control's answers for
// it, as a past run reads them.
const pastRun = {
  id: "run_0123abcd", state: "completed", created: 1, result: { diff: { diff: "+fixed" }, files: {} },
  spec: { agent_id: "curated_coding", prompt: "Fix the example", repository: "https://github.com/example/repo", git_ref: "", agent: { name: "Coding agent", instructions: "", mcp_servers: {}, skills: [], secret_names: [] } },
};

test("cloud-only account saves a secret, starts an agent in a new Cloud workspace, approves, and reads a past run", async ({ page }) => {
  test.setTimeout(90000);
  await page.goto(base); await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  // Runs once opened as panes were remembered; they're forgotten on load.
  await page.evaluate(() => localStorage.setItem("arugula.cloud-panes.someone", JSON.stringify(["run_0123abcd"])));
  await page.reload();
  await expect.poll(() => page.evaluate(() => localStorage.getItem("arugula.cloud-panes.someone"))).toBeNull();
  await page.getByRole("button", { name: "Cloud agents", exact: true }).click();
  await page.getByText("Manage agents", { exact: true }).click();
  await page.getByRole("button", { name: "Create agent", exact: true }).click();
  const editor = page.locator("form").filter({ has: page.getByRole("button", { name: "Save agent", exact: true }) });
  await editor.getByLabel("Name", { exact: true }).fill("Custom reviewer");
  await editor.getByLabel("Instructions", { exact: true }).fill("Review the requested changes carefully.");
  await editor.getByRole("button", { name: "Save agent", exact: true }).click();
  const manage = page.locator("details").filter({ has: page.locator("summary", { hasText: /^Manage agents$/ }) });
  await expect(manage.locator("p", { hasText: "Custom reviewer" })).toHaveCount(1);
  await page.getByText("Import an existing Managoat agent", { exact: true }).click();
  await page.getByLabel("Old agent ID", { exact: true }).fill("legacy");
  await page.getByLabel("Old API credential", { exact: true }).fill("legacy-test");
  await page.getByRole("button", { name: "Import definition", exact: true }).click();
  await expect(manage.locator("p", { hasText: "Imported reviewer" })).toHaveCount(1);
  await expect(page.getByLabel("Old API credential", { exact: true })).toHaveValue("");
  await page.getByText("Secrets", { exact: true }).click();
  const secrets = page.locator("details").filter({ has: page.locator("summary", { hasText: /^Secrets$/ }) });
  await secrets.getByLabel("Name", { exact: true }).fill("GITHUB_TOKEN");
  await secrets.getByLabel("Value", { exact: true }).fill("test-repository-secret");
  await secrets.getByRole("button", { name: "Save secret" }).click();
  await expect(secrets.getByRole("button", { name: "Remove" })).toBeVisible();
  // Nothing in the panel starts a run any more.
  await expect(page.getByRole("button", { name: "Run in cloud" })).toHaveCount(0);
  await page.getByRole("button", { name: "Close", exact: true }).click();

  // New agent → Cloud makes a Cloud workspace from the repository and opens
  // the agent as its tab.
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const start = page.getByRole("dialog", { name: "Start an agent" });
  await expect(start.getByLabel("Run on")).toHaveValue("cloud");
  await expect(start.getByLabel("Cloud workspace")).toHaveValue("new");
  await expect(start.getByRole("combobox", { name: /^Agent/ }).locator("option", { hasText: "Custom reviewer" })).toHaveCount(1);
  await expect(start.getByLabel("Runtime and model")).toContainText("claude · anthropic/test");
  await start.getByLabel("Repository URL").fill("https://github.com/example/repo");
  await start.getByLabel("Repository credential").selectOption("GITHUB_TOKEN");
  await start.getByLabel("Prompt", { exact: true }).fill("Run the tests");
  await start.getByRole("button", { name: "Start", exact: true }).click();
  await expect(start).toHaveCount(0);
  await expect(page.locator("[data-cloud-tab-view]")).toBeVisible({ timeout: 15000 });
  await expect(page.locator("[data-cloud-status]")).toBeVisible();
  await expect(page.locator(".ws-crumb-name")).toHaveText("repo");
  await expect(page.getByRole("button", { name: "Allow once" })).toBeVisible({ timeout: 30000 });
  expect(fake().secret).toBe("test-repository-secret");
  await page.reload();
  await page.getByRole("button", { name: "Allow once" }).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Tests pass.", { timeout: 30000 });

  // A second agent can join the same Cloud workspace from New agent.
  await page.goto(`${base}/`);
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  await expect(start.getByLabel("Cloud workspace")).toContainText("repo");
  await expect(start.getByLabel("Repository URL")).toHaveCount(0);
  await start.getByLabel("Prompt", { exact: true }).fill("Write the docs");
  await start.getByRole("button", { name: "Start", exact: true }).click();
  await expect(page.locator("[data-cloud-tab]")).toHaveCount(2, { timeout: 15000 });
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Docs written.", { timeout: 30000 });

  // A past run opens read only, with its download.
  let events = [{ id: 1, blocks: [{ kind: "text", body: "Changed the example." }] }] as unknown[];
  await page.route(/\/api\/cloud\/runs(\/|$|\?)/, (route) => {
    const path = new URL(route.request().url()).pathname;
    if (route.request().method() !== "GET") return route.fulfill({ status: 405, json: { error: "gone" } });
    if (path === "/api/cloud/runs") return route.fulfill({ json: { runs: [pastRun] } });
    if (path.endsWith("/events")) return route.fulfill({ json: { events } });
    if (path.endsWith("/requests")) return route.fulfill({ json: { requests: [] } });
    return route.fulfill({ json: pastRun });
  });
  await page.goto(`${base}/`);
  await page.getByRole("button", { name: "Cloud agents", exact: true }).click();
  await page.locator("[data-past-runs]").getByRole("button", { name: "Coding agent: Fix the example" }).click();
  await expect(page.locator("[data-cloud-pane] .agent-status", { hasText: "completed" })).toBeVisible();
  await expect(page.locator("[data-cloud-pane]")).toContainText("Changed the example.");
  await expect(page.getByRole("button", { name: "Cancel run" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Run again" })).toHaveCount(0);
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download results and transcript" }).click();
  expect((await download).suggestedFilename()).toBe("run_0123abcd.json");
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole("button", { name: "Close past run" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  // Provider lifecycle failures may have no transcript blocks.
  events = [{ id: 1, stage: "turn", state: "failed", data: JSON.stringify({ reason: "The selected model is unavailable." }), blocks: [] }];
  await page.reload();
  await expect(page.getByText("The selected model is unavailable.", { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 1000, height: 800 });

  // Losing beta access hides nothing saved.
  await page.route("**/api/cloud/agents", (route) => route.fulfill({ status: 403, json: { error: "Cloud beta access is disabled" } }));
  await page.route("**/api/cloud/secrets", (route) => route.fulfill({ status: 503, json: { error: "Secrets are unavailable" } }));
  await page.goto(`${base}/`);
  await page.getByRole("button", { name: "Cloud agents", exact: true }).click();
  await expect(page.getByText("Cloud agents are unavailable:", { exact: false })).toBeVisible();
  await expect(page.locator("[data-past-runs]")).toContainText("Fix the example");
});

test("a Cloud workspace sets up, runs two tabs, asks in the drawer and is deleted", async ({ page }) => {
  test.setTimeout(90000);
  // Wide enough for the session sidebar beside the panes.
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 8, login: "workspace-tester" };
  const before = Object.keys(fake().conversations).length;
  await page.goto(base); await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  // An account with no machine reaches its Cloud workspaces too.
  await page.locator("[data-open-cloud-workspaces]").click();
  await expect(page.locator("[data-cloud-workspaces-group]")).toBeVisible();
  await page.locator("[data-new-cloud-workspace]").click();
  const create = page.getByRole("dialog", { name: "New Cloud workspace" });
  await create.getByLabel("Name", { exact: true }).fill("app work");
  await create.getByLabel("Repository URL").fill("https://github.com/example/app");
  await create.getByLabel("apt packages (optional)").fill("ffmpeg");
  await create.getByRole("button", { name: "Create Cloud workspace" }).click();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Setting up", { timeout: 15000 });
  await expect(page.locator("[data-cloud-home-status]")).toContainText("Setting up");
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
  await expect(page.locator('[data-cloud-workspace][data-cloud-state="Ready"]')).toContainText("app work");
  await expect(page.locator(".cloud-ws-repos")).toContainText("/home/sprite/app");

  // Two tabs, each from + Agent, each in the first repository's mount.
  const open = async (prompt: string) => {
    await page.locator("[data-new-cloud-tab]").click();
    const dialog = page.getByRole("dialog", { name: "Open an agent" });
    await expect(dialog.getByLabel("Runtime and model")).toContainText("claude · anthropic/test");
    await expect(dialog.getByLabel("Starts in")).toHaveValue("/home/sprite/app");
    await dialog.getByLabel("First prompt (optional)").fill(prompt);
    await dialog.getByRole("button", { name: "Open agent" }).click();
    await expect(dialog).toHaveCount(0);
  };
  await open("Run the tests");
  await expect(page.locator("[data-cloud-tab-view] .agent-user", { hasText: "Run the tests" })).toBeVisible({ timeout: 30000 });
  await expect(page.getByRole("button", { name: "Allow once" })).toBeVisible({ timeout: 30000 });
  await open("Write the docs");
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Docs written.", { timeout: 30000 });
  await expect(page.locator("[data-cloud-tab]")).toHaveCount(2);
  await expect(page.locator("[data-cloud-tab] .host-tag").first()).toHaveText("claude");
  await expect(page.locator('[data-cloud-tab][data-attention="needs_input"]')).toHaveCount(1, { timeout: 30000 });

  // The approval waits in the drawer, which goes to its tab.
  await expect(page.locator("[data-open-attention]")).toHaveAttribute("data-bell-count", "1");
  await page.locator("[data-open-attention]").click();
  const drawer = page.locator("[data-attention-drawer]");
  await expect(drawer.locator(".ws-att-item")).toContainText("app work");
  await drawer.locator("[data-attention-goto]").click();
  await expect(page.locator('[data-cloud-tab][aria-selected="true"][data-attention="needs_input"]')).toBeVisible();
  await page.getByRole("button", { name: "Allow once" }).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Tests pass.", { timeout: 30000 });
  await expect(page.locator('[data-cloud-tab][data-attention="needs_input"]')).toHaveCount(0, { timeout: 30000 });
  await expect(page.locator("[data-open-attention]")).toHaveAttribute("data-bell-count", "0");

  // Delete names the tabs it ends, then everything goes.
  await page.locator("[data-delete-cloud-workspace]").click();
  const confirm = page.getByRole("alertdialog", { name: "Delete app work?" });
  await expect(confirm.locator("[data-delete-ends] li")).toHaveCount(2);
  await confirm.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page.locator("[data-cloud-ws-list]")).toBeVisible();
  await expect(page.locator("[data-cloud-workspace]")).toHaveCount(0, { timeout: 45000 });
  expect(Object.values(fake().conversations).slice(before).every((c) => c.terminated)).toBe(true);
});

test("adding a package says Rebuild needed and leaves the open tab alone", async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 9, login: "settings-tester" };
  await page.goto(base); await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  await page.locator("[data-open-cloud-workspaces]").click();
  await page.locator("[data-new-cloud-workspace]").click();
  const create = page.getByRole("dialog", { name: "New Cloud workspace" });
  await create.getByLabel("Name", { exact: true }).fill("settings work");
  await create.getByLabel("Repository URL").fill("https://github.com/example/app");
  await create.getByLabel("apt packages (optional)").fill("ffmpeg");
  await create.getByRole("button", { name: "Create Cloud workspace" }).click();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
  await expect(page.locator("[data-cloud-settings]")).toContainText("built from these settings");

  // A tab that has done its work.
  await page.locator("[data-new-cloud-tab]").click();
  const dialog = page.getByRole("dialog", { name: "Open an agent" });
  await dialog.getByLabel("First prompt (optional)").fill("Write the docs");
  await dialog.getByRole("button", { name: "Open agent" }).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Docs written.", { timeout: 30000 });
  const tab = await page.locator("[data-cloud-tab]").getAttribute("data-cloud-tab");

  // Settings: one more package.
  await page.locator(".ws-crumb", { hasText: "settings work" }).click();
  await page.locator("[data-edit-cloud-settings]").click();
  const form = page.getByRole("form", { name: "Cloud workspace settings" });
  await expect(form.getByLabel("apt packages", { exact: true })).toHaveValue("ffmpeg");
  await form.getByLabel("apt packages", { exact: true }).fill("ffmpeg jq");
  await form.getByRole("button", { name: "Save settings" }).click();
  await expect(form).toHaveCount(0);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Rebuild needed · 1 change", { timeout: 15000 });
  await expect(page.locator('[data-cloud-workspace][data-cloud-state="Rebuild needed · 1 change"]')).toContainText("settings work");
  const change = page.locator('[data-cloud-pending] [data-pending-field="packages"]');
  await expect(change.locator("td").first()).toHaveText("apt ffmpeg");
  await expect(change.locator("td").last()).toHaveText("apt ffmpeg, apt jq");
  expect(Object.values(fake().environments).at(-1)!.packages).toEqual({ apt: ["ffmpeg", "jq"] });

  // The tab is as it was: still open, its transcript there, its
  // conversation not ended or prompted again.
  await expect(page.locator("[data-cloud-tab]")).toHaveCount(1);
  await page.locator(`[data-cloud-tab="${tab}"]`).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Docs written.");
  const conversation = Object.values(fake().conversations).find((c) => c.turns[0]?.prompt === "Write the docs" && !c.terminated);
  expect(conversation).toBeTruthy();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Rebuild needed · 1 change");
});

test("a change an agent made in a Cloud workspace shows in Changes", async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 10, login: "changes-tester" };
  await page.goto(base); await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  await page.locator("[data-open-cloud-workspaces]").click();
  await page.locator("[data-new-cloud-workspace]").click();
  const create = page.getByRole("dialog", { name: "New Cloud workspace" });
  await create.getByLabel("Name", { exact: true }).fill("docs box");
  await create.getByLabel("Repository URL").fill("https://github.com/example/app");
  await create.getByRole("button", { name: "Create Cloud workspace" }).click();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
  // Nothing changed yet.
  const repo = page.locator('[data-cloud-changes-repo="/home/sprite/app"]');
  await expect(repo).toContainText("no changes", { timeout: 15000 });

  await page.locator("[data-new-cloud-tab]").click();
  const dialog = page.getByRole("dialog", { name: "Open an agent" });
  await dialog.getByLabel("First prompt (optional)").fill("Write the guide");
  await dialog.getByRole("button", { name: "Open agent" }).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Docs written.", { timeout: 30000 });
  await page.locator("[data-cloud-tab-changes]").click();
  const changes = page.locator("[data-cloud-tab-view] [data-cloud-changes]");
  await expect(changes.locator('[data-cloud-diff-file="README.md"]')).toContainText("Docs for the app.", { timeout: 15000 });
  await expect(changes.locator(".cloud-changes-status li")).toHaveCount(2);
  await expect(changes).toContainText("main");
  // An untracked file the diff can't show opens from the status.
  await changes.getByRole("button", { name: "docs/guide.md" }).click();
  await expect(changes.locator('[data-cloud-file="/home/sprite/app/docs/guide.md"]')).toContainText("Start.");
  // Each read named the repository, and the diff was against HEAD.
  expect(fake().machineReads.some((r) => /^\/api\/workspaces\/ws\d+\/diff\?path=%2Fhome%2Fsprite%2Fapp&ref=HEAD$/.test(r))).toBe(true);
  expect(fake().machineReads.some((r) => /^\/api\/workspaces\/ws\d+\/git-status\?path=%2Fhome%2Fsprite%2Fapp&untracked=all$/.test(r))).toBe(true);

  await page.locator("[data-delete-cloud-workspace]").click();
  await page.getByRole("alertdialog", { name: "Delete docs box?" }).getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page.locator("[data-cloud-workspace]")).toHaveCount(0, { timeout: 45000 });
});

test("Rebuild asks first, warns that the disk is wiped, and clears the badge", async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 11, login: "rebuild-tester" };
  await page.goto(base); await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  await page.locator("[data-open-cloud-workspaces]").click();
  await page.locator("[data-new-cloud-workspace]").click();
  const create = page.getByRole("dialog", { name: "New Cloud workspace" });
  await create.getByLabel("Name", { exact: true }).fill("rebuild work");
  await create.getByLabel("Repository URL").fill("https://github.com/example/app");
  await create.getByLabel("apt packages (optional)").fill("ffmpeg");
  await create.getByRole("button", { name: "Create Cloud workspace" }).click();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });

  // A tab with some work, then one more package.
  await page.locator("[data-new-cloud-tab]").click();
  const dialog = page.getByRole("dialog", { name: "Open an agent" });
  await dialog.getByLabel("First prompt (optional)").fill("Write the docs");
  await dialog.getByRole("button", { name: "Open agent" }).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Docs written.", { timeout: 30000 });
  const tab = await page.locator("[data-cloud-tab]").getAttribute("data-cloud-tab");
  await page.locator(".ws-crumb", { hasText: "rebuild work" }).click();
  await page.locator("[data-edit-cloud-settings]").click();
  const form = page.getByRole("form", { name: "Cloud workspace settings" });
  await form.getByLabel("apt packages", { exact: true }).fill("ffmpeg jq");
  await form.getByRole("button", { name: "Save settings" }).click();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Rebuild needed · 1 change", { timeout: 15000 });

  // The header's Rebuild asks, and says the disk is wiped; Cancel sends nothing.
  await page.locator('[data-rebuild-cloud-workspace="header"]').click();
  const confirm = page.getByRole("alertdialog", { name: "Rebuild rebuild work?" });
  await expect(confirm.locator("[data-rebuild-warning]")).toContainText("The disk is wiped.");
  await expect(confirm.locator("[data-rebuild-warning]")).toContainText("uncommitted and untracked work");
  await expect(confirm.locator("[data-rebuild-mid-turn]")).toHaveCount(0);
  await confirm.locator("[data-cancel-rebuild]").click();
  await expect(confirm).toHaveCount(0);
  expect(fake().rebuilds).toHaveLength(0);

  // Settings' Rebuild, confirmed: setting up, then ready with no badge.
  await page.locator('[data-rebuild-cloud-workspace="settings"]').click();
  await confirm.locator("[data-confirm-rebuild]").click();
  await expect(confirm).toHaveCount(0);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Setting up", { timeout: 15000 });
  expect(fake().rebuilds).toEqual([{}]);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
  await expect(page.locator("[data-cloud-rebuild-badge]")).toHaveCount(0);
  await expect(page.locator("[data-cloud-settings]")).toContainText("built from these settings");
  await expect(page.locator('[data-cloud-workspace][data-cloud-state="Ready"]')).toContainText("rebuild work");

  // The tab is still there, with its transcript.
  await expect(page.locator("[data-cloud-tab]")).toHaveCount(1);
  await page.locator(`[data-cloud-tab="${tab}"]`).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Docs written.");
});

test("Retry on a failed Cloud workspace sets it up again without a warning", async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 12, login: "retry-tester" };
  failSetup = true;
  await page.goto(base); await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  await page.locator("[data-open-cloud-workspaces]").click();
  await page.locator("[data-new-cloud-workspace]").click();
  const create = page.getByRole("dialog", { name: "New Cloud workspace" });
  await create.getByLabel("Name", { exact: true }).fill("retry work");
  await create.getByLabel("Repository URL").fill("https://github.com/example/app");
  await create.getByRole("button", { name: "Create Cloud workspace" }).click();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Failed", { timeout: 30000 });
  await expect(page.locator("[data-cloud-failed]")).toContainText("pnpm: command not found");
  // A failed workspace has no machine: Rebuild isn't offered, Retry is.
  await expect(page.locator("[data-rebuild-cloud-workspace]")).toHaveCount(0);

  failSetup = false;
  const rebuildsBefore = fake().rebuilds.length;
  await page.locator("[data-retry-cloud-workspace]").click();
  await expect(page.getByRole("alertdialog")).toHaveCount(0);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
  await expect(page.locator("[data-cloud-failed]")).toHaveCount(0);
  expect(fake().retries).toHaveLength(1);
  expect(fake().rebuilds).toHaveLength(rebuildsBefore);
});
