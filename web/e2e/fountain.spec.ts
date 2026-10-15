// M43: the Fountain agent catalog, against a fake Fountain served here
// (synthetic agents shaped like S24's) and the credentials file the config
// points the daemon at (this spec writes it). *Run on Fountain* runs the
// config's stand-in `fountain`, the fake ACP agent. Nothing here reaches a
// real Fountain.
//
// Opened from a pane's menu ("Fountain…"), the account view (#942 F2)
// shows the account's conversations first: an unread dot, label chips that
// filter with Fountain's ?label=, a fan-out's children under their parent,
// and a row that opens the conversation in an agent block beside it. Its
// Agents tab lists every agent; the agent-specs chip leaves the curated ones, a search finds an
// agent by its skill, and *Run on Fountain* opens an agent block beside the
// catalog that answers a prompt. The cards are read-only: no *Run here*
// (M44, retired 2026-10-15), and *Spec* without an agent-specs checkout
// shows the agent as Fountain returns it, in the block.

import { readFileSync, rmSync, writeFileSync } from "node:fs";
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
    if (u.pathname === "/api/agents") return json(res, 200, agents);
    const one = u.pathname.match(/^\/api\/agents\/([^/]+)$/);
    if (one) {
      const a = agents.data.find((x: { id: string }) => x.id === decodeURIComponent(one[1]));
      return a ? json(res, 200, { data: a }) : json(res, 404, { error: "not found" });
    }
    if (u.pathname === "/api/environments") return json(res, 200, envs);
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
  writeFileSync(process.env.ARUGULA_FOUNTAIN_CREDENTIALS!, `[default]\napi_key = "${KEY}"\nbase_url = "${origin}"\n`);
});

test.afterAll(() => {
  server.close();
  rmSync(process.env.ARUGULA_FOUNTAIN_CREDENTIALS!, { force: true });
});

async function openCatalog(page: Page): Promise<number> {
  await reset(page);
  const term = (await panes(page))[0];
  await menu(page, paneEl(page, term), "Fountain…");
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const block = await page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "fountain")!.id);
  await page.locator(`[data-fountain-block="${block}"] [data-fountain-tab="agents"]`).click();
  return block;
}

test("the account view lists conversations, filters by label and opens one", async ({ page }) => {
  await reset(page);
  const term = (await panes(page))[0];
  await menu(page, paneEl(page, term), "Fountain…");
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const block = await page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "fountain")!.id);
  const el = page.locator(`[data-fountain-block="${block}"]`);
  const rows = el.locator(".fountain-conv[data-conversation]");
  await expect(rows).toHaveCount(3);
  // The child right after its parent; the unread dot on the parent.
  await expect(rows.nth(0)).toHaveAttribute("data-conversation", "conv-flaky");
  await expect(rows.nth(1)).toHaveAttribute("data-conversation", "conv-child");
  await expect(rows.nth(0).locator("[data-unread]")).toBeVisible();
  await expect(rows.nth(0).locator("[data-status]")).toHaveText("running");
  await expect(rows.nth(2).locator(".fountain-conv-title")).toHaveText("games conversation");
  // A label chip filters with Fountain's ?label=.
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

test("the catalog filters, and Run on Fountain opens an agent block beside it", async ({ page }) => {
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

  // Clear, then find games and run it on Fountain.
  await el.locator("[data-fountain-clear]").click();
  await expect(cards).toHaveCount(108);
  await el.locator(".fountain-search").fill("games");
  await expect(el.locator('.fountain-card[data-agent="games"]')).toBeVisible();
  await el.locator('.fountain-card[data-agent="games"] [data-run]').click();
  await expect.poll(async () => (await panes(page)).length).toBe(3);
  const agent = await page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "agent")!.id);
  expect(await panes(page)).toContain(agent);
  // It's today's Fountain agent block (here the fake ACP agent), and works.
  expect((await blockState(agent)).label).toBe("Fountain games");
  const a = paneEl(page, agent);
  await a.locator(".agent-composer textarea").fill("hello");
  await a.locator(".agent-composer textarea").press("Enter");
  await expect(a.locator(".agent-msg").last()).toHaveText("Hello! I am fake.");
});

// *Spec* with no agent-specs checkout: the agent as Fountain returns it
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
