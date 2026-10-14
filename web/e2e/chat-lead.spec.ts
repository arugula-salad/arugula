// A conversation's lead (#858), on a local control with Alice's alices (the
// conversation's home) and alices-2, reached from each other only through
// the relay. The conversation links an agent on each machine, both the fake
// ACP agent standing in for Claude Code; so is the lead, whose turns are
// scripted by the @lead messages (fake_acp.py: steps after the words,
// separated by " ;; ", each an MCP call or a line to say).
//
// #858's done-when: "@lead have L1 and L2 run their tests and fix what
// fails" in a conversation linking a pane on each of two machines:
// - the lead prompts both and waits;
// - the cards are answered by people in the conversation;
// - the lead reports;
// - the home daemon restarts mid-run without losing the turn's events;
// - the lead can't reach an unlinked pane, even by number.
// And: its helpers (start_agent) are links, and a lost lead is replaced,
// the conversation keeping its id and timeline.

import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, live, show } from "./team-fixture";

const t = new TeamControl("lead");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let conv = "";
let lead = 0;
/** Each link's pane, by name, and the unlinked pane on alices-2. */
const panes: Record<string, number> = {};
let unlinked = 0;
/** L1's folder, on this host. */
let l1dir = "";

type Msg = { id: number; who: string; name: string; text: string; event?: { kind: string; link: string; ask?: { id: string } } };

/** A request to `host` from Alice's page, whichever host it shows. */
const on = (host: string, method: string, path: string, body?: unknown) =>
  alice.evaluate(
    async ([h, m, p, b]) => {
      const r = await window.__arugula.fleet.request(h as string, m as string, p as string, b);
      const text = await r.text();
      let json: unknown = null;
      try {
        json = JSON.parse(text);
      } catch {
        json = text;
      }
      return { status: r.status, body: json as Record<string, unknown> };
    },
    [host, method, path, body] as const,
  );

const deviceOf = (host: string) => alice.evaluate((h) => window.__arugula.hosts.find(h)?.id ?? "", host);

async function messages(): Promise<Msg[]> {
  const r = await on("alices", "GET", `/api/threads/conv-${conv}`);
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return (r.body as unknown as { messages: Msg[] }).messages;
}

/** The lead's replies, newest last. */
const replies = async () => (await messages()).filter((m) => m.who === "lead").map((m) => m.text);

type Lead = { block: number; cursor: number; asked?: string[]; awaiting?: string[] };
type Rec = { id: string; lead?: Lead; links: { name: string; machine?: string; pane: number; title: string }[] };

/** The conversation's record, as alices's page has it. */
const record = (): Promise<Rec> => alice.evaluate((id) => window.__arugula.client.conv(id) as unknown as Rec, conv);

const say = (text: string) => on("alices", "POST", `/api/threads/conv-${conv}`, { text });

