// M26: the swarm view, against a fake fleet (e2e/fake-fleet.ts): three
// daemons with scripted builds, tests, servers, logs, editors and agents
// that ask through Claude Code's real hooks. Clustering by each grouping
// (and the fallback groups for panes outside any git project), bundling on
// the "needs you" rail, and each action from the rail: allow, deny, answer,
// dismiss and a follow-up; done cards clearing themselves; a full rail;
// opening a pane, zooming to a cluster, the hover peek, a notification's
// deep link; and the phone's strip of cards, pinch and tap.

import { realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { devices, expect, test, type Page } from "@playwright/test";
import { FakeFleet } from "./fake-fleet";
import { closeContexts } from "./helpers";

test.afterAll(closeContexts);

let fake: FakeFleet;

test.use({ baseURL: async ({}, use) => use(fake?.machines[0]?.url) });
test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  fake = new FakeFleet();
  await fake.machine("workstation");
  await fake.machine("build-01");
  await fake.machine("build-02");
  await fake.populate();
});

test.afterAll(() => fake?.stop());

/** The swarm, with every machine's panes in it. */
async function swarm(page: Page) {
  await page.goto("/#swarm");
  await expect
    .poll(() => page.evaluate(() => window.__arugula?.fleet.list.filter((h) => h.state === "connected").length ?? 0), { timeout: 20_000 })
    .toBe(3);
  await expect.poll(() => page.evaluate(() => window.__arugula.fleet.panes.length)).toBeGreaterThanOrEqual(15);
  await expect(page.locator(".swarm")).toBeVisible();
}

const clusters = (page: Page) =>
  page.evaluate(() => ((window.__arugula.swarm as { clusters: { name: string; n: number }[] } | null)?.clusters ?? []).map((c) => c.name).sort());

/** Dismiss whatever is on the rail, so tests start clean. */
async function clearRail(page: Page) {
  await page.evaluate(async () => {
    const f = window.__arugula.fleet;
    for (const p of f.panes) {
      if (!p.info.reason) continue;
      await f.request(p.host, "POST", "/api/attention/act", { action: "dismiss", pane: p.id }).catch(() => {});
    }
  });
  await expect(page.locator(".swarm-card[data-bundle]")).toHaveCount(0, { timeout: 10_000 });
}

/** A pane's screen, through its host's API. */
const capture = (page: Page, host: string, pane: number) =>
  page.evaluate(
    async ([h, p]) => (await (await window.__arugula.fleet.request(h, "GET", `/api/panes/${p}/capture?format=text`)).text!()).replace(/\n/g, ""),
    [host, pane] as const,
  );

test("clusters by project (with fallback groups), machine, kind, session and owner, remembered", async ({ page }) => {
  await swarm(page);
  // By project: the git repositories, and directories for the rest, never
  // one "none" pile.
  // The fleet's other directory is under the temp dir: /tmp's group on
  // Linux, /private's on macOS (/var/folders is /private/var/folders).
  const temp = `/${realpathSync(tmpdir()).split("/")[1]}`;
  await expect.poll(() => clusters(page), { timeout: 10_000 }).toEqual(expect.arrayContaining(["api", "web", "infra", "~/scratch", temp]));
  expect(await clusters(page)).not.toContain("none");
  // It says so (#551).
  await expect(page.locator(".swarm-hint")).toContainText("A pane's project is the git repository it's in");
  // Machines, kinds, sessions, owners.
  await page.locator('[data-g="machine"]').click();
  await expect.poll(() => clusters(page)).toEqual(["build-01", "build-02", "workstation"]);
  await page.locator('[data-g="kind"]').click();
  await expect.poll(() => clusters(page)).toEqual(expect.arrayContaining(["build", "test", "server", "logs", "editor", "shell"]));
  await page.locator('[data-g="session"]').click();
  await expect.poll(async () => (await clusters(page)).every((c) => / · (workstation|build-01|build-02)$/.test(c))).toBe(true);
  await page.locator('[data-g="owner"]').click();
  await expect.poll(() => clusters(page)).toEqual(["you"]);
  // Remembered on this device.
  await page.locator('[data-g="machine"]').click();
  await page.reload();
  await expect(page.locator('[data-g="machine"]')).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => clusters(page)).toEqual(["build-01", "build-02", "workstation"]);
  // By owner (M30): yours, a teammate's and a team's, from the synthetic
  // fleet's owners beside these.
  const stop = await page.evaluateHandle(() => window.__arugula.swarmFake(40));
  await page.locator('[data-g="owner"]').click();
  await expect.poll(() => clusters(page)).toEqual(["sam", "team infra", "you"]);
  await stop.evaluate((f) => f());
    // Tiles are coloured by kind: the legend has every kind.
  await expect(page.locator(".swarm-legend span")).toHaveCount(11);
});

