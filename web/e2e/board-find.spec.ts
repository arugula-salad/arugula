// Finding a row (#546). Alice owns team Acme with boards on. A board of 60
// rows (the response is replaced, as epic-boards.spec.ts does) has its Done
// lane folded in this browser: `/` opens the find box, `#41` leaves the one
// row, in the folded lane, Enter opens its drawer, and Esc brings the board
// back with its fold as it was. On the Boards page `/` filters the list. A
// local control (team-fixture.ts).

import { mkdirSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("boardfind");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = process.env.FIND_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

async function shot(p: Page, name: string) {
  if (SHOTS) await p.screenshot({ path: `${SHOTS}/${name}.png`, animations: "disabled", fullPage: true });
}

/** Rows 1 to 60: 1-15 Next, 16-30 Doing, 31-40 Review, 41-60 Done. */
const lane = (n: number) => (n <= 15 ? "next" : n <= 30 ? "doing" : n <= 40 ? "review" : "done");
const row = (n: number) => ({
  id: `row_${n.toString(16).padStart(8, "0")}`,
  rev: 1,
  pointer: { kind: "issue", id: `iss_${n}` },
  note: "",
  status: "none",
  lane: lane(n),
  attachments: [],
  live: { state: lane(n) === "done" ? "closed" : "open", kind: "native", number: n, title: n === 41 ? "Search the board" : `Task ${n}`, assignees: [] },
  created_at: 1,
  updated_at: 1,
});

test("find: / narrows a 60-row board to a row in a folded lane and Enter opens it; / filters the boards list", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await alice.goto(`${app}/`);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  const ids: Record<string, string> = {};
  for (const title of ["Big", "Launch", "Later"]) {
    ids[title] = await alice.evaluate(
      async ([p, ti]) => {
        const r = await fetch(`/api/projects/${p}/boards`, { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ title: ti }) });
        return ((await r.json()) as { board: { id: string } }).board.id;
      },
      [project, title] as const,
    );
  }
  await alice.route(new RegExp(`/api/boards/${ids.Big}$`), async (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const res = await route.fetch();
    const v = await res.json();
    for (const z of v.zones) z.rows = Array.from({ length: 60 }, (_, i) => row(i + 1)).filter((r) => `lane_${r.lane}` === z.id);
    await route.fulfill({ response: res, json: v });
  });
  const folds = JSON.stringify({ "lane:lane_done": true });
  await alice.addInitScript(([id, f]) => localStorage.setItem(`arugula.board.folds.${id}`, f), [ids.Big, folds] as const);

  for (const [w, h] of [[1280, 900], [390, 844]] as const) {
    await alice.setViewportSize({ width: w, height: h });
    await alice.emulateMedia({ colorScheme: "dark" });
    await alice.goto(`${app}/p/${project}/boards/${ids.Big}`);
    await expect(alice.locator("[data-board-row]")).toHaveCount(40, { timeout: 15_000 });
    await expect(alice.locator('[data-board-fold="lane:lane_done"]')).toHaveAttribute("aria-expanded", "false");

    // The bar is always there; `/` from the page, not a field, focuses the box.
    await expect(alice.locator("[data-board-find]")).toBeVisible();
    await alice.keyboard.press("/");
    const box = alice.locator("[data-board-find-input]");
    await expect(box).toBeFocused();
    await box.fill("#41");
    await expect(alice.locator("[data-board-row]")).toHaveCount(1);
    await expect(alice.locator("[data-board-row]")).toHaveAttribute("data-lane", "done");
    await expect(alice.locator("[data-board-find-count]")).toHaveText("1 of 60 rows");
    await expect(alice.locator("[data-board-row] [data-board-title]")).toHaveText("Search the board");
    await shot(alice, `find-${w}-dark`);

    // The saved fold is untouched, and Enter opens the first match.
    expect(await alice.evaluate((id) => localStorage.getItem(`arugula.board.folds.${id}`), ids.Big)).toBe(folds);
    await box.press("Enter");
    await expect(alice.locator("[data-board-drawer]")).toHaveAttribute("data-board-drawer", row(41).id);
    await shot(alice, `drawer-${w}-dark`);
    await alice.locator("[data-board-drawer-close]").click();

    // Words narrow too; nothing matching says so; Esc clears the box and the board is whole again.
    await box.fill("search BOARD");
    await expect(alice.locator("[data-board-row]")).toHaveCount(1);
    // A `/` typed in the box is text, not the shortcut.
    await box.fill("a");
    await box.press("/");
    await expect(box).toHaveValue("a/");
    await box.fill("zzz");
    await expect(alice.locator("[data-board-find-none]")).toBeVisible();
    await box.press("Escape");
    await expect(box).toHaveValue("");
    await expect(box).not.toBeFocused();
    await expect(alice.locator("[data-board-find]")).toBeVisible();
    await expect(alice.locator("[data-board-row]")).toHaveCount(40);
    await expect(alice.locator('[data-board-fold="lane:lane_done"]')).toHaveAttribute("aria-expanded", "false");
  }

  // The Boards page: `/` focuses the filter, typing narrows the list.
  for (const [w, h] of [[1280, 900], [390, 844]] as const) {
    await alice.setViewportSize({ width: w, height: h });
    await alice.goto(`${app}/p/${project}/boards`);
    await expect(alice.locator("[data-board-link]")).toHaveCount(3, { timeout: 15_000 });
    const filter = alice.locator("[data-boards-filter]");
    // The key listener is a passive effect: press again until it's there.
    await expect(async () => {
      await alice.keyboard.press("/");
      await expect(filter).toBeFocused({ timeout: 500 });
    }).toPass();
    await filter.fill("lau");
    await expect(alice.locator("[data-board-link]")).toHaveText(["Launch"]);
    await shot(alice, `list-${w}-dark`);
    await filter.press("Escape");
    await expect(alice.locator("[data-board-link]")).toHaveCount(3);
  }
});
