import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
// Cloud agents and Cloud workspaces (S40, M116–M120), on the platform page
// (M153; the shell draws none of their views since M155). Each test is a
// fresh control account with no machine: control's page links out to the
// platform page (`--platform-host app.localhost`), which signs in with one
// silent hop through the hub (M152).
import { expect, test, type Page } from "@playwright/test";
import { fakeManagoat } from "../fixtures/fakes.ts";
import { controlPort, listen } from "./ports";

let base: string, app: string, directory: string, control: ChildProcess, github: Server;
// The GitHub account signing in: each test is a fresh control account.
let user = { id: 7, login: "cloud-tester" };
// Managoat (S40): the next setups fail while `failSetup` is set, and a
// rebuild finds no machine free while `noMachine` is (M120).
let failSetup = false, noMachine = false;
const managoat = fakeManagoat(0, "127.0.0.1", { fails: () => (failSetup ? "setup_script exited 1: pnpm: command not found" : null), full: () => noMachine });
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
  control = spawn("../target/debug/arugula-control", ["--listen", "127.0.0.1:0", "--public-url", "http://127.0.0.1:0", "--db", db, "--static-dir", "dist", "--github-client-id", "test", "--github-client-secret", "test", "--github-url", gh, "--github-api", gh, "--cloud-provider-url", upstream, "--cloud-provider-key", "test", "--cloud-secret-key", "01".repeat(32), "--cloud-model", "anthropic/test", "--cloud-accounts", "*", "--platform-host", "app.localhost"], { stdio: "ignore" });
  base = `http://127.0.0.1:${await controlPort(db, control)}`;
  app = base.replace("//127.0.0.1", "//app.localhost");
});
test.afterAll(() => { control?.kill(); github?.close(); managoat.server.close(); if (directory) rmSync(directory, { recursive: true, force: true }); });

/** Sign in on control's page as `user`, with stored codes. */
async function signIn(page: Page) {
  await page.goto(base); await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
}

/** What a button on control's page opens: the platform page, in a tab of
 * its own. */
async function opens(page: Page, button: string): Promise<Page> {
  const [opened] = await Promise.all([page.context().waitForEvent("page"), page.locator(button).click()]);
  await opened.waitForLoadState();
  return opened;
}

/** New Cloud workspace, on the platform page's list of them: `name`, from
 * the example repository, with `more`. */
async function newWorkspace(page: Page, name: string, more: { apt?: string; secret?: string } = {}) {
  await page.locator("[data-create-cloud-workspace]").click();
  const create = page.getByRole("dialog", { name: "New Cloud workspace" });
  await create.getByLabel("Name", { exact: true }).fill(name);
  await create.getByLabel("Repository URL").fill("https://github.com/example/app");
  if (more.secret) await create.getByLabel("Repository credential").selectOption(more.secret);
  if (more.apt) await create.getByLabel("apt packages (optional)").fill(more.apt);
  await create.getByRole("button", { name: "Create Cloud workspace" }).click();
}

// A run from before runs were retired (S40, M119): control's answers for
// it, as a past run reads them.
const pastRun = {
  id: "run_0123abcd", state: "completed", created: 1, result: { diff: { diff: "+fixed" }, files: {} },
  spec: { agent_id: "curated_coding", prompt: "Fix the example", repository: "https://github.com/example/repo", git_ref: "", agent: { name: "Coding agent", instructions: "", mcp_servers: {}, skills: [], secret_names: [] } },
};

