// M78 (#400) and M79 (#401): a task for another machine's agent, over A2A
// through a local control, and its owner's consent. Alice founded Acme and
// has its team box; Bob is an owner of Acme too, on his own machine. The
// team box offers `fixer` (a recipe in a git project); Claude Code's adapter
// is the fake ACP agent (playwright.config.ts), which writes a file when
// told to, so a task has a patch.
//
// Bob's machine sends fixer a task: it waits for the team box's own account
// (Alice) on a card. Bob, though an owner of the team (so `Owner` on the
// box), can't answer it: answers are checked by account. Alice allows it for
// an hour; it runs in a worktree, and Bob's machine gets the reply and the
// patch. The next task goes through on the grant; once Alice revokes it the
// card is back, and Bob's cancel ends the task there.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const t = new TeamControl("delegate");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let block = 0;
/** The project fixer works in, and the first task that changed it. */
let project = "";
let firstTask = "";

interface Task {
  id: string;
  status: { state: string; message?: { parts: { text: string }[] } };
  artifacts: { name: string; parts: { text: string }[]; metadata?: { arugula?: { stat?: string } } }[];
}

/** From Bob's machine: a `delegate` request, through its own API. */
async function delegate(body: Record<string, unknown>): Promise<{ status: number; task: Task; summary: string }> {
  await show(bob, "bobs");
  const r = await call(bob, "POST", "/api/a2a/delegate", { machine: "teambox", agent: "fixer", ...body });
  return { status: r.status, task: (r.body as { task: Task }).task, summary: (r.body as { summary: string }).summary ?? JSON.stringify(r.body) };
}

/** The team box's Team agents block, which holds the cards (opened in
 * session `a2a` for the first task that needed one). */
async function cardBlock(): Promise<number> {
  await show(alice, "teambox");
  await expect
    .poll(() => alice.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "agents")?.id ?? 0), { timeout: 30_000 })
    .not.toBe(0);
  return alice.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "agents")!.id);
}

const waiting = async () => (await call(alice, "GET", "/api/a2a/waiting")).body as unknown as { waiting: Task[] };

test("Acme's team box offers fixer; Bob is an owner of Acme too", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start();
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "owner", true), team);
  bob = await t.person(browser, "bob");
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  await t.addMachine(alice, "teambox", team);
  await t.addMachine(bob, "bobs");
  for (const p of [alice, bob]) await home(p);
  await expect.poll(() => live(alice, ["teambox"]), { timeout: 30_000 }).toBe(true);
  await expect.poll(() => live(bob, ["teambox", "bobs"]), { timeout: 30_000 }).toBe(true);

  // A git project with the recipe.
  const dir = t.temp("calc");
  mkdirSync(join(dir, ".claude/agents"), { recursive: true });
  writeFileSync(join(dir, ".claude/agents/fixer.md"), "---\nname: fixer\ndescription: Fixes things\n---\nYou fix things.\n");
  writeFileSync(join(dir, "calc.py"), "print(1)\n");
  const git = (...args: string[]) => execFileSync("git", ["-C", dir, "-c", "user.name=t", "-c", "user.email=t@e", ...args]);
  git("init", "-q");
  git("add", "-A");
  git("commit", "-q", "-m", "calc");
  project = dir;
  await show(alice, "teambox");
  const r = await call(alice, "POST", "/api/a2a/offers", { agent: "fixer", dir });
  expect(r.status, JSON.stringify(r.body)).toBe(200);

  // Bob's machine reaches teambox only once it has Acme's roster, which
  // comes on its next refresh from control (#711): wait until its catalog
  // (`list kind agents`) names fixer there.
  await show(bob, "bobs");
  await expect
    .poll(
      async () => {
        const c = (await call(bob, "GET", "/api/a2a/catalog?fresh=1")).body as unknown as {
          machines?: { name: string; agents: { name: string }[] }[];
        };
        return c.machines?.find((m) => m.name === "teambox")?.agents.map((a) => a.name) ?? [];
      },
      { timeout: 90_000, intervals: [1_000, 2_000, 5_000] },
    )
    .toContain("fixer");
});

test("a task from Bob's machine waits for the box's own account: not Bob's, though he's an owner of the team", async () => {
  test.setTimeout(120_000);
  const sent = await delegate({ kind: "send", text: "write notes.txt hello from bob", wait: 0 });
  expect(sent.status, sent.summary).toBe(200);
  expect(sent.task.status.state).toBe("TASK_STATE_SUBMITTED");
  block = await cardBlock();
  await expect.poll(async () => (await waiting()).waiting.map((w) => w.id)).toEqual([sent.task.id]);
  // The card on the block says who asks, for which agent, what.
  await alice.evaluate((b) => window.__arugula.client.setActive(b), block);
  const card = alice.locator(`[data-pane="${block}"] .pane-ask`);
  await expect(card).toContainText("bob", { timeout: 20_000 });
  await expect(card).toContainText("fixer");
  await expect(card).toContainText("hello from bob");

  // Bob is `Owner` on the team box, yet another account: refused, every way.
  await show(bob, "teambox");
  const as_bob = await call(bob, "POST", `/api/blocks/${block}/call/answer`, { content: { answer: "hour" } });
  expect(as_bob.status).toBe(403);
  expect((await call(bob, "POST", `/api/a2a/tasks/${sent.task.id}/consent`, { answer: "hour" })).status).toBe(403);
  expect((await call(bob, "GET", "/api/a2a/waiting")).status).toBe(403);
  await show(alice, "teambox");
  expect((await waiting()).waiting).toHaveLength(1);

  // Alice allows it for an hour: it runs, and Bob's machine gets the reply
  // and the patch.
  const yes = await call(alice, "POST", `/api/blocks/${block}/call/answer`, { content: { answer: "hour" } });
  expect(yes.status, JSON.stringify(yes.body)).toBe(200);
  const done = await delegate({ kind: "get", task: sent.task.id, wait: 60 });
  expect(done.task.status.state, done.summary).toBe("TASK_STATE_COMPLETED");
  expect(done.summary).toContain("Wrote notes.txt.");
  firstTask = sent.task.id;
  const patch = done.task.artifacts.find((a) => a.name === "patch")!;
  expect(patch.parts[0].text).toContain("+hello from bob");
  expect(patch.metadata?.arugula?.stat).toContain("notes.txt");
  // The grant is listed on the box.
  await show(alice, "teambox");
  const grants = (await call(alice, "GET", "/api/a2a/grants")).body as unknown as { name: string; agent: string }[];
  expect(grants.map((g) => `${g.name}/${g.agent}`)).toEqual(["bob/fixer"]);
});

