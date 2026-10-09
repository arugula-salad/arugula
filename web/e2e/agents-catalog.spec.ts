// M77 (#399): the team's agent catalog, on a local control with Alice and
// Bob on a team. Alice has the team's box and a machine of her own; Bob has
// one of his own. Every daemon is reached through control's relay.
//
// Recipes are offered on the team box (Alice's `fixer`) and on Bob's own
// machine (`reviewer`). Alice's own machine then reads the catalog as a
// daemon, through the relay, signed with its own key: both cards, each with
// its machine and owner. Bob's machine let Alice's daemon in only because it
// offers something (the A2A-only grant): once Bob stops offering, it's gone
// from the catalog, and Alice's daemon is refused there.

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const t = new TeamControl("agents");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;

/** A project with one recipe, `name`. */
function project(name: string, description: string): string {
  const dir = t.temp(`proj-${name}`);
  mkdirSync(join(dir, ".claude/agents"), { recursive: true });
  writeFileSync(join(dir, ".claude/agents", `${name}.md`), `---\nname: ${name}\ndescription: ${description}\nmodel: haiku\n---\nYou ${description.toLowerCase()}.\n`);
  return dir;
}

interface Shelf {
  name: string;
  owner: string;
  here: boolean;
  online: boolean;
  note: string | null;
  agents: { name: string; description: string }[];
}

/** What `host`'s catalog says, asked fresh: `machine/agent (owner)` each. */
async function catalog(page: Page, host: string): Promise<string[]> {
  await show(page, host);
  const r = await call(page, "GET", "/api/a2a/catalog?fresh=1");
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const shelves = (r.body as unknown as { machines: Shelf[] }).machines;
  return shelves.flatMap((s) => s.agents.map((a) => `${s.name}/${a.name}${s.owner ? ` (${s.owner})` : ""}`)).sort();
}

test("a team, its box, and a machine each", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start();
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
  bob = await t.person(browser, "bob");
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await expect(bob.locator("[data-invite-pending]")).toBeVisible();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  await t.addMachine(alice, "teambox", team);
  await t.addMachine(alice, "alices");
  await t.addMachine(bob, "bobs");
  await t.addMachine(bob, "bobs-other");
  for (const p of [alice, bob]) await home(p);
  await expect.poll(() => live(alice, ["teambox", "alices"]), { timeout: 30_000 }).toBe(true);
  await expect.poll(() => live(bob, ["teambox", "bobs", "bobs-other"]), { timeout: 30_000 }).toBe(true);
});

test("offered on the team box and on Bob's own machine: Alice's machine lists both", async () => {
  test.setTimeout(120_000);
  await show(alice, "teambox");
  const offered = await call(alice, "POST", "/api/a2a/offers", { agent: "fixer", dir: project("fixer", "Fix failing tests") });
  expect(offered.status, JSON.stringify(offered.body)).toBe(200);
  // Bob's machine has his teams pinned once his browser shows it (#233).
  await show(bob, "bobs");
  const r = await call(bob, "POST", "/api/a2a/offers", { agent: "reviewer", dir: project("reviewer", "Review diffs") });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  // A member who isn't the owner can't offer on the team box.
  await show(bob, "teambox");
  expect((await call(bob, "GET", "/api/a2a/offers")).status).toBe(403);

  await expect
    .poll(() => catalog(alice, "alices"), { timeout: 60_000, intervals: [2_000] })
    .toEqual(["bobs/reviewer (bob)", "teambox/fixer"]);
  // Bob's other machine offers nothing: not a row, and not let in.
  const r2 = await call(alice, "GET", "/api/a2a/catalog");
  const names = (r2.body as unknown as { machines: Shelf[] }).machines.map((s) => s.name);
  expect(names).not.toContain("bobs-other");
  expect(names[0]).toBe("alices");
  // Bob's machine sees the team box's agent too, and its own.
  await expect.poll(() => catalog(bob, "bobs"), { timeout: 60_000, intervals: [2_000] }).toEqual(["bobs/reviewer", "teambox/fixer (alice)"]);
});

test("the Team agents block: by machine and owner, offering from it, and Run here", async () => {
  test.setTimeout(120_000);
  await show(alice, "alices");
  const block = await alice.evaluate(() => window.__arugula.client.openBlock({ type: "agents", config: {}, local: true }));
  const el = alice.locator(`[data-agents-block="${block}"]`);
  await expect(el.locator('[data-shelf="teambox"] [data-agent="fixer"]')).toBeVisible({ timeout: 30_000 });
  await expect(el.locator('[data-shelf="bobs"] [data-agent="reviewer"]')).toBeVisible();
  await expect(el.locator('[data-shelf="bobs"] .agents-shelf-head')).toContainText("bob's");
  await expect(el.locator(`[data-send-task="reviewer"]`)).toBeVisible();

  // A project's recipes, offered from the block.
  const dir = project("helper", "Help out");
  await el.locator("[data-pick-dir]").click();
  await alice.locator(".prompt input").fill(dir);
  await alice.locator(".prompt input").press("Enter");
  await expect(el.locator('[data-recipe="helper"][data-offered="false"]')).toBeVisible();
  await el.locator('[data-offer="helper"]').click();
  await expect(el.locator('[data-recipe="helper"][data-offered="true"]')).toBeVisible();
  await expect(el.locator('[data-shelf="alices"] [data-agent="helper"]')).toBeVisible();
  // Bob's machine finds it, as Alice's.
  await expect.poll(() => catalog(bob, "bobs"), { timeout: 60_000, intervals: [2_000] }).toContain("alices/helper (alice)");

  // Run here: a Claude Code block beside it, as the recipe.
  await show(alice, "alices");
  const before = await alice.evaluate(() => window.__arugula.client.state!.panes.filter((p) => p.type === "agent").length);
  await el.locator('[data-run-here="helper"]').click();
  await alice.locator(".prompt input").press("Enter");
  await expect.poll(() => alice.evaluate(() => window.__arugula.client.state!.panes.filter((p) => p.type === "agent").length)).toBe(before + 1);
  const agent = await alice.evaluate(() => window.__arugula.client.state!.panes.filter((p) => p.type === "agent").at(-1)!.id);
  const st = (await call(alice, "GET", `/api/blocks/${agent}`)).body as { state?: { recipe?: string; cwd?: string } } | null;
  expect(st?.state).toMatchObject({ recipe: "helper", cwd: dir });
});

test("once Bob stops offering, his machine shuts Alice's out again", async () => {
  test.setTimeout(120_000);
  await show(bob, "bobs");
  expect((await call(bob, "POST", "/api/a2a/offers", { agent: "reviewer" })).status).toBe(200);
  await expect.poll(() => catalog(alice, "alices"), { timeout: 60_000, intervals: [2_000] }).toEqual(["alices/helper", "teambox/fixer"]);
});