test("an agent on an issue wears its chip in the swarm, which clusters by board with No board last (#372)", async ({ page }, testInfo) => {
  const a = await fake.agentOn("workstation", "#12", { title: "Launch" });
  const b = await fake.agentOn("build-01", "#13", { title: "Launch" });
  const c = await fake.agentOn("build-02", "#14", { title: "Later" });
  const d = await fake.agentOn("workstation", "#15");
  await swarm(page);
  const chip = (host: string, id: number) => page.evaluate(([h, i]) => window.__arugula.fleet.panes.find((p) => p.host === h && p.id === i)?.info.on_issue?.issue ?? null, [host, id] as const);
  await expect.poll(() => chip("workstation", a)).toBe("#12");
  await expect.poll(() => chip("build-02", c)).toBe("#14");
  await page.locator('[data-g="board"]').click();
  await expect(page.locator('[data-g="board"]')).toHaveText("Board");
  await expect.poll(() => clusters(page), { timeout: 10_000 }).toEqual(["Later", "Launch", "No board"]);
  // The agents in them: two on Launch, one on Later, and every other pane (and #15, on no row) on none.
  const sized = await page.evaluate(() => Object.fromEntries((window.__arugula.swarm as { clusters: { name: string; n: number }[] }).clusters.map((c) => [c.name, c.n])));
  expect([sized["Launch"], sized["Later"]]).toEqual([2, 1]);
  expect(sized["No board"]).toBeGreaterThan(10);
  // Hovering it peeks, with the chip (the board's title in its tooltip).
  const key = await page.evaluate(([h, i]) => window.__arugula.fleet.panes.find((p) => p.host === h && p.id === i)!.key, ["build-01", b] as const);
  await page.waitForTimeout(1500);
  const pos = (await page.evaluate((k) => (window.__arugula.swarm as { screenOf(k: string): { x: number; y: number } }).screenOf(k), key))!;
  await page.mouse.move(pos.x, pos.y);
  await expect(page.locator(".swarm-peek [data-swarm-work]")).toHaveText("#13", { timeout: 10_000 });
  await expect(page.locator(".swarm-peek [data-swarm-work]")).toHaveAttribute("title", "#13 on Launch");
  expect(await chip("workstation", d)).toBe("#15");
  await page.screenshot({ path: testInfo.outputPath("swarm-by-board.png") });
  // The sessions sidebar on the panes: its session's row has the chip, linking to the board's row.
  await page.goto("/");
  if (!(await page.locator("[data-session-sidebar]").count())) await page.locator(".ws-head [data-open-sessions]").click();
  await expect(page.locator("[data-session-sidebar]")).toBeVisible();
  const row = page.locator(".ws-session", { has: page.locator("[data-session-work]", { hasText: "#12" }) }).first();
  await expect(row.locator("[data-session-work]").first()).toHaveAttribute("title", /#12 on Launch/, { timeout: 20_000 });
  await page.screenshot({ path: testInfo.outputPath("sidebar.png") });
  for (const [h, i] of [["workstation", a], ["build-01", b], ["build-02", c], ["workstation", d]] as const) await fake.close(h, i);
});

/** A made-up board view (the platform's `GET /api/boards/:id`) of the board "Launch": a row is `[id, lane, number, title]`. */
type Lane = "next" | "doing" | "review" | "done";
interface Placing {
  rows: [string, Lane, number, string][];
  /** Row id to the agent block attached to it: `[host, pane]`. */
  attached: Record<string, [string, number]>;
}
const LANES: [Lane, string][] = [["next", "Next"], ["doing", "Doing"], ["review", "Review"], ["done", "Done"]];

function boardView(id: string, p: Placing, machineOf: (host: string) => string) {
  const row = ([rid, lane, n, title]: Placing["rows"][number]) => ({
    id: rid, rev: 1, pointer: { kind: "issue", id: `iss_${rid}` }, note: "", status: "none", lane,
    attachments: p.attached[rid] ? [{ id: `att_${rid}`, role: "work", pointer: { kind: "block", machine: machineOf(p.attached[rid][0]), pane: `%${p.attached[rid][1]}` } }] : [],
    live: { state: "open", kind: "native", number: n, title }, created_at: 1, updated_at: 1,
  });
  return {
    board: { id, project: "prj_e2e", title: "Launch", goal: "", created_by: "", created_at: 1 },
    zones: LANES.map(([lane, title]) => ({ id: `lane_${lane}`, rev: 1, title, kind: "lane", rows: p.rows.filter((r) => r[1] === lane).map(row) })),
    groups: [],
  };
}

/** Serve the board view `current()` says, as the platform would. */
async function serveBoard(page: Page, id: string, current: () => unknown) {
  await page.route(`**/api/boards/${id}`, (route) => route.fulfill({ json: current() }));
}

/** The swarm clusters by name, with how many tiles each holds. */
const sizes = (page: Page) =>
  page.evaluate(() => Object.fromEntries((window.__arugula.swarm as { clusters: { name: string; n: number }[] }).clusters.map((c) => [c.name, c.n])));

/** What the board's agents are: A on a row in Doing by `on_issue`; B on a row in Doing only by the row's attachment; F says Next by `on_issue` but its row in Review has it attached. Others are on another board or none. */
async function boardAgents() {
  const A = await fake.agentOn("workstation", "#12", { title: "Launch", id: "brd_e2e", row: "row_b" });
  const B = await fake.agentOn("build-01", "#13");
  const F = await fake.agentOn("build-02", "#14", { title: "Launch", id: "brd_e2e", row: "row_a" });
  const other = await fake.agentOn("workstation", "#15", { title: "Later", id: "brd_other", row: "row_z" });
  const none = await fake.agentOn("build-01", "#16");
  return { A, B, F, other, none, all: [["workstation", A], ["build-01", B], ["build-02", F], ["workstation", other], ["build-01", none]] as [string, number][] };
}

const hostId = (page: Page) => (host: string) => page.evaluate((h) => window.__arugula.fleet.list.find((x) => x.name === h)?.id ?? h, host);

test("a board focus clusters its agents by the lane of their row, and a row that moves takes its agent along (#466)", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const ag = await boardAgents();
  const ids: Record<string, string> = {};
  const placing: Placing = {
    rows: [["row_a", "next", 11, "Write the changelog"], ["row_b", "doing", 12, "Fix the installer"], ["row_c", "doing", 13, "Tidy the docs"], ["row_r", "review", 14, "Cut the release"]],
    attached: { row_c: ["build-01", ag.B], row_r: ["build-02", ag.F] },
  };
  await serveBoard(page, "brd_e2e", () => boardView("brd_e2e", placing, (h) => ids[h]));
  await swarm(page);
  for (const h of ["workstation", "build-01", "build-02"]) ids[h] = await hostId(page)(h);
  await expect.poll(() => page.evaluate((k) => window.__arugula.fleet.panes.find((p) => p.key === k)?.info.on_issue?.boards?.[0]?.row ?? null, `build-02:${ag.F}`)).toBe("row_a");

  // In Cluster by → Board, a board's name focuses it; "No board" only dives in.
  await page.locator('[data-g="board"]').click();
  await expect.poll(() => clusters(page), { timeout: 10_000 }).toEqual(["Later", "Launch", "No board"]);
  const clickName = async (name: string) => {
    await expect
      .poll(async () => {
        const box = (await page.evaluate(() => (window.__arugula.swarm as unknown as { labelBoxes: { name: string; x0: number; x1: number; y0: number; y1: number }[] }).labelBoxes)).find((b) => b.name === name);
        if (box) await page.mouse.click((box.x0 + box.x1) / 2, (box.y0 + box.y1) / 2);
        return !!box;
      }, { timeout: 15_000 })
      .toBe(true);
  };
  await clickName("No board");
  await page.waitForTimeout(300);
  expect(new URL(page.url()).hash).toBe("#swarm");
  await clickName("Launch");
  await expect.poll(() => new URL(page.url()).hash).toBe("#swarm=board:brd_e2e");

  // The focus: the board's lanes (Doing, Review, Next, Done: the stub sends them Next first), with the agents in them. A: Doing (on_issue). B: Doing (attachment only). F: Review (the attachment wins over on_issue's Next).
  await expect(page.locator("[data-swarm-board] b")).toHaveText("Board: Launch");
  await expect(page.locator("[data-lane]")).toHaveText(["Doing · 2 rows · 2 workers", "Review · 1 row · 1 worker", "Next · 1 row · no one on it"]);
  await expect.poll(() => sizes(page), { timeout: 10_000 }).toEqual({ Doing: 2, Review: 1 });
  // The other board's agent and the one on none are not here.
  await expect.poll(() => page.evaluate(() => window.__arugula.fleet.panes.length)).toBeGreaterThanOrEqual(15);
  await expect(page.locator('[data-stat="panes"]')).toHaveText("3");
  await expect(page.locator(".swarm-field")).toBeVisible();
  await page.locator("[data-fit]").click();
  // Once settled, each tile is under its own lane's name.
  await expect
    .poll(
      () =>
        page.evaluate(
          ([a, b, f]) => {
            const sw = window.__arugula.swarm as unknown as { labelBoxes: { name: string; x0: number; x1: number }[]; screenOf(k: string): { x: number } | null };
            const box = (n: string) => sw.labelBoxes.find((b) => b.name === n);
            return [["Doing", a], ["Doing", b], ["Review", f]].map(([lane, key]) => {
              const b = box(lane), t = sw.screenOf(key);
              return !!b && !!t && t.x >= b.x0 && t.x <= b.x1;
            });
          },
          [`workstation:${ag.A}`, `build-01:${ag.B}`, `build-02:${ag.F}`] as const,
        ),
      { timeout: 20_000 },
    )
    .toEqual([true, true, true]);
  await page.screenshot({ path: testInfo.outputPath("board-focus-desktop.png") });

  // The tile wears its row: `#12 · Fix the installer`.
  await expect
    .poll(() => page.evaluate((k) => (window.__arugula.swarm as unknown as { tiles: Map<string, { label: string }> }).tiles.get(k)?.label ?? "", `workstation:${ag.A}`))
    .toContain("#12 · Fix the installer");

  // A row moves lane, as the board's own push says it did: no reload.
  const reloaded = await page.evaluate(() => ((window as unknown as { __nav?: number }).__nav = 1));
  placing.rows[2][1] = "review";
  await page.evaluate(() => (window.__arugula.chat as unknown as { objectChanged(t: string): void }).objectChanged("brd_e2e"));
  await expect(page.locator("[data-lane]")).toHaveText(["Doing · 1 row · 1 worker", "Review · 2 rows · 2 workers", "Next · 1 row · no one on it"], { timeout: 10_000 });
  await expect.poll(() => sizes(page), { timeout: 10_000 }).toEqual({ Doing: 1, Review: 2 });
  expect(await page.evaluate(() => (window as unknown as { __nav?: number }).__nav)).toBe(reloaded);
  // The clusters follow the board's lanes, not their size: Review now has more workers than Doing, and still comes after it, to its right.
  await expect.poll(() => page.evaluate(() => [...document.querySelectorAll(".swarm-clusters [data-cluster]")].map((e) => e.getAttribute("data-cluster")))).toEqual(["Doing", "Review"]);
  await expect
    .poll(async () => {
      const at = Object.fromEntries((await page.evaluate(() => (window.__arugula.swarm as unknown as { labelBoxes: { name: string; x0: number }[] }).labelBoxes)).map((b) => [b.name, b.x0]));
      return at.Doing !== undefined && at.Review !== undefined && at.Doing < at.Review;
    })
    .toBe(true);

  // Another theme only changes how the clusters are drawn.
  await page.locator('[data-theme-pick="hive"]').click();
  await expect.poll(() => clusters(page), { timeout: 15_000 }).toEqual(["Doing", "Review"]);
  await page.locator('[data-theme-pick="timeline"]').click();
  await expect.poll(() => clusters(page), { timeout: 15_000 }).toEqual(["Doing", "Review"]);
  await page.locator('[data-theme-pick="blocks"]').click();

  // All panes: back to every pane.
  await page.locator("[data-swarm-all]").click();
  await expect(page.locator("[data-swarm-board]")).toHaveCount(0);
  expect(new URL(page.url()).hash).toBe("#swarm");
  await expect.poll(() => page.evaluate(() => window.__arugula.fleet.panes.length)).toBeGreaterThanOrEqual(15);
  await expect(page.locator('[data-stat="panes"]')).not.toHaveText("3");
  for (const [h, i] of ag.all) await fake.close(h, i);
});

