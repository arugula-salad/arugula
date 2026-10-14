// What a conversation's links do, in its timeline (#857), on a local
// control with Alice and Bob on a team, every daemon reached through the
// relay: Alice's alices (the conversation's home) and alices-2, and Bob's
// bobs. Bob is on a phone.
//
// The conversation links a pane on each machine. Claude Code is played by
// its hooks, as `arugula hook` and `arugula attention` call them: a turn
// starts (working), asks a permission (the card waits on the pane's
// machine), and ends with its last message. On alices-2 the ask shows up
// at home as a card; Bob answers it from his phone, the pane goes on, and
// the turn's end comes with its summary and "1 approval". Claude Code's
// "waiting for your input" a minute later posts nothing. On Bob's own
// machine (another account) Alice answers from her home, and bobs records
// it as hers "via conversation". An answer names its ask: one for an ask
// that was answered doesn't land on the next. The watchers come back with
// the home's daemon.
//
// #887: a card whose ask was answered at the pane stops offering Allow
// once the pane's machine refuses it (409). Claude Code's Write (played by
// `fake_claude.py` in L2's pane, arugulad being its IDE) asks, then waits
// as a diff: the ask's card is retired by the diff's, which Bob accepts
// from his phone, and the file is written.

import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const t = new TeamControl("events");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let conv = "";
let bobAccount = "";
/** Each link's pane, by name. */
const panes: Record<string, number> = {};

const deviceOf = (page: Page, host: string) => page.evaluate((h) => window.__arugula.hosts.find(h)?.id ?? "", host);
const firstPane = async (page: Page) => ((await call(page, "GET", "/api/panes")).body as unknown as { id: number }[])[0].id;

/** A request to `host` from `page`, whichever host it shows. */
const on = (page: Page, host: string, method: string, path: string, body?: unknown) =>
  page.evaluate(
    async ([h, m, p, b]) => {
      const r = await window.__arugula.fleet.request(h as string, m as string, p as string, b);
      return { status: r.status, body: await r.json<Record<string, unknown>>().catch(() => null) };
    },
    [host, method, path, body] as const,
  );

/** Claude Code's hooks in `pane` on `host`, as `page` (who may drive it). */
const hook = {
  state: (page: Page, host: string, pane: number, state: string, why?: string) =>
    on(page, host, "POST", `/api/panes/${pane}/attention`, { state, why: why ?? null }),
  /** `arugula hook` on PermissionRequest: it waits for an answer, kept as
   * `window.__permits[key]`. */
  permit: (page: Page, host: string, pane: number, key: string, command: string) =>
    page.evaluate(
      ([h, p, k, c]) => {
        const w = window as unknown as { __permits: Record<string, Promise<unknown>> };
        w.__permits ??= {};
        w.__permits[k as string] = window.__arugula.fleet
          .request(h as string, "POST", `/api/panes/${p}/permit`, {
            tool_name: "Bash",
            tool_input: { command: c },
            session_id: "s-e2e",
            hook_event_name: "PermissionRequest",
          })
          .then((r) => r.json());
      },
      [host, pane, key, command] as const,
    ),
  answered: (page: Page, key: string) =>
    page.evaluate((k) => (window as unknown as { __permits: Record<string, Promise<{ action: string }>> }).__permits[k], key),
  /** `arugula ask` on AskUserQuestion (#859): it waits for the answer,
   * kept as `window.__permits[key]` too. */
  ask: (page: Page, host: string, pane: number, key: string, questions: unknown) =>
    page.evaluate(
      ([h, p, k, q]) => {
        const w = window as unknown as { __permits: Record<string, Promise<unknown>> };
        w.__permits ??= {};
        w.__permits[k as string] = window.__arugula.fleet
          .request(h as string, "POST", `/api/panes/${p}/ask`, { questions: q, id: k })
          .then((r) => r.json());
      },
      [host, pane, key, questions] as const,
    ),
};