/** An agent block (the fake Claude Code) on `host`, working in `cwd`. */
async function agent(host: string, cwd: string): Promise<number> {
  const r = await on(host, "POST", "/api/blocks", { type: "agent", config: { agent: "claude", cwd } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return (r.body as { block: number }).block;
}

/** What the lead was last prompted with: its block's transcript. */
const transcript = async (block: number) => String((await on("alices", "GET", `/api/panes/${block}/capture`)).body);

async function openConv() {
  await alice.evaluate((c) => (location.hash = `#chat=alices/conv-${c}`), conv);
  await expect(alice.locator("[data-chat]")).toBeVisible({ timeout: 15_000 });
}

test("a new conversation on alices has its lead, and links an agent on each machine", async ({ browser }) => {
  test.setTimeout(240_000);
  await t.start();
  alice = await t.person(browser, "alice");
  await t.addMachine(alice, "alices");
  await t.addMachine(alice, "alices-2");
  await home(alice);
  await expect.poll(() => live(alice, ["alices", "alices-2"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "alices-2");
  await show(alice, "alices");

  const c = await on("alices", "POST", "/api/convs", { title: "calc tests" });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  conv = (c.body as { id: string }).id;
  // Started with it: an agent block here.
  lead = (c.body as { lead?: { block: number } }).lead?.block ?? 0;
  expect(lead).toBeGreaterThan(0);

  const homeId = await deviceOf("alices");
  const grant = { device: homeId, device_name: "alices", conv, conv_title: "calc tests" };
  l1dir = mkdtempSync(join(tmpdir(), "arugula-e2e-lead-l1-"));
  panes.L1 = await agent("alices", l1dir);
  expect((await on("alices", "POST", `/api/panes/${panes.L1}/conv-grants`, grant)).status).toBe(200);
  const l1 = await on("alices", "POST", `/api/convs/${conv}/links`, { pane: panes.L1, title: "claude" });
  expect(l1.status, JSON.stringify(l1.body)).toBe(200);

  panes.L2 = await agent("alices-2", mkdtempSync(join(tmpdir(), "arugula-e2e-lead-l2-")));
  // Another pane there, never linked: its number is the lead's to try.
  unlinked = (await on("alices-2", "POST", "/api/run", { command: "bash --norc" })).body.pane as number;
  expect((await on("alices-2", "POST", `/api/panes/${panes.L2}/conv-grants`, grant)).status).toBe(200);
  const l2 = await on("alices", "POST", `/api/convs/${conv}/links`, { machine: await deviceOf("alices-2"), pane: panes.L2, title: "claude" });
  expect(l2.status, JSON.stringify(l2.body)).toBe(200);

  await openConv();
  const chip = alice.locator("[data-chat] [data-conv-lead]");
  await expect(chip).toBeVisible();
  await expect(chip).toHaveAttribute("data-lead-state", /idle|working/, { timeout: 30_000 });
  await expect(alice.locator('[data-chat] [data-conv-link="L2"]')).toBeVisible();
});

test("@lead: it prompts both links and waits, the card is answered in the conversation, and it reports", async () => {
  test.setTimeout(180_000);
  const script = [
    "@lead have L1 and L2 run their tests and fix what fails",
    `mcp prompt_agent ${JSON.stringify({ pane: "L1", text: "write calc.py def mean(xs): return sum(xs) / len(xs)", timeout: 5 })}`,
    `mcp prompt_agent ${JSON.stringify({ pane: "L2", text: "run python3 -m pytest -q", timeout: 5 })}`,
    `mcp wait ${JSON.stringify({ pane: "L2", until: "idle", timeout: 90 })}`,
    `mcp post_thread ${JSON.stringify({ text: "L1 fixed mean() in calc.py; L2's tests pass." })}`,
  ].join(" ;; ");
  const box = alice.locator("[data-chat] .chat-thread textarea");
  await box.fill(script);
  await box.press("Enter");

  // L2 (on alices-2) asks: a card here, which Alice allows.
  const card = alice.locator('[data-chat] [data-conv-card][data-event-link="L2"]');
  await expect(card).toBeVisible({ timeout: 60_000 });
  await expect(card).toContainText("python3 -m pytest -q");
  await expect(alice.locator("[data-chat] [data-conv-lead]")).toHaveAttribute("data-lead-state", "working");
  await card.locator("[data-card-allow]").click();
  await expect(card).toHaveAttribute("data-card-state", "answered", { timeout: 30_000 });

  // The lead's report, after waiting on L2.
  await expect.poll(replies, { timeout: 90_000 }).toContain("L1 fixed mean() in calc.py; L2's tests pass.");
  await expect(alice.locator("[data-chat] .thread-msg:not(.mine)").filter({ hasText: "L1 fixed mean() in calc.py" })).toBeVisible();
  // L1 did its work, prompted by name through the lead.
  expect(readFileSync(join(l1dir, "calc.py"), "utf8")).toContain("def mean(xs)");
  // It owes Alice nothing now, and waits on no link: it saw both finish.
  await expect.poll(async () => (await record()).lead?.asked ?? [], { timeout: 15_000 }).toEqual([]);
  expect((await record()).lead?.awaiting ?? []).toEqual([]);
});

test("the lead can't reach an unlinked pane, even by number, nor a link but to prompt, wait and read", async () => {
  test.setTimeout(120_000);
  const before = (await replies()).length;
  const tries = [
    // L2's own number, which here is no link: a number reaches the lead's
    // tab only. (Here it's often the lead's own number, as in S36's first
    // run: the lead didn't start itself, so it can't drive it either.)
    `mcp prompt_agent ${JSON.stringify({ pane: panes.L2, text: "hello" })}`,
    `mcp prompt_agent ${JSON.stringify({ pane: unlinked, text: "hello" })}`,
    // L1 by number: it's here, but not in the lead's tab.
    `mcp read_output ${JSON.stringify({ pane: panes.L1 })}`,
    `mcp read_output ${JSON.stringify({ pane: "L9" })}`,
    `mcp agent_respond ${JSON.stringify({ pane: "L2", action: "allow" })}`,
    `mcp send_input ${JSON.stringify({ pane: "L1", text: "rm -rf /" })}`,
    `mcp read_thread ${JSON.stringify({ pane: panes.L1 })}`,
  ];
  expect((await say(["@lead try these", ...tries].join(" ;; "))).status).toBe(200);
  try {
    await expect.poll(async () => (await replies()).length, { timeout: 60_000 }).toBeGreaterThan(before);
  } catch (e) {
    console.log("DEBUG", JSON.stringify(panes), unlinked, lead, (await transcript(lead)).slice(-3000));
    console.log("DEBUG msgs", JSON.stringify((await messages()).map((m) => `${m.name}: ${m.text.slice(0, 200)}`), null, 1));
    throw e;
  }
  const said = (await replies()).at(-1)!;
  const lines = said.split("\n");
  expect(lines).toHaveLength(tries.length);
  // Every one refused, none answered.
  for (const l of lines) expect(l, said).toContain('"isError": true');
  expect(lines[0]).toMatch(/isn't in your tab|wasn't started by this agent/);
  expect(lines[1]).toContain("isn't in your tab");
  expect(lines[2]).toContain("isn't in your tab");
  expect(lines[3]).toContain("no link L9");
  expect(lines[4]).toContain("the people's to answer");
  expect(lines[5]).toContain("doesn't reach a link");
  expect(lines[6]).toContain("its conversation only");
});

test("the home daemon restarts while a link the lead prompted waits on its card; nothing is lost", async () => {
  test.setTimeout(240_000);
  const before = (await replies()).length;
  // The lead prompts L2 and ends its turn: L2 asks, and the lead waits on it.
  const go = `@lead check again ;; mcp prompt_agent ${JSON.stringify({ pane: "L2", text: "run make test", timeout: 5 })}`;
  expect((await say(go)).status).toBe(200);
  await expect.poll(async () => (await replies()).length, { timeout: 60_000 }).toBeGreaterThan(before);
  await expect.poll(async () => (await record()).lead?.awaiting ?? [], { timeout: 15_000 }).toEqual(["L2"]);
  const card = async () =>
    (await messages()).filter((m) => m.event?.kind === "needs_input" && m.event.link === "L2" && m.text.includes("make test")).at(-1);
  await expect.poll(card, { timeout: 30_000 }).toBeTruthy();
  const ask = (await card())!.event!.ask!.id;

  await t.restartMachine("alices");
  await home(alice);
  await show(alice, "alices");
  await expect.poll(() => live(alice, ["alices", "alices-2"]), { timeout: 60_000 }).toBe(true);
  // What it waits on is kept.
  expect((await record()).lead?.awaiting).toEqual(["L2"]);
  // The card is answered after the restart, at home.
  await expect
    .poll(async () => (await on("alices", "POST", `/api/convs/${conv}/act`, { link: "L2", id: ask, action: "allow" })).status, {
      timeout: 60_000,
      intervals: [3_000],
    })
    .toBe(200);
  // L2's turn ends; the lead is woken by it, told what happened since its
  // last turn, the card from before the restart included.
  await expect.poll(async () => (await replies()).at(-1) ?? "", { timeout: 90_000 }).toContain("Noted: L2 (you prompted it) ended its turn");
  const told = await transcript(lead);
  const wake = told.slice(told.lastIndexOf("## You"));
  expect(wake).toContain("L2 (you prompted it) ended its turn");
  expect(wake).toContain("Since your last turn:");
  expect(wake).toContain("needs you:");
  expect(wake).toContain("make test");
  expect(wake).toContain("allowed L2");
  expect(wake).toContain("turn ended");
  expect(wake).toContain("L2: claude on alices-2");
  expect((await record()).lead?.awaiting ?? []).toEqual([]);
});

test("start_agent from the lead opens beside it, as a link", async () => {
  test.setTimeout(120_000);
  await openConv();
  const before = (await replies()).length;
  const go = [
    "@lead get help",
    `mcp start_agent ${JSON.stringify({ prompt: "stream 6 300", title: "helper" })}`,
    `mcp list ${JSON.stringify({ kind: "links" })}`,
  ].join(" ;; ");
  expect((await say(go)).status).toBe(200);
  await expect.poll(async () => (await replies()).length, { timeout: 60_000 }).toBeGreaterThan(before);
  const said = (await replies()).at(-1)!;
  expect(said).toContain("linked as L3");
  expect(said).toContain("helper on alices (this machine)");
  const l3 = (await record()).links.find((l) => l.name === "L3")!;
  expect(l3.machine).toBeUndefined();
  expect(l3.title).toBe("helper");
  await expect(alice.locator('[data-chat] [data-conv-link="L3"]')).toBeVisible({ timeout: 15_000 });
  // Its turn ending wakes the lead, as any link it prompted.
  await expect.poll(async () => (await replies()).at(-1) ?? "", { timeout: 60_000 }).toContain("Noted: L3 (you prompted it) ended its turn");
});

test("a lost lead is replaced: the next @lead starts another, which has the links and the last messages", async () => {
  test.setTimeout(120_000);
  expect((await on("alices", "POST", `/api/panes/${lead}/close`)).status).toBe(200);
  await expect(alice.locator("[data-chat] [data-conv-lead]")).toHaveAttribute("data-lead-state", "gone", { timeout: 15_000 });
  const before = (await messages()).length;
  expect((await say("@lead say back again")).status).toBe(200);
  await expect.poll(async () => (await replies()).at(-1) ?? "", { timeout: 60_000 }).toBe("back again");
  const r = await record();
  expect(r.id).toBe(conv);
  expect(r.lead!.block).not.toBe(lead);
  // The same timeline, with nothing taken out.
  expect((await messages()).length).toBeGreaterThan(before);
  // Its first prompt: the last messages and the links.
  const told = await transcript(r.lead!.block);
  expect(told).toContain("Since your last turn:");
  expect(told).toContain("Noted: L3");
  expect(told).toContain("L1: claude on alices (this machine)");
  expect(told).toContain("L3: helper on alices (this machine)");
  await expect(alice.locator("[data-chat] [data-conv-lead]")).toHaveAttribute("data-lead-state", /idle|working/, { timeout: 15_000 });
});
