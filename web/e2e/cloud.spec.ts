import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { controlPort, listen } from "./ports";

let base: string, directory: string, control: ChildProcess, github: Server, provider: Server;
let answered = false, terminated = false;
let secret = "";
// The GitHub account signing in: each test is a fresh control account.
let user = { id: 7, login: "cloud-tester" };
// Cloud workspaces (S40): the fake workspace's reads, its tabs'
// conversations (by id: status and whether its approval was answered).
let workspaceReads = 0, made = 0;
// The environment as last created or PATCHed, and as the workspace's setup
// read it: the workspace is behind in the fields that differ (M117).
const buildFields = ["repositories", "packages", "setup_script", "setup_timeout_seconds", "env_vars", "networking_type", "networking_config"];
let environment: Record<string, unknown> = {}, built: Record<string, unknown> = {}, revision = 1;
const changedFields = () => buildFields.filter((f) => JSON.stringify(environment[f]) !== JSON.stringify(built[f]));
const conversations = new Map<string, { status: string; prompt: string; answered: boolean }>();
// Rebuild and Retry (M120): what each was asked with, and whether the next
// setups fail, or a rebuild finds no machine free.
const rebuilds: unknown[] = [], retries: unknown[] = [];
let failSetup = false, noMachine = false;
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
  provider = createServer(async (req, res) => {
    const chunks: Buffer[] = []; for await (const chunk of req) chunks.push(Buffer.from(chunk));
    const body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString()) : {};
    const url = new URL(req.url!, "http://provider"); const path = url.pathname;
    let data: unknown = {};
    if (req.method === "DELETE") { res.writeHead(204).end(); return; }
    const tab = /^\/api\/conversations\/(tabconv\d+)(\/.*)?$/.exec(path);
    if (path === "/api/vaults") data = { id: "vault" };
    else if (path === "/api/workspaces" && req.method === "POST") {
      built = { ...environment }; workspaceReads = 0;
      data = { id: "ws", name: body.name, environment_id: body.environment_id, vault_id: body.vault_id, status: "provisioning", sandbox_status: "pending", error: null, revision: "rev1", built_from_revision: null, stale: false, changed_fields: [] };
    }
    // Setting up for the first reads, then ready (or failed).
    else if (path === "/api/workspaces/ws") {
      const ready = ++workspaceReads > 2;
      const changed = ready ? changedFields() : [];
      data = ready && failSetup
        ? { id: "ws", status: "failed", sandbox_status: null, error: "setup_script exited 1: pnpm: command not found", revision: `rev${revision}`, built_from_revision: null, stale: false, changed_fields: [] }
        : { id: "ws", status: ready ? "ready" : "provisioning", sandbox_status: ready ? "ready" : "starting", error: null, revision: `rev${revision}`, built_from_revision: ready ? `rev${built.revision}` : null, stale: changed.length > 0, changed_fields: changed };
    }
    // A new machine from the environment as it is now, set up over the next
    // reads. A rebuild is refused while a turn runs, unless it interrupts.
    else if (path === "/api/workspaces/ws/rebuild" || path === "/api/workspaces/ws/retry") {
      (path.endsWith("/retry") ? retries : rebuilds).push(body);
      const running = [...conversations].filter(([, c]) => c.status === "running" && c.prompt === "Run the tests" && !c.answered).map(([id]) => id);
      if (path.endsWith("/rebuild") && noMachine) {
        res.writeHead(503, { "content-type": "application/json" }).end(JSON.stringify({ error: "fleet_full", message: "no machine free" }));
        return;
      }
      if (path.endsWith("/rebuild") && running.length && !body.interrupt) {
        res.writeHead(409, { "content-type": "application/json" }).end(JSON.stringify({ error: "turns_running", message: "refused", conversation_ids: running }));
        return;
      }
      built = { ...environment }; workspaceReads = 0;
      res.writeHead(202, { "content-type": "application/json" }).end(JSON.stringify({ data: { id: "ws", status: "provisioning", sandbox_status: "pending", error: null, revision: `rev${revision}`, built_from_revision: null, stale: false, changed_fields: [] } }));
      return;
    } else if (path === "/api/agents" && body.metadata?.arugula_tab) data = { id: `tabagent${++made}` };
    else if (path === "/api/conversations" && body.workspace_id) {
      const id = `tabconv${conversations.size + 1}`;
      conversations.set(id, { status: "running", prompt: body.prompt, answered: false });
      data = { id, status: "running" };
    } else if (tab) {
      const c = conversations.get(tab[1])!; const rest = tab[2] ?? "";
      // The tests prompt asks for an approval; any other just answers.
      const asks = c.prompt === "Run the tests";
      if (rest === "/events") {
        const events = [{ id: 1, stage: "turn", state: "started", blocks: [{ kind: "prompt", body: c.prompt }] },
          ...(asks ? [{ id: 2, blocks: [{ kind: "permission_request", request_id: "approval", summary: "Run the tests?", options: [{ optionId: "allow", name: "Allow once" }] }] }] : []),
          ...(!asks || c.answered ? [{ id: 3, blocks: [{ kind: "text", body: asks ? "Tests pass." : "Docs written." }] }] : [])];
        data = events.filter((e) => e.id > Number(url.searchParams.get("after")));
      } else if (rest === "/requests/approval") { c.answered = true; c.status = "idle"; }
      else if (rest === "/terminate") c.status = "terminated";
      else if (rest === "") data = { id: tab[1], status: c.status === "running" && !asks ? "idle" : c.status, usage_total: {} };
      else { res.writeHead(404).end(); return; }
    }
    else if (path === "/api/agents/legacy") {
      if (req.headers.authorization !== "Bearer legacy-test") { res.writeHead(401).end(); return; }
      data = { name: "Imported reviewer", system: "Review the changes", skills: [], mcp_servers: {} };
    }
    else if (path === "/api/environments") {
      revision = 1;
      environment = { setup_timeout_seconds: 120, networking_type: "unrestricted", networking_config: {}, ...body, revision };
      data = { id: "env", revision: "rev1" };
    } else if (path === "/api/environments/env" && req.method === "PATCH") {
      const next = { ...environment, ...body };
      if (buildFields.some((f) => JSON.stringify(next[f]) !== JSON.stringify(environment[f]))) next.revision = ++revision;
      environment = next;
      data = { id: "env", revision: `rev${revision}` };
    }
    else if (path.endsWith("/secrets")) { secret = body.value; data = { key: body.key }; }
    else if (path === "/api/agents") data = { id: "agent" };
    else if (path === "/api/conversations") data = { id: "conversation" };
    else if (path.endsWith("/events")) {
      const events = [{ id: 1, blocks: [{ kind: "permission_request", request_id: "approval", summary: "Run the tests?", options: [{ optionId: "allow", name: "Allow once" }] }] }, ...(answered ? [{ id: 2, blocks: [{ kind: "text", body: "Tests pass. Change complete." }] }] : [])];
      data = events.filter((e) => e.id > Number(url.searchParams.get("after")));
    } else if (path.endsWith("/requests/approval")) { answered = true; }
    else if (path.endsWith("/terminate")) { terminated = true; }
    else if (path === "/api/conversations/conversation") data = { status: terminated ? "terminated" : answered ? "idle" : "running", sandbox_id: "sandbox", usage_total: {} };
    else if (path === "/api/sandboxes/sandbox") data = { status: "ready", transition: null };
    else if (path.endsWith("/diff")) data = { diff: "+fixed", truncated: false };
    else if (path.endsWith("/git-status")) data = { entries: [], truncated: false };
    else { res.writeHead(404).end(); return; }
    res.setHeader("content-type", "application/json"); res.end(JSON.stringify({ data }));
  });
  const gh = `http://127.0.0.1:${await listen(github)}`;
  const upstream = `http://127.0.0.1:${await listen(provider)}`;
  directory = mkdtempSync(join(tmpdir(), "arugula-cloud-e2e-")); const db = join(directory, "control.db");
  control = spawn("../target/debug/arugula-control", ["--listen", "127.0.0.1:0", "--public-url", "http://127.0.0.1:0", "--db", db, "--static-dir", "dist", "--github-client-id", "test", "--github-client-secret", "test", "--github-url", gh, "--github-api", gh, "--cloud-provider-url", upstream, "--cloud-provider-key", "test", "--cloud-secret-key", "01".repeat(32), "--cloud-model", "anthropic/test", "--cloud-accounts", "*"], { stdio: "ignore" });
  base = `http://127.0.0.1:${await controlPort(db, control)}`;
});
test.afterAll(() => { control?.kill(); github?.close(); provider?.close(); if (directory) rmSync(directory, { recursive: true, force: true }); });