/** Claude's AskUserQuestion input: a single pick and a multi-select. */
const QUESTIONS = [
  {
    question: "Which test runner?",
    header: "Runner",
    multiSelect: false,
    options: [
      { label: "pytest", description: "What the repo uses" },
      { label: "unittest", description: "The standard library's" },
    ],
  },
  {
    question: "Which suites?",
    header: "Suites",
    multiSelect: true,
    options: [{ label: "unit" }, { label: "integration" }, { label: "e2e" }],
  },
];

/** The conversation's events, as `link:kind` (each card with its state). */
async function events(page: Page): Promise<string[]> {
  const r = await on(page, "alices", "GET", `/api/threads/conv-${conv}`);
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const msgs = (r.body as unknown as { messages: { event?: { kind: string; link: string } }[] }).messages;
  return msgs.filter((m) => m.event).map((m) => `${m.event!.link}:${m.event!.kind}`);
}

async function openConv(page: Page) {
  await page.evaluate((c) => (location.hash = `#chat=alices/conv-${c}`), conv);
  await expect(page.locator("[data-chat]")).toBeVisible({ timeout: 15_000 });
}

test("a conversation on alices links a pane on each machine", async ({ browser }) => {
  test.setTimeout(240_000);
  await t.start();
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const invite = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
  bob = await t.person(browser, "bob", { viewport: { width: 390, height: 760 }, isMobile: true, hasTouch: true });
  await bob.goto(invite);
  await bob.locator("[data-accept-invite]").click();
  await expect(bob.locator("[data-invite-pending]")).toBeVisible();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await alice.locator("[data-admit-yes]").click({ timeout: 15_000 });
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  bobAccount = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "bob")!.account);
  await t.addMachine(alice, "alices");
  await t.addMachine(alice, "alices-2");
  await t.addMachine(bob, "bobs");
  for (const p of [alice, bob]) await home(p);
  await expect.poll(() => live(alice, ["alices", "alices-2"]), { timeout: 30_000 }).toBe(true);
  await expect.poll(() => live(bob, ["bobs"]), { timeout: 30_000 }).toBe(true);
  await show(bob, "bobs");
  await show(alice, "alices-2");
  await show(alice, "alices");

  const homeId = await deviceOf(alice, "alices");
  const c = await call(alice, "POST", "/api/convs", { title: "calc tests" });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  conv = (c.body as { id: string }).id;
  const grant = { device: homeId, device_name: "alices", conv, conv_title: "calc tests" };
  // L1 here, L2 on alices-2.
  panes.L1 = await firstPane(alice);
  expect((await call(alice, "POST", `/api/panes/${panes.L1}/conv-grants`, grant)).status).toBe(200);
  expect((await call(alice, "POST", `/api/convs/${conv}/links`, { pane: panes.L1, title: "claude" })).status).toBe(200);
  await show(alice, "alices-2");
  panes.L2 = await firstPane(alice);
  const g2 = await call(alice, "POST", `/api/panes/${panes.L2}/conv-grants`, grant);
  expect(g2.status, JSON.stringify(g2.body)).toBe(200);
  await show(alice, "alices");
  const otherId = await deviceOf(alice, "alices-2");
  const l2 = await call(alice, "POST", `/api/convs/${conv}/links`, { machine: otherId, pane: panes.L2, title: "claude" });
  expect(l2.status, JSON.stringify(l2.body)).toBe(200);

  // Bob is a member; L3 is his pane on bobs.
  const add = await call(alice, "POST", `/api/convs/${conv}/members`, { who: `account:${bobAccount}`, history: true, name: "bob" });
  expect(add.status, JSON.stringify(add.body)).toBe(200);
  await home(bob);
  await expect.poll(() => bob.evaluate(() => window.__arugula.hosts.names), { timeout: 60_000, intervals: [2_000] }).toContain("alices");
  await show(bob, "bobs");
  panes.L3 = await firstPane(bob);
  const g3 = await call(bob, "POST", `/api/panes/${panes.L3}/conv-grants`, { ...grant, device: await deviceOf(bob, "alices") });
  expect(g3.status, JSON.stringify(g3.body)).toBe(200);
  const bobsId = await deviceOf(bob, "bobs");
  await expect
    .poll(
      async () => {
        const r = await on(bob, "alices", "POST", `/api/convs/${conv}/links`, { machine: bobsId, pane: panes.L3, title: "claude" });
        return r.status === 200 ? 200 : JSON.stringify(r.body);
      },
      { timeout: 60_000, intervals: [3_000] },
    )
    .toBe(200);
  // Nothing has happened in any of them yet.
  expect(await events(alice)).toEqual([]);
});