test("a board that can't be read says so, and shows no one", async ({ page }) => {
  await page.route("**/api/boards/brd_nope", (route) => route.fulfill({ status: 404, json: { error: "no such board" } }));
  await page.goto("/#swarm=board:brd_nope");
  await expect(page.locator("[data-swarm-board-missing]")).toContainText("no such board", { timeout: 20_000 });
  await expect(page.locator('[data-stat="panes"]')).toHaveText("0");
});

test.describe("a board focus on a phone", () => {
  const { userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport: { width: 390, height: 844 }, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the same lanes fit the phone's width (#466)", async ({ page }, testInfo) => {
    const ag = await boardAgents();
    const ids: Record<string, string> = {};
    const placing: Placing = {
      rows: [["row_a", "next", 11, "Write the changelog"], ["row_b", "doing", 12, "Fix the installer"], ["row_c", "doing", 13, "Tidy the docs"], ["row_r", "review", 14, "Cut the release"]],
      attached: { row_c: ["build-01", ag.B], row_r: ["build-02", ag.F] },
    };
    await serveBoard(page, "brd_e2e", () => boardView("brd_e2e", placing, (h) => ids[h]));
    await swarm(page);
    for (const h of ["workstation", "build-01", "build-02"]) ids[h] = await hostId(page)(h);
    await page.evaluate(() => (location.hash = "swarm=board:brd_e2e"));
    await expect(page.locator("[data-lane]")).toHaveText(["Doing · 2 rows · 2 workers", "Review · 1 row · 1 worker", "Next · 1 row · no one on it"], { timeout: 30_000 });
    await expect.poll(() => sizes(page), { timeout: 10_000 }).toEqual({ Doing: 2, Review: 1 });
    // No sideways scroll.
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.waitForTimeout(1500);
    await page.screenshot({ path: testInfo.outputPath("board-focus-phone.png") });
    for (const [h, i] of ag.all) await fake.close(h, i);
  });
});