test("cloud-only account saves a secret, runs, reconnects, approves and downloads results", async ({ page }) => {
  test.setTimeout(60000);
  await page.goto(base); await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  await page.getByRole("button", { name: "Cloud agents", exact: true }).click();
  await page.getByText("Manage agents", { exact: true }).click();
  await page.getByRole("button", { name: "Create agent", exact: true }).click();
  const editor = page.locator("form").filter({ has: page.getByRole("button", { name: "Save agent", exact: true }) });
  await editor.getByLabel("Name", { exact: true }).fill("Custom reviewer");
  await editor.getByLabel("Instructions", { exact: true }).fill("Review the requested changes carefully.");
  await editor.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(page.getByRole("option", { name: "Custom reviewer", exact: true })).toHaveCount(1);
  await page.getByText("Import an existing Managoat agent", { exact: true }).click();
  await page.getByLabel("Old agent ID", { exact: true }).fill("legacy");
  await page.getByLabel("Old API credential", { exact: true }).fill("legacy-test");
  await page.getByRole("button", { name: "Import definition", exact: true }).click();
  await expect(page.getByRole("option", { name: "Imported reviewer", exact: true })).toHaveCount(1);
  await expect(page.getByLabel("Old API credential", { exact: true })).toHaveValue("");
  await page.getByText("Secrets", { exact: true }).click();
  const secrets = page.locator("details").filter({ has: page.locator("summary", { hasText: /^Secrets$/ }) });
  await secrets.getByLabel("Name", { exact: true }).fill("GITHUB_TOKEN");
  await secrets.getByLabel("Value", { exact: true }).fill("test-repository-secret");
  await secrets.getByRole("button", { name: "Save secret" }).click();
  await expect(secrets.getByRole("button", { name: "Remove" })).toBeVisible();
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  await expect(page.getByLabel("Run on")).toHaveValue("cloud");
  await page.getByLabel("Repository URL").fill("https://github.com/example/repo");
  await page.getByLabel("Repository credential").selectOption("GITHUB_TOKEN");
  await page.getByLabel("Task", { exact: true }).fill("Fix the example");
  await page.getByRole("button", { name: "Run in cloud" }).click();
  await expect(page.getByRole("button", { name: "Allow once" })).toBeVisible({ timeout: 30000 });
  expect(secret).toBe("test-repository-secret");
  await page.goto(`${base}/#swarm`);
  const cloudCard = page.locator('.swarm-card[data-kind="input"]').filter({ hasText: "Fix the example" });
  await expect(cloudCard).toBeVisible();
  await cloudCard.locator("[data-open-card]").click();
  await expect(page.getByRole("button", { name: "Allow once" })).toBeVisible();
  await expect(page.locator("[data-cloud-pane]")).toBeVisible();
  await expect(page.getByRole("tab", { name: /Cloud/ })).toHaveAttribute("aria-selected", "true");
  await page.reload();
  await expect(page.getByRole("button", { name: "Allow once" })).toBeVisible();
  await page.getByRole("button", { name: "Allow once" }).click();
  await expect(page.locator("[data-cloud-pane] .agent-status", { hasText: "completed" })).toBeVisible({ timeout: 30000 });
  await page.getByRole("button", { name: "Run again", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Run again" })).toContainText("fresh checkout");
  await page.getByRole("dialog", { name: "Run again" }).getByRole("button", { name: "Cancel", exact: true }).click();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download results and transcript" }).click();
  expect((await download).suggestedFilename()).toMatch(/^run_.*\.json$/);
  // Losing beta access must not hide saved work or its navigation entry.
  await page.route("**/control.json", async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, json: { ...await response.json(), cloud: false } });
  });
  await page.route("**/api/cloud/agents", (route) => route.fulfill({ status: 403, json: { error: "Cloud beta access is disabled" } }));
  await page.route("**/api/cloud/secrets", (route) => route.fulfill({ status: 503, json: { error: "Secrets are unavailable" } }));
  await page.reload();
  await page.getByRole("button", { name: "Close cloud pane" }).click();
  await page.getByRole("button", { name: "Cloud agents", exact: true }).click();
  await expect(page.getByText("New cloud runs are unavailable:", { exact: false })).toBeVisible();
  await expect(page.getByRole("button", { name: "Run in cloud" })).toBeDisabled();
  await page.getByRole("button", { name: "Coding agent: Fix the example" }).click();
  await expect(page.locator("[data-cloud-pane] .agent-status", { hasText: "completed" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Download results and transcript" })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole("tab", { name: /Cloud.*Fix the example/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Close cloud pane" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: "/tmp/arugula-cloud-mobile.png" });
  // Provider lifecycle failures may have no transcript blocks.
  await page.route("**/api/cloud/runs/*/events?*", (route) => route.fulfill({ json: { events: [
    { id: 1, stage: "turn", state: "failed", data: JSON.stringify({ reason: "The selected model is unavailable." }), blocks: [] },
  ] } }));
  await page.reload();
  await expect(page.getByText("The selected model is unavailable.", { exact: true })).toBeVisible();
});