test("a permission on alices-2 is a card at home; Bob allows it from his phone and the pane goes on", async () => {
  test.setTimeout(120_000);
  // (A hook's request through the page times out in 30s: open the
  // conversation first.)
  await openConv(bob);
  await hook.state(alice, "alices-2", panes.L2, "working");
  await hook.permit(alice, "alices-2", panes.L2, "l2", "python3 -m pytest -q");
  const card = bob.locator('[data-chat] [data-conv-card][data-event-link="L2"]');
  await expect(card).toBeVisible({ timeout: 30_000 });
  await expect(card).toContainText("Bash: python3 -m pytest -q");
  await expect(card).toHaveAttribute("data-card-state", "open");
  await card.locator("[data-card-allow]").tap();
  // The pane's hook hears "allow": the remote pane goes on.
  expect(await hook.answered(alice, "l2")).toMatchObject({ action: "allow" });
  await expect(card).toHaveAttribute("data-card-state", "answered");
  await expect(card.locator("[data-card-done]")).toContainText("allowed by bob");

  // Its turn ends, with its last message and the approval.
  await hook.state(alice, "alices-2", panes.L2, "working");
  await hook.state(alice, "alices-2", panes.L2, "done", "Fixed mean(): it divided by len - 1. 4 passed.");
  const ended = bob.locator('[data-chat] [data-conv-event="turn_ended"][data-event-link="L2"]');
  await expect(ended).toBeVisible({ timeout: 30_000 });
  await expect(ended.locator("[data-event-summary]")).toHaveText("Fixed mean(): it divided by len - 1. 4 passed.");
  await expect(ended).toContainText("1 approval");
  expect(await events(alice)).toEqual(["L2:needs_input", "L2:answered", "L2:turn_ended"]);

  // Claude Code's "waiting for your input", a minute later: no line.
  await hook.state(alice, "alices-2", panes.L2, "needs_input", "Claude is waiting for your input");
  await bob.waitForTimeout(4_000);
  expect(await events(alice)).toEqual(["L2:needs_input", "L2:answered", "L2:turn_ended"]);
});

