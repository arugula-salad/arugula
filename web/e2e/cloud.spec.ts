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
test.beforeAll(async () => {
  github = createServer((req, res) => {
    const url = new URL(req.url!, "http://github");
    if (url.pathname === "/login/oauth/authorize") {
      const back = new URL(url.searchParams.get("redirect_uri")!);
      back.searchParams.set("code", "test"); back.searchParams.set("state", url.searchParams.get("state")!);
      res.writeHead(302, { location: back.href }).end();
    } else {
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify(url.pathname === "/user" ? { id: 7, login: "cloud-tester" } : { access_token: "test-token" }));
    }
  });
  provider = createServer(async (req, res) => {
    const chunks: Buffer[] = []; for await (const chunk of req) chunks.push(Buffer.from(chunk));
    const body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString()) : {};
    const url = new URL(req.url!, "http://provider"); const path = url.pathname;
    let data: unknown = {};
    if (req.method === "DELETE") { res.writeHead(204).end(); return; }
    if (path === "/api/agents/legacy") {
      if (req.headers.authorization !== "Bearer legacy-test") { res.writeHead(401).end(); return; }
      data = { name: "Imported reviewer", system: "Review the changes", skills: [], mcp_servers: {} };
    }
    else if (path === "/api/environments") data = { id: "env" };
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
  await page.getByLabel("Repository URL").fill("https://github.com/example/repo");
  await page.getByLabel("Repository credential").selectOption("GITHUB_TOKEN");
  await page.getByLabel("Task", { exact: true }).fill("Fix the example");
  await page.getByRole("button", { name: "Run in cloud" }).click();
  await expect(page.getByRole("button", { name: "Allow once" })).toBeVisible({ timeout: 30000 });
  expect(secret).toBe("test-repository-secret");
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await page.reload();
  await page.getByRole("button", { name: "Cloud agents", exact: true }).click();
  await page.getByRole("button", { name: "Coding agent: Fix the example" }).click();
  await page.getByRole("button", { name: "Allow once" }).click();
  await expect(page.getByRole("heading", { name: "Coding agent · completed" })).toBeVisible({ timeout: 30000 });
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
  await page.getByRole("button", { name: "Cloud agents", exact: true }).click();
  await expect(page.getByText("New cloud runs are unavailable:", { exact: false })).toBeVisible();
  await expect(page.getByRole("button", { name: "Run in cloud" })).toBeDisabled();
  await page.getByRole("button", { name: "Coding agent: Fix the example" }).click();
  await expect(page.getByRole("heading", { name: "Coding agent · completed" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Download results and transcript" })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: "/tmp/arugula-cloud-mobile.png" });
  // Provider lifecycle failures may have no transcript blocks.
  await page.route("**/api/cloud/runs/*/events?*", (route) => route.fulfill({ json: { events: [
    { id: 1, stage: "turn", state: "failed", data: JSON.stringify({ reason: "The selected model is unavailable." }), blocks: [] },
  ] } }));
  await page.getByRole("button", { name: "Back to runs" }).click();
  await page.getByRole("button", { name: "Coding agent: Fix the example" }).click();
  await expect(page.getByText("The selected model is unavailable.", { exact: true })).toBeVisible();
});