test("a Cloud workspace sets up, runs two tabs, asks in the drawer and is deleted", async ({ page }) => {
  test.setTimeout(90000);
  // Wide enough for the session sidebar beside the panes.
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 8, login: "workspace-tester" };
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
  expect([...conversations.values()].every((c) => c.status === "terminated")).toBe(true);
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
  expect(environment.packages).toEqual({ apt: ["ffmpeg", "jq"] });

  // The tab is as it was: still open, its transcript there, its
  // conversation not ended or prompted again.
  await expect(page.locator("[data-cloud-tab]")).toHaveCount(1);
  await page.locator(`[data-cloud-tab="${tab}"]`).click();
  await expect(page.locator("[data-cloud-tab-view]")).toContainText("Docs written.");
  const conversation = [...conversations.values()].find((c) => c.prompt === "Write the docs" && c.status !== "terminated");
  expect(conversation).toBeTruthy();
  await expect(page.locator("[data-cloud-status]")).toHaveText("Rebuild needed · 1 change");
});

test("Rebuild asks first, warns that the disk is wiped, and clears the badge", async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 10, login: "rebuild-tester" };
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
  expect(rebuilds).toHaveLength(0);

  // No machine free: it waits, says so, and Cancel stops control asking.
  noMachine = true;
  await page.locator('[data-rebuild-cloud-workspace="header"]').click();
  await confirm.locator("[data-confirm-rebuild]").click();
  await expect(confirm).toHaveCount(0);
  const waiting = page.locator('[data-rebuild-waiting="header"]');
  await expect(waiting).toContainText("Rebuild waiting for a machine");
  await expect(page.locator('[data-rebuild-waiting="settings"]')).toBeVisible();
  await expect(page.locator("[data-rebuild-queued]")).toBeVisible();
  await expect.poll(() => rebuilds.length, { timeout: 15000 }).toBeGreaterThan(1);
  await waiting.locator("[data-cancel-queued-rebuild]").click();
  await expect(page.locator("[data-rebuild-waiting]")).toHaveCount(0);
  await expect(page.locator('[data-rebuild-cloud-workspace="header"]')).toBeVisible();
  const asked = rebuilds.length;
  await page.waitForTimeout(3000);
  expect(rebuilds).toHaveLength(asked);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Rebuild needed · 1 change");
  noMachine = false;
  rebuilds.length = 0;

  // Settings' Rebuild, confirmed: setting up, then ready with no badge.
  await page.locator('[data-rebuild-cloud-workspace="settings"]').click();
  await confirm.locator("[data-confirm-rebuild]").click();
  await expect(confirm).toHaveCount(0);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Setting up", { timeout: 15000 });
  expect(rebuilds).toEqual([{}]);
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
  user = { id: 11, login: "retry-tester" };
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
  const rebuildsBefore = rebuilds.length;
  await page.locator("[data-retry-cloud-workspace]").click();
  await expect(page.getByRole("alertdialog")).toHaveCount(0);
  await expect(page.locator("[data-cloud-status]")).toHaveText("Ready", { timeout: 30000 });
  await expect(page.locator("[data-cloud-failed]")).toHaveCount(0);
  expect(retries).toHaveLength(1);
  expect(rebuilds).toHaveLength(rebuildsBefore);
});
