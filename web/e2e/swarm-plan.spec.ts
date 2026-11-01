// #445: the swarm's plan board, the steps inside agent sessions. Against
// the fake fleet (e2e/fake-fleet.ts) with agent blocks running the scripted
// fake ACP agent, which writes its task list to the session's transcript
// as Claude Code's task tools do; the daemon reads it from there
// (`PaneInfo.plan`). One session works through its steps, one blocked on
// another; one asks a question mid-step (Review, saying the question); a
// terminal's Claude Code asks to run something with no plan (a Review card
// of its own, "Bash wants to run …"); one has no list (the strip). A card
// opens its pane, on a laptop and on a phone, where Review comes first.

import { writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { devices, expect, test, type Page } from "@playwright/test";
import { FakeFleet } from "./fake-fleet";

let fake: FakeFleet;
const panes: Record<string, string> = {};

test.use({ baseURL: async ({}, use) => use(fake?.machines[0]?.url) });
test.describe.configure({ mode: "serial" });

const QUESTION = "Where should search results be cached?";

/** An agent block on `machine` playing `steps` as its first turn. */
async function agent(machine: string, name: string, steps: object[]) {
  const file = join(fake.root, `${name}.json`);
  writeFileSync(file, JSON.stringify({ turns: [steps], then: [{ say: "Done." }] }));
  const m = fake.get(machine);
  const command = ["python3", resolve(import.meta.dirname, "../../crates/daemon/tests/fake_acp.py"), "--demo", file];
  const res = await fetch(`${m.url}/api/blocks`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ type: "agent", config: { agent: "claude", command, cwd: fake.dir("api"), prompt: "go" } }),
  });
  if (!res.ok) throw new Error(`blocks: ${res.status} ${await res.text()}`);
  panes[name] = `${machine}:${((await res.json()) as { block: number }).block}`;
}

test.beforeAll(async () => {
  fake = new FakeFleet();
  await fake.machine("workstation");
  await fake.machine("build-01");
  await fake.machine("build-02");
  await fake.populate();
  await agent("workstation", "worker", [
    { title: "Graceful worker shutdown" },
    { usage: 40_000, model: "claude-sonnet-5-5" },
    {
      tasks: [
        { subject: "Read the main loop", active: "Reading the main loop", status: "completed" },
        { subject: "Stop taking jobs on SIGTERM", active: "Handling SIGTERM", status: "in_progress" },
        { subject: "Test the drain", active: "Testing the drain", status: "pending" },
        { subject: "Ship it", active: "Shipping it", status: "pending", after: ["2"] },
      ],
    },
    { busy: "go test ./...", kind: "execute", secs: null },
  ]);
  await agent("build-01", "cache", [
    { title: "Cache product search" },
    { usage: 30_000, model: "claude-opus-5-5" },
    {
      tasks: [
        { subject: "Profile the facet counts", active: "Profiling", status: "completed" },
        { subject: "Pick where results are cached", active: "Picking a cache", status: "in_progress" },
      ],
    },
    {
      question: {
        question: QUESTION,
        header: "Cache",
        multiSelect: false,
        options: [
          { label: "Redis", description: "Shared" },
          { label: "In-process LRU", description: "Nothing new to run" },
        ],
      },
    },
    { say: "Cached." },
  ]);
  await agent("workstation", "notes", [{ title: "Release notes" }, { say: "Written.", ms: 5 }]);
  panes.asks = `workstation:${await fake.agentAsks("workstation", "api", "cargo test -p plan")}`;
});

test.afterAll(() => fake?.stop());

interface Board {
  clusters: { name: string; n: number; need: number }[];
  cardsOf(): { key: string; pane: string; column: string; text: string; agent: string; why: string | null; after: string[] | null; more: number }[];
  noPlan(): string[];
  screenOfCard(key: string): { x: number; y: number } | null;
}
const board = <T,>(page: Page, fn: (b: Board, arg: string) => T, arg = "") =>
  page.evaluate(([f, a]) => new Function("b", "a", `return (${f})(b, a)`)(window.__arugula.swarm, a), [fn.toString(), arg] as const) as Promise<T>;
const isBoard = (page: Page) => page.evaluate(() => !!(window.__arugula.swarm as { cardsOf?: unknown } | null)?.cardsOf);
const cards = (page: Page) => board(page, (b) => b.cardsOf());