test("Panes and Escape lead back to the panes (#551)", async ({ page }) => {
  await swarm(page);
  // The rail beside the swarm: its Panes.
  await page.locator(".ws-rail").getByRole("button", { name: "Panes", exact: true }).click();
  await expect(page.locator(".swarm")).toBeHidden();
  expect(new URL(page.url()).hash).toBe("");
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".swarm")).toBeHidden();
  expect(new URL(page.url()).hash).toBe("");
});

test("a finished card's main button is Open, and Dismiss is down with Show and Edit", async ({ page }) => {
  await swarm(page);
  const pane = await fake.finish("workstation");
  const card = page.locator(`.swarm-card[data-panes~="workstation:${pane}"]`);
  await expect(card).toBeVisible({ timeout: 20_000 });
  await expect(card.locator(".ca button.primary")).toHaveText(["Open"]);
  await expect(card.locator(".ca-nav [data-dismiss-card]")).toBeVisible();
  await card.locator("[data-open-card]").click();
  await expect(page.locator(".swarm")).toBeHidden();
});

test("failures on one machine bundle into one card, dismissed together", async ({ page }) => {
  await swarm(page);
  const panes = await fake.trouble("build-02", 3);
  const card = page.locator('.swarm-card[data-bundle="failed:build-02"]');
  await expect(card).toBeVisible({ timeout: 20_000 });
  await expect(card.locator(".ch .ct")).toHaveText("3 failed on build-02", { timeout: 10_000 });
  await expect(card.locator(".n")).toHaveText("×3");
  await expect(card.locator(".cq")).toContainText("cargo test -p relay failed (exit 101)");
  await card.getByRole("button", { name: "Dismiss all 3" }).click();
  await expect(card).toHaveCount(0);
  for (const id of panes) {
    await expect.poll(() => page.evaluate((k) => window.__arugula.fleet.panes.find((p) => p.key === k)?.info.attention, `build-02:${id}`)).toBe("idle");
  }
});