test("a question on alices-2 is a card with its options; Bob answers it from his phone (#859)", async () => {
  test.setTimeout(120_000);
  await hook.state(alice, "alices-2", panes.L2, "working");
  await hook.ask(alice, "alices-2", panes.L2, "q1", QUESTIONS);
  const card = bob.locator('[data-chat] [data-conv-card][data-event-link="L2"][data-card-state="open"]');
  await expect(card).toBeVisible({ timeout: 30_000 });
  // Its options, as beside the pane: not only Skip.
  await expect(card.locator(".ask-question")).toHaveCount(2);
  await expect(card.locator('[data-question="0"] [role="radio"]')).toHaveText([/pytest/, /unittest/]);
  await expect(card.locator('[data-question="1"] input[type="checkbox"]')).toHaveCount(3);
  await expect(card.locator("[data-ask-submit]")).toBeDisabled();
  await card.locator('[data-question="0"] [role="radio"]', { hasText: "pytest" }).tap();
  await card.locator('[data-question="1"] .ask-option', { hasText: "unit" }).first().tap();
  await card.locator('[data-question="1"] .ask-option', { hasText: "e2e" }).tap();
  await card.locator('[data-question="1"] .ask-other').fill("doctests");
  await card.locator("[data-ask-submit]").tap();
  // The hook gets Claude Code's answers, as it does from the pane's card.
  const got = (await hook.answered(alice, "q1")) as unknown as { action: string; output: { hookSpecificOutput: { updatedInput: { answers: Record<string, string> } } } };
  expect(got.action).toBe("accept");
  expect(got.output.hookSpecificOutput.updatedInput.answers).toEqual({
    "Which test runner?": "pytest",
    "Which suites?": "unit, e2e, doctests",
  });
  const done = bob.locator('[data-chat] [data-conv-card="q1"]');
  await expect(done).toHaveAttribute("data-card-state", "answered");
  await expect(done.locator("[data-card-done]")).toContainText("answered by bob: Which suites? → unit, e2e, doctests; Which test runner? → pytest");
  await expect(bob.locator('[data-chat] [data-conv-event="answered"][data-event-link="L2"] [data-event-answer]').last()).toContainText("Which test runner? → pytest");
  await hook.state(alice, "alices-2", panes.L2, "done", "Ran pytest on unit and e2e.");
  await expect(bob.locator('[data-chat] [data-conv-event="turn_ended"][data-event-link="L2"]')).toHaveCount(2, { timeout: 30_000 });

  // A question skipped from the card: the hook hears it declined.
  await hook.state(alice, "alices-2", panes.L2, "working");
  await hook.ask(alice, "alices-2", panes.L2, "q2", [QUESTIONS[0]]);
  const next = bob.locator('[data-chat] [data-conv-card="q2"]');
  await expect(next).toHaveAttribute("data-card-state", "open", { timeout: 30_000 });
  await next.locator("[data-ask-decline]").tap();
  expect(await hook.answered(alice, "q2")).toMatchObject({ action: "decline" });
  await expect(next.locator("[data-card-done]")).toContainText("skipped by bob");
  await hook.state(alice, "alices-2", panes.L2, "done", "Skipped.");
});

test("a teammate's pane: Alice answers its card at home, and bobs records it as hers via the conversation", async () => {
  test.setTimeout(90_000);
  await show(alice, "alices");
  await alice.locator("[data-open-chat]").click();
  await alice.locator(`[data-chat-channels] [data-chat-thread="conv-${conv}"]`).click();
  await hook.state(bob, "bobs", panes.L3, "working");
  await hook.permit(bob, "bobs", panes.L3, "l3", "pytest tests/test_calc.py");
  const card = alice.locator('[data-chat] [data-conv-card][data-event-link="L3"]');
  await expect(card).toBeVisible({ timeout: 30_000 });
  await card.locator("[data-card-allow]").click();
  expect(await hook.answered(bob, "l3")).toMatchObject({ action: "allow" });
  await expect(card).toHaveAttribute("data-card-state", "answered");
  const said = await on(bob, "bobs", "GET", `/api/history?pane=${panes.L3}&kind=answer`);
  expect(JSON.stringify(said.body)).toContain("via conversation “calc tests”");
});