async function open(page: Page) {
  await page.addInitScript(() => localStorage.setItem("arugula.swarm.theme", "plan"));
  await page.goto("/#swarm");
  await expect
    .poll(() => page.evaluate(() => window.__arugula?.fleet.list.filter((h) => h.state === "connected").length ?? 0), { timeout: 20_000 })
    .toBe(3);
  await expect(page.locator(".swarm")).toHaveAttribute("data-theme", "plan");
  await expect.poll(() => isBoard(page), { timeout: 10_000 }).toBe(true);
  // Every session's list (and question) has reached the board.
  const want = ["blocked:Ship it", "done:Profile the facet counts", "done:Read the main loop", "in_progress:Handling SIGTERM", "pending:Test the drain", "review:Picking a cache"];
  await expect
    .poll(
      async () => {
        const all = await cards(page);
        const have = all.filter((c) => c.pane !== panes.asks).map((c) => `${c.column}:${c.text}`).sort();
        return JSON.stringify(have) === JSON.stringify(want) && all.some((c) => c.pane === panes.asks && c.column === "review");
      },
      { timeout: 60_000 },
    )
    .toBe(true);
}

test("each session's steps are in their columns, with why and what they wait on", async ({ page }) => {
  await open(page);
  const all = await cards(page);
  const of = (text: string) => all.find((c) => c.text === text)!;
  // The question, in the card's words; the agent and its pane.
  expect(of("Picking a cache")).toMatchObject({ column: "review", why: QUESTION, pane: panes.cache, agent: "Claude Code" });
  // The terminal asks with no plan: a Review card of its own.
  const asks = all.find((c) => c.pane === panes.asks)!;
  expect(asks).toMatchObject({ column: "review", why: "Bash wants to run cargo test -p plan" });
  expect(of("Ship it")).toMatchObject({ column: "blocked", after: ["Stop taking jobs on SIGTERM"], pane: panes.worker });
  expect(of("Handling SIGTERM").pane).toBe(panes.worker);
  // Counts per column, and the strip.
  const n = (name: string) => board(page, (b, x) => b.clusters.find((c) => c.name === x)?.n ?? -1, name);
  expect(await n("Review")).toBe(2);
  expect(await n("Done")).toBe(2);
  expect(await board(page, (b) => b.noPlan())).toContain(panes.notes);
  // Only agents: no shell, build or server is on it.
  expect(new Set(all.map((c) => c.pane)).size).toBe(3);
  await expect(page.locator('[data-theme-key="plan"] summary')).toHaveText("How to read the plan");
  await expect(page.locator(".swarm-legend")).toBeHidden();
});

test("a card opens its session's pane; a hover peeks at it", async ({ page }) => {
  await open(page);
  const key = (await cards(page)).find((c) => c.text === "Handling SIGTERM")!.key;
  await page.waitForTimeout(800);
  let pos = (await board(page, (b, k) => b.screenOfCard(k), key))!;
  await page.mouse.move(pos.x, pos.y);
  await expect(page.locator(".swarm-peek")).toBeVisible();
  pos = (await board(page, (b, k) => b.screenOfCard(k), key))!;
  await page.mouse.click(pos.x, pos.y);
  await expect(page.locator(".swarm")).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(Number(panes.worker.split(":")[1]));
});

test.describe("phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("columns stack with what waits on you first; a tap opens the pane", async ({ page }) => {
    await open(page);
    const y = async (text: string) => {
      const k = (await cards(page)).find((c) => c.text === text)!.key;
      return (await board(page, (b, x) => b.screenOfCard(x), k))!.y;
    };
    const asks = (await cards(page)).find((c) => c.pane === panes.asks)!.key;
    const top = (await board(page, (b, x) => b.screenOfCard(x), asks))!;
    expect(top.y).toBeLessThan(await y("Test the drain"));
    const rail = (await page.locator(".swarm-rail").boundingBox())!;
    expect(top.y).toBeLessThan(rail.y);
    await page.touchscreen.tap(top.x, top.y);
    await expect(page.locator(".swarm")).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(Number(panes.asks.split(":")[1]));
  });
});

// Last: it changes what the others expect.
test("answering the question moves its step back to In progress", async ({ page }) => {
  await open(page);
  const [machine, pane] = panes.cache.split(":");
  const url = fake.get(machine).url;
  const call = async (what: string, body: object) =>
    (await (await fetch(`${url}/api/blocks/${pane}/call/${what}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) })).json()) as {
      asks?: { id: unknown }[];
    };
  const asks = (await call("state", {})).asks ?? [];
  expect(asks.length).toBe(1);
  await call("answer", { id: asks[0].id, content: { question_0: "In-process LRU" } });
  // The turn carries on and ends: its step no longer waits on anyone.
  await expect.poll(async () => (await cards(page)).find((c) => c.text === "Picking a cache")?.column, { timeout: 30_000 }).toBe("in_progress");
});
