// M43: the Managoat agent catalog, against a fake Managoat served here
// (synthetic agents shaped like S24's) and the credentials file the config
// points the daemon at (this spec writes it). *Run on Managoat* opens a
// conversation on the native link (#942, F3), which this fake answers over
// REST and SSE. Nothing here reaches a real Managoat.
//
// Opened from a pane's menu ("Managoat…"), the account view (#942 F2)
// shows the account's conversations first: an unread dot, label chips that
// filter with Managoat's ?label=, a fan-out's children under their parent,
// and a row that opens the conversation in an agent block beside it. Its
// Agents tab lists every agent; the agent-specs chip leaves the curated ones, a search finds an
// agent by its skill, and *Run on Managoat* opens an agent block beside the
// catalog that answers a prompt. The cards are read-only: no *Run here*
// (M44, retired 2026-10-15), and *Spec* without an agent-specs checkout
// shows the agent as Managoat returns it, in the block.
//
// F4: a recipe offered from a project (a git checkout of acme/widgets) has
// *Run on Managoat* on its card in the agents block. Its dialog shows the
// preflight (the checkout, what doesn't carry over, the variables Managoat
// lacks, each with an explicit copy that says when it's not on this
// machine), then the run applies `recipe-e2e-fixer` and the conversation it
// opens carries the recipe's hash and answers.

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer, type Server, type ServerResponse } from "node:http";
import { expect, test, type Page } from "@playwright/test";
import { menu, paneEl, panes, reset } from "./helpers";
import { listen } from "./ports";

const KEY = "e2e-fountain-key";
const fixture = (f: string) => JSON.parse(readFileSync(new URL(`../../crates/daemon/tests/fixtures/fountain/${f}`, import.meta.url), "utf8"));

let origin = "";
const games = "02fb18b8-fd17-4689-bbe3-3fe0c2dc6a3f";
const convs: { id: string; status: string; agent_id: string; title: string | null; unread: boolean; labels: Record<string, string>; parent_conversation_id?: string; runtime: string }[] = [
  { id: "conv-flaky", status: "running", agent_id: games, title: "fix the flaky test", unread: true, labels: { env: "prod" }, runtime: "claude" },
  { id: "conv-child", status: "idle", agent_id: games, title: "check the logs", unread: false, labels: { env: "dev" }, parent_conversation_id: "conv-flaky", runtime: "claude" },
  { id: "conv-cli", status: "idle", agent_id: games, title: null, unread: false, labels: { team: "a" }, runtime: "claude" },
];
const reads: string[] = [];
let server: Server;
/** F3: the conversations Run on Managoat makes (F4's too): their events
 *  (`[id, conversation, payload]`). */
const events: [number, string, Record<string, unknown>][] = [];
const stage = (c: string, stage: string, state: string, meta: Record<string, unknown> = {}) =>
  events.push([events.length + 1, c, { kind: "stage", stream: "", data: JSON.stringify(meta), stage, state, turn_id: meta.turn_id ?? null }]);
const said = (c: string, text: string, turn: string) =>
  events.push([
    events.length + 1,
    c,
    {
      kind: "output",
      stream: "acp",
      turn_id: turn,
      data: JSON.stringify({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "s", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text } } } }),
    },
  ]);
const made = (id: string) => ({ id, status: "idle", sandbox_id: `sb-${id}`, sandbox: { id: `sb-${id}`, status: "ready" } });
const conv = made("conv-e2e");
/** F4: what the runs applied and created, and the agents and environments
 *  they made. */
const applied: { kind: string; name: string; spec: Record<string, any> }[] = [];
const created: Record<string, any>[] = [];
const upserted: Record<string, any>[] = [];

/** SSE after `Last-Event-ID`, held open (unless `wait=false`) until quiet.
 *  Only conv-e2e has events. */
function stream(req: import("node:http").IncomingMessage, res: ServerResponse, id: string, wait: boolean) {
  let last = Number(req.headers["last-event-id"] ?? 0);
  res.writeHead(200, { "content-type": "text/event-stream" });
  res.write(": connected\n\n");
  const flush = () => {
    let sent = false;
    for (const [n, , p] of events.filter(([n, c]) => n > last && c === id)) {
      res.write(`id: ${n}\nevent: ${p.kind}\ndata: ${JSON.stringify(p)}\n\n`);
      last = n;
      sent = true;
    }
    return sent;
  };
  flush();
  if (!wait) return res.end();
  let quiet = Date.now();
  const t = setInterval(() => {
    if (flush()) quiet = Date.now();
    else if (Date.now() - quiet > 2000) {
      clearInterval(t);
      res.end();
    }
  }, 50);
  req.on("close", () => clearInterval(t));
}
const seen: { path: string; ua: string }[] = [];