test("an answer names its ask: an old one doesn't land on the next prompt", async () => {
  test.setTimeout(60_000);
  // L1, here: two asks in a row; the first was answered at the pane.
  await hook.state(alice, "alices", panes.L1, "working");
  await hook.permit(alice, "alices", panes.L1, "first", "rm -rf build");
  await expect.poll(() => events(alice), { timeout: 15_000 }).toContain("L1:needs_input");
  const first = await on(alice, "alices", "GET", `/api/threads/conv-${conv}`);
  const msgs = (first.body as unknown as { messages: { event?: { link: string; ask?: { id: string } } }[] }).messages;
  const ask = msgs.filter((m) => m.event?.link === "L1").at(-1)!.event!.ask!.id;
  const deny = await on(alice, "alices", "POST", "/api/attention/act", { action: "deny", pane: panes.L1, id: ask });
  expect(deny.status, JSON.stringify(deny.body)).toBe(200);
  expect(await hook.answered(alice, "first")).toMatchObject({ action: "deny" });
  await hook.permit(alice, "alices", panes.L1, "second", "make test");
  await expect.poll(async () => (await events(alice)).filter((e) => e === "L1:needs_input").length, { timeout: 15_000 }).toBe(2);
  // The first card's answer, late: refused, and the second still waits.
  const late = await on(alice, "alices", "POST", `/api/convs/${conv}/act`, { link: "L1", id: ask, action: "allow" });
  expect(late.status, JSON.stringify(late.body)).toBe(409);
  expect(JSON.stringify(late.body)).toContain("asks something else");
  const cards = alice.locator('[data-chat] [data-conv-card][data-event-link="L1"]');
  await expect(cards).toHaveCount(2);
  await expect(cards.first()).toHaveAttribute("data-card-state", "moved");
  await cards.last().locator("[data-card-deny]").click();
  expect(await hook.answered(alice, "second")).toMatchObject({ action: "deny" });
});

/** The newest card of `link` in the conversation: its ask's id. */
async function newestCard(link: string): Promise<string> {
  const r = await on(alice, "alices", "GET", `/api/threads/conv-${conv}`);
  const msgs = (r.body as unknown as { messages: { event?: { kind: string; link: string; ask?: { id: string } } }[] }).messages;
  return msgs.filter((m) => m.event?.kind === "needs_input" && m.event.link === link).at(-1)?.event?.ask?.id ?? "";
}

test("a card whose ask was answered at the pane stops offering Allow (#887)", async () => {
  test.setTimeout(60_000);
  await openConv(bob);
  await hook.state(alice, "alices-2", panes.L2, "working");
  await hook.permit(alice, "alices-2", panes.L2, "stale", "python3 -m pytest -x");
  const card = bob.locator('[data-chat] [data-conv-card][data-event-link="L2"][data-card-state="open"]');
  await expect(card).toBeVisible({ timeout: 30_000 });
  const id = await newestCard("L2");
  // Answered at the pane, and nothing posted after it: the card can't know.
  const at = await on(alice, "alices-2", "POST", "/api/attention/act", { action: "allow", pane: panes.L2, id });
  expect(at.status, JSON.stringify(at.body)).toBe(200);
  expect(await hook.answered(alice, "stale")).toMatchObject({ action: "allow" });
  const before = await events(alice);
  // Bob's Allow is refused: the card says so and offers nothing more.
  const stale = bob.locator(`[data-chat] [data-conv-card="${id}"]`);
  await stale.locator("[data-card-allow]").tap();
  await expect(stale).toHaveAttribute("data-card-state", "moved");
  await expect(stale.locator("[data-card-allow]")).toHaveCount(0);
  await expect(stale.locator("[data-card-done]")).toHaveText("Answered elsewhere");
  await expect(stale.locator(".thread-error")).toHaveCount(0);
  expect(await events(alice)).toEqual(before);
  await hook.state(alice, "alices-2", panes.L2, "done", "Ran it.");
});