test("an agent's approval is allowed from the rail, and the follow-up wakes it", async ({ page }) => {
  await swarm(page);
  await clearRail(page);
  const pane = await fake.agentAsks("workstation", "api", "cargo test -p swarm");
  const card = page.locator(`.swarm-card[data-panes~="workstation:${pane}"]`);
  await expect(card).toBeVisible({ timeout: 20_000 });
  await expect(card.locator(".ch .ct")).toHaveText("Claude Code asks");
  await expect(card.locator(".agent-perm-cmd")).toHaveText("cargo test -p swarm");
  await card.getByRole("button", { name: "Allow", exact: true }).click();
  // The card says who allowed it, and offers a follow-up.
  const done = page.locator(`.swarm-card[data-answered][data-panes="workstation:${pane}"]`);
  await expect(done.locator(".answered-by")).toHaveText(/^Allowed by .+, \d\d:\d\d/);
  await expect.poll(() => capture(page, "workstation", pane)).toContain('"behavior":"allow"');
  await expect.poll(() => page.evaluate((k) => window.__arugula.fleet.panes.find((p) => p.key === k)?.info.inbox, `workstation:${pane}`)).toBe(true);
  await done.locator(".followup input").fill("now run the tests");
  await done.locator(".followup button").click();
  await expect(done.locator(".followup-sent")).toHaveText("Sent.");
  await expect.poll(() => capture(page, "workstation", pane)).toContain("(sent through Arugula): now run the tests");
  await fake.close("workstation", pane);
});