function json(res: ServerResponse, status: number, v: unknown) {
  res.writeHead(status, { "content-type": "application/json" }).end(JSON.stringify(v));
}

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  const agents = fixture("agents.json");
  const envs = fixture("environments.json");
  server = createServer((req, res) => {
    const u = new URL(req.url!, "http://fountain");
    seen.push({ path: u.pathname, ua: String(req.headers["user-agent"] ?? "") });
    if (req.headers.authorization !== `Bearer ${KEY}`) return json(res, 401, { error: { message: "invalid api key" } });
    if (u.pathname === "/api/agents") return json(res, 200, { data: [...agents.data, ...upserted.filter((a) => a.kind === "Agent")] });
    const one = u.pathname.match(/^\/api\/agents\/([^/]+)$/);
    if (one) {
      const a = agents.data.find((x: { id: string }) => x.id === decodeURIComponent(one[1]));
      return a ? json(res, 200, { data: a }) : json(res, 404, { error: "not found" });
    }
    if (u.pathname === "/api/environments") return json(res, 200, { data: [...envs.data, ...upserted.filter((a) => a.kind === "Environment")] });
    // F4: a vault, keys only (none set), and apply.
    if (u.pathname === "/api/vaults") return json(res, 200, { data: [{ id: "vault-1", name: "v1" }] });
    if (/^\/api\/(vaults|environments)\/[^/]+\/secrets$/.test(u.pathname)) return json(res, 200, { data: [] });
    if (u.pathname === "/api/apply" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        const results = (JSON.parse(body).resources as { kind: string; name: string; spec: Record<string, any> }[]).map((r) => {
          applied.push(r);
          const had = upserted.find((a) => a.kind === r.kind && a.name === r.name);
          if (!had) upserted.push({ ...r.spec, kind: r.kind, name: r.name, id: `${r.kind.toLowerCase()}-${r.name}` });
          return { kind: r.kind, name: r.name, action: had ? "unchanged" : "created", errors: null, secrets: [] };
        });
        json(res, 200, { data: { results } });
      });
      return;
    }
    // F3: Run on Managoat's conversation, which echoes as the fake ACP
    // agent did; the listed ones can be opened and followed (quietly).
    // Path segments come percent-encoded, as a server decodes them.
    const path = decodeURIComponent(u.pathname);
    if (path === "/api/conversations" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        const b = JSON.parse(body);
        // A recipe's run (F4) gets a conversation of its own.
        const c = b.labels?.["arugula.recipe"] ? made(`conv-f4-${created.length + 1}`) : conv;
        if (c !== conv) created.push({ ...b, id: c.id });
        stage(c.id, "provision", "done");
        json(res, 201, { data: c });
      });
      return;
    }
    const it = path.match(/^\/api\/conversations\/([^/]+)(\/(turns|stream|prompts))?$/);
    const found = it && (it[1] === conv.id ? conv : (created.find((c) => c.id === it[1]) && made(it[1])) || convs.find((c) => c.id === it[1]));
    if (it && found) {
      if (!it[3]) return json(res, 200, { data: found });
      if (it[3] === "turns") return json(res, 200, { data: [] });
      if (it[3] === "stream") return stream(req, res, it[1], u.searchParams.get("wait") !== "false");
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        const crid = JSON.parse(body).client_request_id ?? null;
        const turn = `turn-${events.length}`;
        stage(it[1], "turn", "started", { turn_id: turn, client_request_id: crid });
        said(it[1], "Hello! I am fake.", turn);
        stage(it[1], "turn", "done", { turn_id: turn, stop_reason: "end_turn" });
        json(res, 200, { data: { status: "accepted", client_request_id: crid } });
      });
      return;
    }
    if (u.pathname === "/api/conversations") {
      const want = u.searchParams.getAll("label");
      const data = convs.filter((c) => want.every((l) => { const [k, v] = [l.slice(0, l.indexOf(":")), l.slice(l.indexOf(":") + 1)]; return c.labels[k] === v; }));
      return json(res, 200, { data });
    }
    const read = u.pathname.match(/^\/api\/conversations\/([^/]+)\/read$/);
    if (read && req.method === "POST") {
      const c = convs.find((x) => x.id === decodeURIComponent(read[1]));
      if (c) c.unread = false;
      reads.push(decodeURIComponent(read[1]));
      res.writeHead(204).end();
      return;
    }
    json(res, 404, { error: "not here" });
  });
  origin = `http://127.0.0.1:${await listen(server)}`;
  writeFileSync(process.env.ARUGULA_MANAGOAT_CREDENTIALS!, `[default]\napi_key = "${KEY}"\nbase_url = "${origin}"\n`);
});

test.afterAll(() => {
  server.close();
  rmSync(process.env.ARUGULA_MANAGOAT_CREDENTIALS!, { force: true });
});