test("Bob reviews the work in a diff block on a copy of his checkout, then applies it (M80)", async () => {
  test.setTimeout(120_000);
  const mine = t.temp("bobs-clone");
  execFileSync("git", ["clone", "-q", project, mine]);
  await show(bob, "bobs");
  const before = await bob.evaluate(() => window.__arugula.client.state!.panes.length);
  const review = await call(bob, "POST", "/api/a2a/delegate", { kind: "review", machine: "", agent: "", task: firstTask, dir: mine });
  expect(review.status, JSON.stringify(review.body)).toBe(200);
  const body = review.body as unknown as { block: number; applied: { clean: boolean; files: string[] }; summary: string };
  expect(body.applied.clean, body.summary).toBe(true);
  expect(body.applied.files).toEqual(["notes.txt"]);
  // The diff block shows it; the checkout itself hasn't changed.
  await expect.poll(() => bob.evaluate(() => window.__arugula.client.state!.panes.length)).toBe(before + 1);
  const diff = (await call(bob, "GET", `/api/blocks/${body.block}`)).body as { state?: { files?: { path: string }[] } } | null;
  await expect
    .poll(async () => ((await call(bob, "GET", `/api/blocks/${body.block}`)).body as typeof diff)?.state?.files?.map((f) => f.path))
    .toEqual(["notes.txt"]);
  expect(existsSync(join(mine, "notes.txt"))).toBe(false);

  const applied = await call(bob, "POST", "/api/a2a/delegate", { kind: "apply", machine: "", agent: "", task: firstTask, dir: mine });
  expect(applied.status, JSON.stringify(applied.body)).toBe(200);
  expect((applied.body as unknown as { applied: { clean: boolean } }).applied.clean).toBe(true);
  expect(readFileSync(join(mine, "notes.txt"), "utf8")).toBe("hello from bob\n");
});

test("the grant lets the next one through; revoked, the card is back; Bob's cancel ends it", async () => {
  test.setTimeout(120_000);
  const second = await delegate({ kind: "send", text: "write more.txt again", wait: 60 });
  expect(second.task.status.state, second.summary).toBe("TASK_STATE_COMPLETED");

  await show(alice, "teambox");
  const bobs = ((await call(alice, "GET", "/api/a2a/grants")).body as unknown as { account: string }[])[0].account;
  expect((await call(alice, "POST", `/api/blocks/${block}/call/revoke`, { account: bobs, agent: "fixer" })).status).toBe(200);
  const third = await delegate({ kind: "send", text: "write third.txt once more", wait: 0 });
  expect(third.task.status.state).toBe("TASK_STATE_SUBMITTED");
  await show(alice, "teambox");
  await expect.poll(async () => (await waiting()).waiting.map((w) => w.id)).toEqual([third.task.id]);

  const canceled = await delegate({ kind: "cancel", task: third.task.id });
  expect(canceled.task.status.state, canceled.summary).toBe("TASK_STATE_CANCELED");
  await show(alice, "teambox");
  await expect.poll(async () => (await waiting()).waiting).toEqual([]);
});

test("a task waiting on the owner is still waiting after the box restarts, and runs once allowed", async () => {
  test.setTimeout(180_000);
  const sent = await delegate({ kind: "send", text: "write after.txt across a restart", wait: 0 });
  expect(sent.task.status.state).toBe("TASK_STATE_SUBMITTED");
  await show(alice, "teambox");
  await expect.poll(async () => (await waiting()).waiting.map((w) => w.id)).toEqual([sent.task.id]);
  await t.restartMachine("teambox");
  await home(alice);
  await expect.poll(() => live(alice, ["teambox"]), { timeout: 60_000 }).toBe(true);
  await show(alice, "teambox");
  await expect.poll(async () => (await waiting())?.waiting?.map((w) => w.id), { timeout: 30_000 }).toEqual([sent.task.id]);
  // Its card is up again, on the block that holds them.
  const holder = await cardBlock();
  await expect
    .poll(() => alice.evaluate((b) => window.__arugula.client.state!.panes.find((p) => p.id === b)?.ask?.id ?? null, holder), { timeout: 30_000 })
    .toBe(sent.task.id);
  expect((await call(alice, "POST", `/api/blocks/${holder}/call/answer`, { content: { answer: "once" } })).status).toBe(200);
  const done = await delegate({ kind: "get", task: sent.task.id, wait: 60 });
  expect(done.task.status.state, done.summary).toBe("TASK_STATE_COMPLETED");
  expect(done.task.artifacts.find((a) => a.name === "patch")!.parts[0].text).toContain("+across a restart");
});