test("cloud-only account saves a secret, starts an agent in a new Cloud workspace, approves, and reads a past run", async ({ page }) => {
  test.setTimeout(90000);
  await signIn(page);
  // Runs once opened as panes were remembered; they're forgotten on load.
  await page.evaluate(() => localStorage.setItem("arugula.cloud-panes.someone", JSON.stringify(["run_0123abcd"])));
  await page.reload();
  await expect.poll(() => page.evaluate(() => localStorage.getItem("arugula.cloud-panes.someone"))).toBeNull();
  // Cloud agents is the platform page's.
  const p = await opens(page, "button:text-is('Cloud agents')");
  await expect(p).toHaveURL(`${app}/agents`, { timeout: 15000 });
  await p.getByText("Manage agents", { exact: true }).click();
  await p.getByRole("button", { name: "Create agent", exact: true }).click();
  const editor = p.locator("form").filter({ has: p.getByRole("button", { name: "Save agent", exact: true }) });
  await editor.getByLabel("Name", { exact: true }).fill("Custom reviewer");
  await editor.getByLabel("Instructions", { exact: true }).fill("Review the requested changes carefully.");
  await editor.getByRole("button", { name: "Save agent", exact: true }).click();
  const manage = p.locator("details").filter({ has: p.locator("summary", { hasText: /^Manage agents$/ }) });
  await expect(manage.locator("p", { hasText: "Custom reviewer" })).toHaveCount(1);
  await p.getByText("Import an existing Managoat agent", { exact: true }).click();
  await p.getByLabel("Old agent ID", { exact: true }).fill("legacy");
  await p.getByLabel("Old API credential", { exact: true }).fill("legacy-test");
  await p.getByRole("button", { name: "Import definition", exact: true }).click();
  await expect(manage.locator("p", { hasText: "Imported reviewer" })).toHaveCount(1);
  await expect(p.getByLabel("Old API credential", { exact: true })).toHaveValue("");
  await p.getByText("Secrets", { exact: true }).click();
  const secrets = p.locator("details").filter({ has: p.locator("summary", { hasText: /^Secrets$/ }) });
  await secrets.getByLabel("Name", { exact: true }).fill("GITHUB_TOKEN");
  await secrets.getByLabel("Value", { exact: true }).fill("test-repository-secret");
  await secrets.getByRole("button", { name: "Save secret" }).click();
  await expect(secrets.getByRole("button", { name: "Remove" })).toBeVisible();
  // Nothing in the panel starts a run any more.
  await expect(p.getByRole("button", { name: "Run in cloud" })).toHaveCount(0);

  // A Cloud workspace from the repository, with its credential, and the
  // agent opened as its tab.
  await p.locator("[data-open-cloud-workspaces]").click();
  await expect(p.locator("[data-cloud-ws-list]")).toBeVisible({ timeout: 15000 });
  await newWorkspace(p, "repo", { secret: "GITHUB_TOKEN" });
  await expect(p.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
  await expect(p.locator(".ws-crumb-name")).toHaveText("repo");
  await p.locator("[data-new-cloud-tab]").click();
  const start = p.getByRole("dialog", { name: "Open an agent" });
  await expect(start.getByRole("combobox", { name: /^Agent/ }).locator("option", { hasText: "Custom reviewer" })).toHaveCount(1);
  await expect(start.getByLabel("Runtime and model")).toContainText("claude · anthropic/test");
  await start.getByLabel("First prompt (optional)").fill("Run the tests");
  await start.getByRole("button", { name: "Open agent" }).click();
  await expect(start).toHaveCount(0);
  await expect(p.locator("[data-cloud-tab-view]")).toBeVisible({ timeout: 15000 });
  await expect(p.getByRole("button", { name: "Allow once" })).toBeVisible({ timeout: 30000 });
  expect(fake().secret).toBe("test-repository-secret");
  await p.reload();
  await p.getByRole("button", { name: "Allow once" }).click();
  await expect(p.locator("[data-cloud-tab-view]")).toContainText("Tests pass.", { timeout: 30000 });

  // A second agent joins the same Cloud workspace.
  await p.locator("[data-new-cloud-tab]").click();
  await start.getByLabel("First prompt (optional)").fill("Write the docs");
  await start.getByRole("button", { name: "Open agent" }).click();
  await expect(p.locator("[data-cloud-tab]")).toHaveCount(2, { timeout: 15000 });
  await expect(p.locator("[data-cloud-tab-view]")).toContainText("Docs written.", { timeout: 30000 });

  // A past run opens read only, with its download.
  // Managoat's transcript opens the turn with its prompt: shown once.
  let events = [{ id: 1, stage: "turn", state: "started", blocks: [{ kind: "prompt", body: "Fix the example" }] }, { id: 2, blocks: [{ kind: "text", body: "Changed the example." }] }] as unknown[];
  await p.route(/\/api\/cloud\/runs(\/|$|\?)/, (route) => {
    const path = new URL(route.request().url()).pathname;
    if (route.request().method() !== "GET") return route.fulfill({ status: 405, json: { error: "gone" } });
    if (path === "/api/cloud/runs") return route.fulfill({ json: { runs: [pastRun] } });
    if (path.endsWith("/events")) return route.fulfill({ json: { events } });
    if (path.endsWith("/requests")) return route.fulfill({ json: { requests: [] } });
    return route.fulfill({ json: pastRun });
  });
  await p.goto(`${app}/agents`);
  await p.locator("[data-past-runs]").getByRole("button", { name: "Coding agent: Fix the example" }).click();
  await expect(p).toHaveURL(/\/past\/run_0123abcd$/);
  await expect(p.locator("[data-cloud-pane] .agent-status", { hasText: "completed" })).toBeVisible();
  await expect(p.locator("[data-cloud-pane]")).toContainText("Changed the example.");
  await expect(p.locator("[data-cloud-pane] .agent-user")).toHaveText(["Fix the example"]);
  await expect(p.getByRole("button", { name: "Cancel run" })).toHaveCount(0);
  await expect(p.getByRole("button", { name: "Run again" })).toHaveCount(0);
  const download = p.waitForEvent("download");
  await p.getByRole("button", { name: "Download results and transcript" }).click();
  expect((await download).suggestedFilename()).toBe("run_0123abcd.json");
  await p.setViewportSize({ width: 390, height: 844 });
  await expect(p.locator("[data-cloud-pane]")).toBeVisible();
  expect(await p.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  // Provider lifecycle failures may have no transcript blocks.
  events = [{ id: 1, stage: "turn", state: "failed", data: JSON.stringify({ reason: "The selected model is unavailable." }), blocks: [] }];
  await p.reload();
  await expect(p.getByText("The selected model is unavailable.", { exact: true })).toBeVisible();
  await p.setViewportSize({ width: 1000, height: 800 });

  // Losing beta access hides nothing saved.
  await p.route("**/api/cloud/agents", (route) => route.fulfill({ status: 403, json: { error: "Cloud beta access is disabled" } }));
  await p.route("**/api/cloud/secrets", (route) => route.fulfill({ status: 503, json: { error: "Secrets are unavailable" } }));
  await p.goto(`${app}/agents`);
  await expect(p.getByText("Cloud agents are unavailable:", { exact: false })).toBeVisible();
  await expect(p.locator("[data-past-runs]")).toContainText("Fix the example");
});

test("a Cloud workspace sets up, runs two tabs, asks for an approval and is deleted", async ({ page }) => {
  test.setTimeout(90000);
  user = { id: 8, login: "workspace-tester" };
  const before = Object.keys(fake().conversations).length;
  await signIn(page);
  // An account with no machine reaches its Cloud workspaces too: control's
  // page opens them on the platform page.
  page = await opens(page, "[data-open-cloud-workspaces]");
  await expect(page).toHaveURL(new RegExp(`^${app}/p/prj_[0-9a-f]+[^/]*/workspaces$`), { timeout: 15000 });
  await expect(page.locator("[data-cloud-ws-list]")).toBeVisible();
  await page.setViewportSize({ width: 1400, height: 900 });
  await newWorkspace(page, "app work", { apt: "ffmpeg" });
  // Managoat (v0.39.0) says which step it's on.
  await expect(page.locator("[data-cloud-status]")).toHaveText(/^Setting up · (machine|packages|setup script)$/, { timeout: 15000 });
  await expect(page.locator("[data-cloud-home-status]")).toContainText("Setting up · ");
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
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

  // The approval waits on its tab, which goes to it.
  await page.locator('[data-cloud-tab][data-attention="needs_input"]').click();
  await expect(page.locator('[data-cloud-tab][aria-selected="true"][data-attention="needs_input"]')).toBeVisible();
  await page.getByRole("button", { name: "Allow once" }).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Tests pass.", { timeout: 30000 });
  await expect(page.locator('[data-cloud-tab][data-attention="needs_input"]')).toHaveCount(0, { timeout: 30000 });
  // Answered, it says so, with no buttons left to press.
  await expect(page.locator("[data-cloud-tab-view] .agent-perm .agent-perm-done")).toHaveText("Answered: Allow once");
  await expect(page.locator("[data-cloud-tab-view] .agent-perm button")).toHaveCount(0);
  await page.reload();
  await expect(page.locator("[data-cloud-tab-view] .agent-perm .agent-perm-done")).toHaveText("Answered: Allow once", { timeout: 15000 });
  await expect(page.locator("[data-cloud-tab-view] .agent-perm button")).toHaveCount(0);

  // A streamed reply reads as one message: its chunks joined, its code
  // fence whole, and a tool call's empty input not shown.
  await open("Stream it");
  const streamed = page.locator("[data-cloud-tab-view] .cloud-transcript");
  await expect(streamed).toContainText("hello from claude", { timeout: 30000 });
  await expect(streamed.locator(".agent-msg")).toHaveCount(2);
  await expect(streamed.locator(".agent-msg").first()).toHaveText("I'll run pwd first.");
  await expect(streamed.locator(".agent-msg").nth(1).locator("pre")).toHaveCount(1);
  await expect(streamed.locator(".agent-msg").nth(1).locator("pre")).toContainText("echo hi");
  await expect(streamed.locator(".cloud-tool").first()).toHaveText("Terminal · pwd");
  await expect(streamed.getByText("{}", { exact: true })).toHaveCount(0);
  // The tab scrolls inside its pane: the page under it never does.
  expect(await page.evaluate(() => document.body.scrollTop + document.documentElement.scrollTop)).toBe(0);
  await page.locator(".cloud-tab-prompt textarea").focus();
  expect(await page.evaluate(() => document.body.scrollTop + document.documentElement.scrollTop)).toBe(0);

  // Delete names the tabs it ends, then everything goes.
  await page.locator("[data-delete-cloud-workspace]").click();
  const confirm = page.getByRole("alertdialog", { name: "Delete app work?" });
  await expect(confirm.locator("[data-delete-ends] li")).toHaveCount(3);
  await confirm.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page.locator("[data-cloud-ws-list]")).toBeVisible();
  await expect(page.locator(".cloud-ws-list li")).toHaveCount(0, { timeout: 45000 });
  expect(Object.values(fake().conversations).slice(before).every((c) => c.terminated)).toBe(true);

  // Its tabs stay to read, as past tabs.
  await page.reload();
  await expect(page.locator("[data-past-tabs] li")).toHaveCount(3, { timeout: 15000 });
  await expect(page.locator("[data-past-tabs]")).toContainText("app work (deleted)");
  // The one that ran the tests (they're all the same agent).
  const ran = await page.evaluate(async () => {
    const { past_tabs } = await (await fetch("/api/cloud/past-tabs")).json() as { past_tabs: { id: string }[] };
    for (const t of past_tabs) if (JSON.stringify(await (await fetch(`/api/cloud/past-tabs/${t.id}/events`)).json()).includes("Run the tests")) return t.id;
    return "";
  });
  await page.locator(`[data-past-tab="${ran}"]`).click();
  const past = page.locator("[data-past-tab-view]");
  await expect(past).toContainText("Tests pass.", { timeout: 15000 });
  await expect(past.locator(".agent-user")).toHaveText(["Run the tests"]);
  await expect(past.locator("textarea")).toHaveCount(0);
  await expect(past.locator(".agent-perm button")).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/past/${ran}$`));
});

test("adding a package says Rebuild needed and leaves the open tab alone", async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 9, login: "settings-tester" };
  await signIn(page);
  await page.goto(`${app}/workspaces`);
  await expect(page.locator("[data-cloud-ws-list]")).toBeVisible({ timeout: 15000 });
  await page.locator("[data-create-cloud-workspace]").click();
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
  await page.locator(".ws-crumb", { hasText: "Cloud workspaces" }).click();
  await expect(page.locator(".cloud-ws-list li", { hasText: "settings work" })).toContainText("Rebuild needed · 1 change");
  await page.locator(".cloud-ws-list-name", { hasText: "settings work" }).click();
  // Back on its tab: its home has the settings.
  await page.locator(".ws-crumb", { hasText: "settings work" }).click();
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
  await signIn(page);
  await page.goto(`${app}/workspaces`);
  await expect(page.locator("[data-cloud-ws-list]")).toBeVisible({ timeout: 15000 });
  await page.locator("[data-create-cloud-workspace]").click();
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
  await expect(page.locator(".cloud-ws-list li")).toHaveCount(0, { timeout: 45000 });
});

test("Rebuild asks first, warns that the disk is wiped, and clears the badge", async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 11, login: "rebuild-tester" };
  await signIn(page);
  await page.goto(`${app}/workspaces`);
  await expect(page.locator("[data-cloud-ws-list]")).toBeVisible({ timeout: 15000 });
  await page.locator("[data-create-cloud-workspace]").click();
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

  // No machine free: it waits, says so, and Cancel stops control asking.
  noMachine = true;
  await page.locator('[data-rebuild-cloud-workspace="header"]').click();
  await confirm.locator("[data-confirm-rebuild]").click();
  await expect(confirm).toHaveCount(0);
  const waiting = page.locator('[data-rebuild-waiting="header"]');
  await expect(waiting).toContainText("Rebuild waiting for a machine");
  await expect(page.locator('[data-rebuild-waiting="settings"]')).toBeVisible();
  await expect(page.locator("[data-rebuild-queued]")).toBeVisible();
  await expect.poll(() => fake().rebuilds.length, { timeout: 15000 }).toBeGreaterThan(1);
  await waiting.locator("[data-cancel-queued-rebuild]").click();
  await expect(page.locator("[data-rebuild-waiting]")).toHaveCount(0);
  await expect(page.locator('[data-rebuild-cloud-workspace="header"]')).toBeVisible();
  const asked = fake().rebuilds.length;
  await page.waitForTimeout(3000);
  expect(fake().rebuilds).toHaveLength(asked);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Rebuild needed · 1 change");
  noMachine = false;
  fake().rebuilds.length = 0;

  // Settings' Rebuild, confirmed: setting up, then ready with no badge.
  await page.locator('[data-rebuild-cloud-workspace="settings"]').click();
  await confirm.locator("[data-confirm-rebuild]").click();
  await expect(confirm).toHaveCount(0);
  await expect(page.locator("[data-cloud-status]")).toHaveText(/^Setting up/, { timeout: 15000 });
  expect(fake().rebuilds).toEqual([{}]);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
  await expect(page.locator("[data-cloud-rebuild-badge]")).toHaveCount(0);
  await expect(page.locator("[data-cloud-settings]")).toContainText("built from these settings");
  await page.locator(".ws-crumb", { hasText: "Cloud workspaces" }).click();
  await expect(page.locator(".cloud-ws-list li", { hasText: "rebuild work" })).toContainText("Ready");
  await page.locator(".cloud-ws-list-name", { hasText: "rebuild work" }).click();

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
  await signIn(page);
  await page.goto(`${app}/workspaces`);
  await expect(page.locator("[data-cloud-ws-list]")).toBeVisible({ timeout: 15000 });
  await page.locator("[data-create-cloud-workspace]").click();
  const create = page.getByRole("dialog", { name: "New Cloud workspace" });
  await create.getByLabel("Name", { exact: true }).fill("retry work");
  await create.getByLabel("Repository URL").fill("https://github.com/example/app");
  await create.getByRole("button", { name: "Create Cloud workspace" }).click();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Failed", { timeout: 30000 });
  await expect(page.locator("[data-cloud-failed]")).toContainText("pnpm: command not found");
  // What the setup printed, step by step, read on demand.
  await page.locator("[data-show-setup-log]").click();
  await expect(page.locator('[data-setup-step="setup_script"][data-setup-status="failed"]')).toContainText("pnpm: command not found");
  await expect(page.locator('[data-setup-step="packages"]')).toHaveAttribute("data-setup-status", "succeeded");
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