test("an edit waiting as a diff on alices-2 is a card; Bob accepts it from his phone (#887)", async () => {
  test.setTimeout(120_000);
  // Claude Code in L2's pane, with arugulad as its IDE.
  const fake = resolve("../crates/daemon/tests/fake_claude.py");
  const send = (text: string) => on(alice, "alices-2", "POST", `/api/panes/${panes.L2}/send`, { text, enter: true });
  const screen = () =>
    alice.evaluate(
      async ([p]) => (await window.__arugula.fleet.request("alices-2", "GET", `/api/panes/${p}/capture?format=text`)).text(),
      [panes.L2] as const,
    );
  expect((await send(`python3 ${fake}`)).status).toBe(200);
  await expect.poll(screen, { timeout: 20_000 }).toContain("connected");
  const dir = mkdtempSync(join(tmpdir(), "arugula-e2e-887-"));
  const file = join(dir, "calc.py");
  writeFileSync(file, "def mean(xs):\n    return sum(xs) / (len(xs) - 1)\n");

  // The Write asks first: a card, after the hold.
  await hook.state(alice, "alices-2", panes.L2, "working");
  await hook.permit(alice, "alices-2", panes.L2, "write", `Write ${file}`);
  const asked = bob.locator('[data-chat] [data-conv-card][data-event-link="L2"][data-card-state="open"]');
  await expect(asked).toBeVisible({ timeout: 30_000 });
  const askId = await newestCard("L2");
  // Then it waits as a diff: the diff's card comes at once, and the ask's
  // card is retired.
  expect((await send(`edit ${file} def mean(xs):\\n    return sum(xs) / len(xs)\\n`)).status).toBe(200);
  const card = bob.locator('[data-chat] [data-conv-card][data-card-diff][data-event-link="L2"]');
  await expect(card).toHaveAttribute("data-card-state", "open", { timeout: 30_000 });
  await expect(bob.locator(`[data-chat] [data-conv-card="${askId}"]`)).toHaveAttribute("data-card-state", "moved");
  await expect(card.locator(".conv-card-head")).toContainText("Wants to edit");
  await expect(card.locator(".conv-card-file")).toContainText("calc.py");
  await expect(card.locator(".diff-text .add")).toContainText("return sum(xs) / len(xs)");
  await expect(card.locator(".diff-text .del")).toContainText("(len(xs) - 1)");
  await card.locator("[data-card-accept]").tap();
  // Claude Code writes it, and the conversation says who accepted it.
  await expect.poll(screen, { timeout: 15_000 }).toContain("result FILE_SAVED");
  await expect.poll(() => readFileSync(file, "utf8"), { timeout: 15_000 }).toContain("return sum(xs) / len(xs)\n");
  await expect(card).toHaveAttribute("data-card-state", "answered");
  await expect(card.locator("[data-card-done]")).toContainText("accepted by bob");
  const line = bob.locator('[data-chat] [data-conv-event="answered"][data-event-link="L2"]').last();
  await expect(line).toContainText("accepted L2:");
  // (Outside the pane's directory, the file is named in full.)
  await expect(line).toContainText(`edit ${file} (+1 −1)`);
  // alices-2 records it as Bob's, via the conversation.
  await expect
    .poll(async () => JSON.stringify((await on(alice, "alices-2", "GET", `/api/history?pane=${panes.L2}&kind=answer`)).body))
    .toContain("via conversation “calc tests”");

  // The tool ran: its hook's ask closes, and the turn ends.
  const post = { hook_event_name: "PostToolUse", session_id: "s-e2e", tool_name: "Bash", tool_input: { command: `Write ${file}` } };
  expect((await on(alice, "alices-2", "POST", `/api/panes/${panes.L2}/hook`, post)).status).toBe(200);
  await hook.answered(alice, "write");
  await hook.state(alice, "alices-2", panes.L2, "done", "Fixed mean().");
  await send("quit");
});

test("the home's watchers come back with its daemon", async () => {
  test.setTimeout(120_000);
  await t.restartMachine("alices");
  await home(alice);
  await show(alice, "alices");
  await expect.poll(() => live(alice, ["alices", "alices-2"]), { timeout: 30_000 }).toBe(true);
  const before = (await events(alice)).length;
  await hook.state(alice, "alices-2", panes.L2, "working");
  await expect
    .poll(
      async () => {
        // Each try is a turn of its own: the follow may not be up yet.
        await hook.state(alice, "alices-2", panes.L2, "done", "Back after the restart.");
        await hook.state(alice, "alices-2", panes.L2, "working");
        return (await events(alice)).slice(before);
      },
      { timeout: 60_000, intervals: [3_000] },
    )
    .toContain("L2:turn_ended");
});
