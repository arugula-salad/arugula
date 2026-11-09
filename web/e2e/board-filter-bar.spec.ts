// #614: the filter bar is on the board itself, at every width. Alice pins a
// Board block beside a terminal in a 1500px window and drags the divider until
// the block is under 520px: a window-width query would call that a desktop and
// hide the bar, but it is the board's width that counts. With clicks alone
// she picks a label, the rows narrow, and Clear brings them back. The same bar
// is on the board's page, wide. A local control (team-fixture.ts).

import { mkdirSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const t = new TeamControl("boardfilterbar");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = process.env.FILTER_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

async function shot(p: Page, name: string) {
  if (SHOTS) await p.screenshot({ path: `${SHOTS}/${name}.png`, animations: "disabled" });
}

const ROWS: { title: string; lane: string; labels: string[] }[] = [
  { title: "Search the board", lane: "next", labels: ["boards", "docs"] },
  { title: "Fold the lanes", lane: "doing", labels: ["boards"] },
  { title: "Write the guide", lane: "review", labels: ["docs"] },
  { title: "Nothing on it", lane: "done", labels: [] },
];
const row = (n: number) => ({
  id: `row_${(n + 1).toString(16).padStart(8, "0")}`,
  rev: 1,
  pointer: { kind: "issue", id: `iss_${n}` },
  note: "",
  status: "none",
  lane: ROWS[n].lane,
  attachments: [],
  live: { state: ROWS[n].lane === "done" ? "closed" : "open", kind: "native", number: n + 1, title: ROWS[n].title, assignees: [], labels: ROWS[n].labels },
  created_at: 1,
  updated_at: 1,
});

test("a narrow Board block in a wide window filters by label with clicks alone", async ({ browser }) => {
  test.setTimeout(240_000);
  await t.start(["--platform-host", "app.localhost"]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.setViewportSize({ width: 1500, height: 900 });
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await home(alice);
  await t.addMachine(alice, "alices", team);
  await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "alices");
  // The project and board are control's: asked as the page, which is signed in.
  const api = (method: string, path: string, body?: unknown) =>
    alice.evaluate(
      async ([m, u, b]) => {
        const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
        return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
      },
      [method, path, body] as const,
    );
  const projects = await api("GET", "/api/projects");
  const project = (projects.body!.projects as { id: string; name: string }[]).find((p) => p.name === "Acme")!.id;
  const made = await api("POST", `/api/projects/${project}/boards`, { title: "Filter" });
  expect(made.status).toBe(200);
  const board = made.body!.board.id as string;
  await alice.route(new RegExp(`/api/boards/${board}$`), async (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const res = await route.fetch();
    const v = await res.json();
    for (const z of v.zones) z.rows = ROWS.map((_, i) => row(i)).filter((r) => `lane_${r.lane}` === z.id);
    await route.fulfill({ response: res, json: v });
  });

  const run = await call(alice, "POST", "/api/run", { command: "sleep 600" });
  expect(run.status, JSON.stringify(run.body)).toBe(200);
  const terminal = (run.body as { pane: number }).pane;
  const opened = await call(alice, "POST", "/api/blocks", { type: "board", config: { board }, split: terminal, local: true });
  expect(opened.status, JSON.stringify(opened.body)).toBe(200);
  const pane = (opened.body as { block: number }).block;
  const blk = alice.locator(`[data-pane="${pane}"]`);
  await alice.bringToFront();
  await alice.evaluate((p) => window.__arugula.client.setActive(p), terminal);
  await expect(blk.locator("[data-board-row]")).toHaveCount(4, { timeout: 30_000 });

  // Drag the divider right until the board is under 520px.
  const divider = alice.locator(".divider.live").first();
  await expect(divider).toBeVisible();
  const d = (await divider.boundingBox())!;
  const y = d.y + d.height / 2;
  await alice.mouse.move(d.x + d.width / 2, y);
  await alice.mouse.down();
  await alice.mouse.move(1500 - 380, y, { steps: 20 });
  await alice.mouse.up();
  const widthOf = () => blk.locator("[data-board-block]").evaluate((e) => e.getBoundingClientRect().width);
  await expect.poll(widthOf, { timeout: 10_000 }).toBeLessThan(520);
  expect(await alice.evaluate(() => innerWidth)).toBeGreaterThanOrEqual(1400);

  // The bar is there with no key pressed; the menus wrap under the box.
  const bar = blk.locator("[data-board-find]");
  await expect(bar).toBeVisible();
  const input = blk.locator("[data-board-find-input]");
  await expect(input).toBeVisible();
  await expect(blk.locator("[data-board-find-clear]")).toHaveCount(0);
  const box = (await input.boundingBox())!;
  const menu = (await blk.locator('[data-board-facet-button="label"]').boundingBox())!;
  expect(menu.y).toBeGreaterThan(box.y + box.height - 1);
  await shot(alice, "block-narrow");

  await blk.locator('[data-board-facet-button="label"]').click();
  await blk.locator('[data-board-facet-value="docs"]').check();
  // The open menu's tick keeps its own size: not stretched like the box.
  expect((await blk.locator('[data-board-facet-value="docs"] .menu-lead').boundingBox())!.width).toBeLessThan(40);
  await expect(blk.locator("[data-board-row]")).toHaveCount(2);
  await expect(blk.locator("[data-board-find-count]")).toHaveText("2 of 4 rows");
  await shot(alice, "block-narrow-filtered");

  await blk.locator("[data-board-find-clear]").click();
  await expect(blk.locator("[data-board-row]")).toHaveCount(4);
  await expect(blk.locator("[data-board-find-clear]")).toHaveCount(0);
  // The menu shut when Clear was pressed; opened again, nothing in it is ticked.
  await blk.locator('[data-board-facet-button="label"]').click();
  await expect(blk.locator('[data-board-facet-value="docs"]')).not.toBeChecked();

  // The board's page, wide: one row, no key.
  await alice.goto(`${app}/p/${project}/boards/${board}`);
  await expect(alice.locator("[data-board-row]")).toHaveCount(4, { timeout: 15_000 });
  const wide = alice.locator("[data-board-find]");
  await expect(wide).toBeVisible();
  const wi = (await alice.locator("[data-board-find-input]").boundingBox())!;
  const wm = (await alice.locator('[data-board-facet-button="label"]').boundingBox())!;
  expect(Math.abs(wm.y - wi.y)).toBeLessThan(wi.height);
  await shot(alice, "page-wide");
});