test("two agents asking in one project are one card: deny them both", async ({ page }) => {
  await swarm(page);
  await clearRail(page);
  const a = await fake.agentAsks("workstation", "web", "rm -rf build");
  const b = await fake.agentAsks("build-01", "web", "rm -rf build");
  const card = page.locator(`.swarm-card[data-kind="ask"][data-panes~="workstation:${a}"]`);
  await expect(card).toHaveAttribute("data-panes", new RegExp(`build-01:${b}`), { timeout: 20_000 });
  await expect(card.locator(".ch .ct")).toHaveText("2 agents ask");
  await card.getByRole("button", { name: "Deny all 2" }).click();
  await expect(card).toHaveCount(0);
  await expect.poll(() => capture(page, "workstation", a)).toContain('"behavior":"deny"');
  await expect.poll(() => capture(page, "build-01", b)).toContain('"behavior":"deny"');
  await fake.close("workstation", a);
  await fake.close("build-01", b);
});

test("a question is answered on its card", async ({ page }) => {
  await swarm(page);
  await clearRail(page);
  const pane = await fake.agentQuestion("build-02", "infra");
  const card = page.locator(`.swarm-card[data-bundle][data-panes~="build-02:${pane}"]`);
  await expect(card.locator(".cq")).toHaveText("Which colour do you prefer?", { timeout: 20_000 });
  // AskUserQuestion's card: three questions, answered in place.
  const q = (n: number) => card.locator(`[data-question="${n}"]`);
  await q(0).getByRole("radio", { name: /^Blue/ }).click();
  await q(1).getByRole("checkbox", { name: /Pear/ }).check();
  await q(2).getByRole("radio", { name: /^Cat/ }).click();
  await card.getByRole("button", { name: "Submit" }).click();
  await expect(card).toHaveCount(0);
  await expect(page.locator(`.swarm-card[data-answered][data-panes="build-02:${pane}"] .answered-by`)).toHaveText(/^Answered by /);
  await expect.poll(() => capture(page, "build-02", pane)).toContain("Blue");
  await fake.close("build-02", pane);
});

test("a finished build's card clears by itself", async ({ page }) => {
  test.setTimeout(60_000);
  await swarm(page);
  await clearRail(page);
  const pane = await fake.finish("workstation");
  const card = page.locator(`.swarm-card[data-kind="done"][data-panes~="workstation:${pane}"]`);
  await expect(card).toBeVisible({ timeout: 20_000 });
  await expect(card.locator(".cq")).toContainText("cargo build --release finished after 5s");
  await expect(card).toHaveCount(0, { timeout: 20_000 });
});

