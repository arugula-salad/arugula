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
const machineReads: string[] = [];
const conversations = new Map<string, { status: string; prompt: string; answered: boolean }>();
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
    else if (path === "/api/workspaces" && req.method === "POST") data = { id: "ws", name: body.name, environment_id: body.environment_id, vault_id: body.vault_id, status: "provisioning", sandbox_status: "pending", error: null, revision: "rev1", built_from_revision: null, stale: false, changed_fields: [] };
    // Setting up for the first reads, then ready.
    else if (path === "/api/workspaces/ws") {
      const ready = ++workspaceReads > 2;
      data = { id: "ws", status: ready ? "ready" : "provisioning", sandbox_status: ready ? "ready" : "starting", error: null, revision: "rev1", built_from_revision: ready ? "rev1" : null, stale: false, changed_fields: [] };
    } else if (/^\/api\/workspaces\/ws\/(diff|git-status|file)$/.test(path)) {
      // The machine's reads (M118): the guide tab's change, once it ran.
      machineReads.push(`${path}${url.search}`);
      const wrote = [...conversations.values()].some((c) => c.prompt === "Write the guide");
      if (path.endsWith("/diff")) data = { path: url.searchParams.get("path"), repo_root: "/home/sprite/app", staged: false, ref: url.searchParams.get("ref"), truncated: false, diff: wrote ? "diff --git a/README.md b/README.md\nindex 1..2 100644\n--- a/README.md\n+++ b/README.md\n@@ -1 +1,2 @@\n # app\n+Docs for the app.\n" : "" };
      else if (path.endsWith("/git-status")) data = { path: url.searchParams.get("path"), repo_root: "/home/sprite/app", branch: "main", untracked: "all", truncated: false, entries: wrote ? [{ path: "README.md", index: "unchanged", worktree: "modified", renamed_from: null }, { path: "docs/guide.md", index: "untracked", worktree: "untracked", renamed_from: null }] : [] };
      else if (url.searchParams.get("path") === "/home/sprite/app/docs/guide.md") data = { path: "/home/sprite/app/docs/guide.md", size: 13, truncated: false, encoding: "utf-8", content: "# Guide\nStart." };
      else { res.writeHead(404).end(); return; }
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
    else if (path === "/api/environments") data = { id: "env", revision: "rev1" };
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

test("a change an agent made in a Cloud workspace shows in Changes", async ({ page }) => {
  test.setTimeout(90000);
  await page.setViewportSize({ width: 1400, height: 900 });
  user = { id: 9, login: "changes-tester" };
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
  expect(machineReads).toContain("/api/workspaces/ws/diff?path=%2Fhome%2Fsprite%2Fapp&ref=HEAD");
  expect(machineReads).toContain("/api/workspaces/ws/git-status?path=%2Fhome%2Fsprite%2Fapp&untracked=all");

  await page.locator("[data-delete-cloud-workspace]").click();
  await page.getByRole("alertdialog", { name: "Delete docs box?" }).getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page.locator("[data-cloud-workspace]")).toHaveCount(0, { timeout: 45000 });
});