async function openCatalog(page: Page): Promise<number> {
  await reset(page);
  const term = (await panes(page))[0];
  await menu(page, paneEl(page, term), "Managoat…");
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const block = await page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "managoat")!.id);
  await page.locator(`[data-fountain-block="${block}"] [data-fountain-tab="agents"]`).click();
  return block;
}

test("the account view lists conversations, filters by label and opens one", async ({ page }) => {
  await reset(page);
  const term = (await panes(page))[0];
  await menu(page, paneEl(page, term), "Managoat…");
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const block = await page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "managoat")!.id);
  const el = page.locator(`[data-fountain-block="${block}"]`);
  const rows = el.locator(".fountain-conv[data-conversation]");
  await expect(rows).toHaveCount(3);
  // The child right after its parent; the unread dot on the parent.
  await expect(rows.nth(0)).toHaveAttribute("data-conversation", "conv-flaky");
  await expect(rows.nth(1)).toHaveAttribute("data-conversation", "conv-child");
  await expect(rows.nth(0).locator("[data-unread]")).toBeVisible();
  await expect(rows.nth(0).locator("[data-status]")).toHaveText("running");
  await expect(rows.nth(2).locator(".fountain-conv-title")).toHaveText("games conversation");
  // A label chip filters with Managoat's ?label=.
  await el.locator('[data-chips="labels"] [data-label="team:a"]').click();
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toHaveAttribute("data-conversation", "conv-cli");
  await el.locator("[data-labels-clear]").click();
  await expect(rows).toHaveCount(3);
  // Opening one: an agent block beside the view, and it's read.
  await rows.nth(0).locator("[data-open]").click();
  await expect.poll(async () => (await panes(page)).length).toBe(3);
  expect(reads).toContain("conv-flaky");
  await expect(rows.nth(0).locator("[data-unread]")).toHaveCount(0);
});

test("the catalog filters, and Run on Managoat opens an agent block beside it", async ({ page }) => {
  const block = await openCatalog(page);
  const el = page.locator(`[data-fountain-block="${block}"]`);
  const cards = el.locator(".fountain-card");
  // Every agent, app-made ones too.
  await expect(cards).toHaveCount(108);
  await expect(el.locator("[data-fountain-count]")).toHaveText("108");
  expect(seen.length).toBeGreaterThan(0);
  expect(seen.every((s) => s.ua.startsWith("arugula/"))).toBe(true);
  await expect(el.locator('.fountain-card[data-source="app"]').first()).toBeVisible();

  // The agent-specs chip leaves the curated ones.
  await el.locator('[data-chips="source"] [data-chip="agent-specs"]').click();
  await expect(cards).toHaveCount(23);
  await expect(el.locator("[data-fountain-count]")).toHaveText("23 of 108");
  await expect(el.locator('[data-chips="source"] [data-chip="agent-specs"]')).toHaveAttribute("aria-pressed", "true");
  // A skill finds its agent.
  await el.locator(".fountain-search").fill("frontend-design");
  await expect(cards).toHaveCount(1);
  await expect(cards.first()).toHaveAttribute("data-agent", "designer");
  // The filter is the block's: kept in its state (and config).
  const blockState = (b: number) =>
    page.evaluate((b) => window.__arugula.client.request("GET", `/api/blocks/${b}`).then((r) => r.json<{ state: Record<string, any> }>()), b).then((v) => v.state);
  await expect.poll(() => blockState(block).then((s) => s.filter.query)).toBe("frontend-design");
  // Spec is offered; Run here (M44) is gone.
  const designer = el.locator('.fountain-card[data-agent="designer"]');
  await expect(designer.locator("[data-spec]")).toBeEnabled();
  await expect(el.locator("[data-run-here]")).toHaveCount(0);

  // Clear, then find games and run it on Managoat.
  await el.locator("[data-fountain-clear]").click();
  await expect(cards).toHaveCount(108);
  await el.locator(".fountain-search").fill("games");
  await expect(el.locator('.fountain-card[data-agent="games"]')).toBeVisible();
  await el.locator('.fountain-card[data-agent="games"] [data-run]').click();
  await expect.poll(async () => (await panes(page)).length).toBe(3);
  const agent = await page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "agent")!.id);
  expect(await panes(page)).toContain(agent);
  // A Managoat conversation on the native link (F3), and it works.
  expect((await blockState(agent)).label).toBe("Managoat games");
  const a = paneEl(page, agent);
  await expect(a.locator("[data-fountain-line]")).toHaveAttribute("data-fountain-line", "idle·awake");
  await expect(a.locator("[data-fountain-terminate]")).toBeVisible();
  await a.locator(".agent-composer textarea").fill("hello");
  await a.locator(".agent-composer textarea").press("Enter");
  await expect(a.locator(".agent-msg").last()).toHaveText("Hello! I am fake.");
});