test("a full rail: the rest pulse in place, and the rail says how many", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1000, height: 480 } });
  await swarm(page);
  await clearRail(page);
  const failing = (
    await Promise.all([fake.trouble("workstation", 1), fake.trouble("build-01", 1), fake.trouble("build-02", 2)])
  ).flatMap((ids, i) => ids.map((id) => `${["workstation", "build-01", "build-02"][i]}:${id}`));
  await expect(page.locator(".swarm-card[data-bundle]")).toHaveCount(2, { timeout: 20_000 });
  await expect(page.locator(".swarm-waiting")).toHaveAttribute("data-waiting", /^[12]$/);
  // Every run has failed before the rail is cleared: one failing after it
  // would put a card back.
  await expect
    .poll(() => page.evaluate((keys) => keys.filter((k) => window.__arugula.fleet.panes.find((p) => p.key === k)?.info.reason).length, failing), {
      timeout: 20_000,
    })
    .toBe(4);
  await clearRail(page);
  await expect(page.locator(".swarm-waiting")).toHaveCount(0);
  await page.close();
});

test("hover peeks, a cluster name zooms, a pane opens in its tab, a notification opens its card", async ({ page }) => {
  await swarm(page);
  await page.locator('[data-g="machine"]').click();
  await page.waitForTimeout(1500);
  // A tile: hover shows its last lines; click opens it, connected for real.
  const key = await page.evaluate(() => window.__arugula.fleet.panes.find((p) => p.host === "build-01" && p.info.kind === "test")!.key);
  const at = async () => (await page.evaluate((k) => (window.__arugula.swarm as { screenOf(k: string): { x: number; y: number } }).screenOf(k), key))!;
  await page.waitForTimeout(1500);
  let pos = await at();
  await page.mouse.move(pos.x, pos.y);
  await expect(page.locator(".swarm-peek pre")).toContainText("test vt::parser::case_", { timeout: 10_000 });
  // The cluster's name zooms in.
  const z0 = await page.evaluate(() => (window.__arugula.swarm as { cam: { tz: number } }).cam.tz);
  const label = (await page.evaluate(() => (window.__arugula.swarm as { labelOf(n: string): { x: number; y: number } }).labelOf("workstation")))!;
  await page.mouse.click(label.x, label.y);
  await expect.poll(() => page.evaluate(() => (window.__arugula.swarm as { cam: { tz: number } }).cam.tz)).toBeGreaterThan(z0);
  await page.locator("[data-fit]").click();
  await page.waitForTimeout(1200);
  pos = await at();
  await page.mouse.click(pos.x, pos.y);
  await expect(page.locator(".swarm")).toHaveCount(0);
  const id = Number(key.split(":")[1]);
  await expect.poll(() => page.evaluate(() => window.__arugula.hosts.current)).toBe("build-01");
  await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(id);
  // A notification for a pane that wants you opens the swarm at its card.
  const [pane] = await fake.trouble("workstation", 1);
  await page.goto(`/#swarm=${pane}`);
  await expect(page.locator(`.swarm-card.focus[data-panes~="workstation:${pane}"]`)).toBeVisible({ timeout: 20_000 });
  await clearRail(page);
});

