// A team's context writes team Docs (Arugula #862, aru-139). Alice owns team
// Acme, and her machine geek is the team's. An agent on geek (here, its
// requests by hand over /mcp) calls `doc new`: without `docs.write` over the
// project that is control's own refusal, and nothing is kept on the machine;
// with it, the Doc is the team's, in its project, and three fills land there
// (Alice reads them from control, as a teammate would). The block shows it,
// survives a restart of the machine, and with control gone shows the last
// read with "Offline: read-only" while a fill fails with that. `<state>/docs`
// never holds it. A local control (team-fixture.ts), as access-platform.spec.ts
// runs it.

import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { expect, request, test, type APIRequestContext, type Page } from "@playwright/test";
import { bring } from "./doc-steps";
import { closeContexts } from "./helpers";
import { daemonPort } from "./ports";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("docteam");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

/** One MCP session over Streamable HTTP (mcp.spec.ts's); the answer as it
 * came, an error too. */
async function mcp(req: APIRequestContext, name: string) {
  const base = { "Content-Type": "application/json", Accept: "application/json, text/event-stream" };
  const messages = (body: string) =>
    body
      .split("\n\n")
      .map((e) => e.split("\n").filter((l) => l.startsWith("data:")).map((l) => l.slice(5).trim()).join(""))
      .filter((d) => d)
      .map((d) => JSON.parse(d));
  const init = await req.post("/mcp", {
    headers: base,
    data: { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name, version: "1" } } },
  });
  expect(init.status()).toBe(200);
  const headers = { ...base, "Mcp-Session-Id": init.headers()["mcp-session-id"], "MCP-Protocol-Version": "2025-06-18" };
  await req.post("/mcp", { headers, data: { jsonrpc: "2.0", method: "notifications/initialized" } });
  let id = 2;
  return async (tool: string, args: object) => {
    const res = await req.post("/mcp", { headers, data: { jsonrpc: "2.0", id: ++id, method: "tools/call", params: { name: tool, arguments: args } }, timeout: 60_000 });
    const answer = messages(await res.text()).find((m) => m.id === id);
    return { error: !!answer?.result?.isError, text: (answer?.result?.content?.[0]?.text ?? "") as string, out: answer?.result?.structuredContent };
  };
}

async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
    },
    [method, path, body] as const,
  );
}

/** The Docs `<state>/docs/` holds (its dot files are the library's marks). */
const library = (state: string) => (existsSync(join(state, "docs")) ? readdirSync(join(state, "docs")).filter((f) => !f.startsWith(".")) : []);

test("a team machine's doc new is the team's Doc: refused without docs.write, written through control, offline read-only", async ({ browser }) => {
  test.setTimeout(300_000);
  await t.start(["--platform-host", "app.localhost"]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const state = await t.addMachine(alice, "geek", team);

  await alice.goto(`${app}/`);
  await expect(alice.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  // The machine's own port: it moves when the daemon is started again.
  const base = async () => {
    for (let i = 0; i < 200; i++) {
      try {
        const b = `http://127.0.0.1:${await daemonPort(state)}`;
        if ((await fetch(`${b}/api/host`)).status < 500) return b;
      } catch {
        // not yet
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    throw new Error("geek didn't come up");
  };
  let agent = await request.newContext({ baseURL: await base() });
  let call = await mcp(agent, "claude-code");
  const sections = [
    { heading: "Goals", intent: "what we commit to" },
    { heading: "Plan", intent: "steps and dates" },
    { heading: "Risks", intent: "what could go wrong" },
  ];

  // No docs.write: control's own words, and the Doc is not kept here instead.
  const refused = await call("doc", { kind: "new", title: "Quarterly plan", sections });
  expect(refused.error).toBe(true);
  expect(refused.text).toContain("docs.write");
  expect(library(state), "nothing kept on the machine").toEqual([]);

  // Alice gives it the power.
  const mine = await api(alice, "GET", "/api/grants");
  const geek = (mine.body!.grants as { subject: { id: string; name: string } }[]).find((g) => g.subject.name === "geek")!.subject.id;
  expect((await api(alice, "POST", "/api/grants", { daemon: geek, power: "docs.write", scope: { kind: "project", id: project } })).status).toBe(200);

  const made = await call("doc", { kind: "new", title: "Quarterly plan", sections });
  expect(made.error, made.text).toBe(false);
  expect(made.out.summary).toContain("A team Doc in ");
  expect(made.out.home).toBe("team");
  const doc: number = made.out.block;
  const id: string = made.out.doc;
  expect(id).toMatch(/^pdc_[0-9a-f]{32}$/);
  for (const [i, s] of ["s1", "s2", "s3"].entries()) {
    const filled = await call("doc", { kind: "fill", block: doc, section: s, markdown: `Written **${i + 1}**.` });
    expect(filled.error, filled.text).toBe(false);
  }
  // In the team's project: Alice reads it from control, as a teammate would.
  const theirs = await api(alice, "GET", `/api/projects/${project}/docs/${id}`);
  expect(theirs.status).toBe(200);
  expect((theirs.body!.sections as { body: string }[]).map((s) => s.body)).toEqual(["Written **1**.", "Written **2**.", "Written **3**."]);
  expect(library(state), "ls <state>/docs stays library-only").toEqual([]);

  // The block shows it.
  const shown = async () => {
    const shell = await browser.newPage();
    await shell.goto(await base());
    await expect.poll(() => shell.evaluate(() => window.__arugula?.client.connected), { timeout: 30_000 }).toBe(true);
    await bring(shell, doc);
    const block = shell.locator(`[data-doc-block="${doc}"]`);
    await expect(block.locator(".doc-title")).toHaveText("Quarterly plan", { timeout: 30_000 });
    await expect(block.locator('[data-section="s3"] strong')).toHaveText("3", { timeout: 15_000 });
    return { shell, block };
  };
  let seen = await shown();
  await expect(seen.shell.locator("[data-doc-offline]")).toHaveCount(0);

  // A restart of the machine: the block is still the team's Doc.
  await seen.shell.close();
  await agent.dispose();
  await t.restartMachine("geek");
  seen = await shown();
  await expect(seen.shell.locator("[data-doc-offline]")).toHaveCount(0);

  // Control goes away: the last read stays, read-only, and a fill says why.
  t.stopControl();
  await seen.shell.reload();
  await expect(seen.shell.locator("[data-doc-offline]")).toHaveText("Offline: read-only", { timeout: 60_000 });
  await expect(seen.shell.locator(`[data-doc-block="${doc}"] [data-section="s3"] strong`)).toHaveText("3");
  agent = await request.newContext({ baseURL: await base() });
  call = await mcp(agent, "claude-code");
  const offline = await call("doc", { kind: "fill", block: doc, section: "s1", markdown: "Not saved." });
  expect(offline.error).toBe(true);
  expect(offline.text).toContain("Offline: read-only");
  await expect(seen.shell.locator(`[data-doc-block="${doc}"] [data-section="s1"]`)).not.toContainText("Not saved.");

  // ...and a restart with control still gone shows the same.
  await seen.shell.close();
  await agent.dispose();
  await t.restartMachine("geek");
  seen = await shown();
  await expect(seen.shell.locator("[data-doc-offline]")).toHaveText("Offline: read-only", { timeout: 60_000 });
  expect(library(state), "still library-only").toEqual([]);
  await seen.shell.close();
});