// *Spec* with no agent-specs checkout: the agent as Managoat returns it
// (GET /api/agents/{id}), shown in the block, not a link to a console page.
test("Spec shows the agent in the block", async ({ page }) => {
  const block = await openCatalog(page);
  const el = page.locator(`[data-fountain-block="${block}"]`);
  await el.locator(".fountain-search").fill("games");
  const games = el.locator('.fountain-card[data-agent="games"]');
  await expect(games).toBeVisible();
  await games.locator("[data-spec]").click();
  const spec = el.locator('[data-fountain-spec="games"]');
  await expect(spec).toBeVisible();
  await expect(spec.locator(".fountain-spec-json")).toContainText('"name": "games"');
  await expect(spec.locator(".fountain-spec-json")).toContainText("love2d");
  expect(seen.some((s) => s.path.startsWith("/api/agents/"))).toBe(true);
  expect((await panes(page)).length).toBe(2);
  await spec.locator("[data-fountain-spec-close]").click();
  await expect(spec).toHaveCount(0);
});

// F4: the same recipe that runs here runs on Managoat, from its card.
test("a recipe runs on Managoat from its card, after the preflight", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "arugula-e2e-f4-"));
  mkdirSync(join(dir, ".claude/agents"), { recursive: true });
  writeFileSync(
    join(dir, ".claude/agents/e2e-fixer.md"),
    "---\nname: e2e-fixer\ndescription: Fixes tests\ntools: Read, Edit\nmodel: haiku\nmcpServers:\n  - tracker:\n      type: http\n      url: https://tracker.example.com/mcp\n      headers:\n        Authorization: Bearer ${E2E_F4_NOT_SET}\n---\nYou fix tests.\n",
  );
  execFileSync("git", ["init", "-q", dir]);
  execFileSync("git", ["-C", dir, "remote", "add", "origin", "https://github.com/acme/widgets.git"]);
  try {
    await reset(page);
    const block = await page.evaluate(
      (dir) => window.__arugula.client.request("POST", "/api/blocks", { type: "agents", config: { dir }, local: true }).then((r) => r.json<{ block: number }>()).then((v) => v.block),
      dir,
    );
    const offered = await page.evaluate(([b, dir]) => window.__arugula.client.request("POST", `/api/blocks/${b}/call/offer`, { agent: "e2e-fixer", dir }).then((r) => r.status), [block, dir] as const);
    expect(offered).toBe(200);
    const el = page.locator(`[data-agents-block="${block}"]`);
    // Beside Run on this machine, from the same card.
    const card = el.locator('.fountain-card[data-agent="e2e-fixer"]');
    await expect(card.locator("[data-run-here]")).toBeVisible();
    await card.locator('[data-run-fountain="e2e-fixer"]').click();

    // The preflight: the checkout, what doesn't carry over, what's missing.
    const dlg = page.locator('[data-fountain-run="e2e-fixer"]');
    await expect(dlg.locator("[data-fountain-repo]")).toContainText("https://github.com/acme/widgets.git");
    await expect(dlg.locator("[data-fountain-repo]")).toContainText("Uncommitted work on this machine doesn't travel");
    await expect(dlg.locator("[data-fountain-missing]")).toHaveAttribute("data-fountain-missing", "2");
    await expect(dlg.locator("[data-fountain-lost]")).toContainText("no tool allowlist");
    expect(applied).toHaveLength(0);
    // A copy is per variable, once a vault is chosen; one this machine
    // doesn't have says so.
    await expect(dlg.locator("[data-copy-var]")).toHaveCount(0);
    await dlg.locator("[data-fountain-vault]").selectOption("v1");
    await dlg.locator('[data-copy-var="E2E_F4_NOT_SET"]').click();
    await expect(dlg.locator("[data-fountain-said]")).toContainText("isn't set on this machine");

    // Run: the agent applied, the conversation labelled, and it answers.
    await dlg.locator("textarea").fill("hello");
    await dlg.locator("[data-fountain-go]").click();
    await expect(dlg).toHaveCount(0);
    await expect.poll(() => applied.map((a) => `${a.kind} ${a.name}`)).toEqual(["Environment repo-acme-widgets", "Agent recipe-e2e-fixer"]);
    const spec = applied[1].spec;
    expect(spec.metadata["managed-by"]).toBe("arugula");
    expect(spec.mcp_servers.tracker.headers.Authorization).toBe("Bearer ${E2E_F4_NOT_SET}");
    await expect.poll(() => created.length).toBe(1);
    expect(created[0].labels["arugula.recipe"]).toBe("e2e-fixer");
    expect(created[0].labels["arugula.recipe_hash"]).toBe(spec.metadata.recipe_hash);
    expect(created[0].vault_id).toBe("vault-1");
    const agent = await page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "agent")!.id);
    const a = paneEl(page, agent);
    await expect(a.locator(".agent-msg").last()).toHaveText("Hello! I am fake.");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