test.describe("phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("cluster names never overlap on a phone, by project or by machine", async ({ page }) => {
    await swarm(page);
    // Crowd it: the synthetic fleet adds projects and machines beside the
    // real ones, and a few need you, so labels carry their longest lines.
    const stop = await page.evaluateHandle(() => window.__arugula.swarmFake(300));
    await fake.trouble("build-02", 2);
    type Box = { name: string; x0: number; y0: number; x1: number; y1: number };
    const overlaps = async () => {
      const boxes: Box[] = await page.evaluate(() => (window.__arugula.swarm as unknown as { labelBoxes: Box[] }).labelBoxes);
      const out: string[] = [];
      for (let i = 0; i < boxes.length; i++)
        for (let j = i + 1; j < boxes.length; j++) {
          const [a, b] = [boxes[i], boxes[j]];
          if (a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1) out.push(`${a.name} × ${b.name}`);
        }
      return { n: boxes.length, out };
    };
    // A slow phone (or a busy CI runner) draws a few frames a second: the
    // clusters still reach their places in time, not in frames.
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: 15 });
    for (const by of ["project", "machine"]) {
      await page.locator(`[data-g="${by}"]`).click();
      await page.locator("[data-fit]").tap();
      // Through the regroup's animation and after it settles.
      for (let i = 0; i < 12; i++) {
        await page.waitForTimeout(400);
        const { n, out } = await overlaps();
        expect(n, `${by}: clusters drawn`).toBeGreaterThanOrEqual(6);
        expect(out, `${by}: overlapping names`).toEqual([]);
      }
    }
    await stop.evaluate((f) => f());
    await clearRail(page);
  });

  test("cards along the bottom: allow and dismiss there; pinch zooms; a tap opens a pane", async ({ page }) => {
    await swarm(page);
    await clearRail(page);
    // The rail is a strip along the bottom.
    const rail = await page.locator(".swarm-rail").boundingBox();
    expect(rail!.y).toBeGreaterThan(viewport.height / 2);
    const ask = await fake.agentAsks("build-01", "infra", "npm publish --dry-run");
    const [failed] = await fake.trouble("workstation", 1);
    const askCard = page.locator(`.swarm-card[data-panes~="build-01:${ask}"]`);
    await expect(askCard).toBeVisible({ timeout: 20_000 });
    await askCard.getByRole("button", { name: "Allow", exact: true }).tap();
    await expect.poll(() => capture(page, "build-01", ask)).toContain('"behavior":"allow"');
    await fake.close("build-01", ask);
    const failCard = page.locator(`.swarm-card[data-panes~="workstation:${failed}"]`);
    await failCard.scrollIntoViewIfNeeded();
    await failCard.getByRole("button", { name: "Dismiss" }).tap();
    await expect(failCard).toHaveCount(0);
    // Pinch: two fingers apart zoom in.
    const z0 = await page.evaluate(() => (window.__arugula.swarm as { cam: { tz: number } }).cam.tz);
    await page.evaluate(() => {
      const cv = document.querySelector<HTMLCanvasElement>(".swarm-field")!;
      const ev = (type: string, id: number, x: number) =>
        cv.dispatchEvent(new PointerEvent(type, { pointerId: id, clientX: x, clientY: 300, bubbles: true, pointerType: "touch" }));
      ev("pointerdown", 1, 180);
      ev("pointerdown", 2, 210);
      ev("pointermove", 1, 120);
      ev("pointermove", 2, 290);
      ev("pointerup", 1, 120);
      ev("pointerup", 2, 290);
    });
    await expect.poll(() => page.evaluate(() => (window.__arugula.swarm as { cam: { tz: number } }).cam.tz)).toBeGreaterThan(z0 * 2);
    await page.locator("[data-fit]").tap();
    await page.waitForTimeout(1500);
    // A tap on a tile opens it.
    const key = await page.evaluate(() => window.__arugula.fleet.panes.find((p) => p.host === "workstation" && p.info.kind === "server")!.key);
    const pos = (await page.evaluate((k) => (window.__arugula.swarm as { screenOf(k: string): { x: number; y: number } }).screenOf(k), key))!;
    await page.touchscreen.tap(pos.x, pos.y);
    await expect(page.locator(".swarm")).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(Number(key.split(":")[1]));
  });

  // #237: a card's Share opens its pane, on its machine, with the share
  // picker for its session over it.
  test("a card's Share opens the pane with its session's share picker", async ({ page }) => {
    await swarm(page);
    await clearRail(page);
    const [pane] = await fake.trouble("build-02", 1);
    const card = page.locator(`.swarm-card[data-panes~="build-02:${pane}"]`);
    await expect(card).toBeVisible({ timeout: 20_000 });
    await card.locator("[data-share-card]").tap();
    await expect(page.locator(".swarm")).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => window.__arugula.hosts.current)).toBe("build-02");
    await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(pane);
    const session = await page.evaluate((p) => window.__arugula.client.sessionOfPane(p), pane);
    const dialog = page.locator(`[data-share="${session}"]`);
    await expect(dialog).toBeVisible();
    await expect(dialog.getByLabel("Who")).toBeVisible();
    await dialog.getByRole("button", { name: "Done" }).tap();
    await expect(dialog).toHaveCount(0);
    await swarm(page);
    await clearRail(page);
  });
});
