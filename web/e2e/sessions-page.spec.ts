// A project's Sessions page (Arugula inversion L3): every session in the
// project, wherever it runs, grouped by its card, with the groups that need
// you first and Not on a card last; finished sessions folded; a computer's
// session opening to its panes, each with its line, Open (live, over the
// page) and Open on its machine; Open card to the card's drawer on its
// board; and, from a cloud without the by-card route, the hosted wave alone.
// On the app's frame fixture (app-frame-page.tsx), with the cloud's
// answers from sessions-fixture.ts and a real pane of the suite's daemon.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";
import { PROJECT as P, byCard, wave } from "./sessions-fixture";

/** The reader's computer, as the cloud names it: on the daemon's own page
 * its hosts carry no ids, and the page's own daemon stands in. */
const M = "d00d0001";

test.use({ permissions: ["local-network-access"], viewport: { width: 1280, height: 800 } });

const board = {
  board: { id: "brd_00e2e0", project: P, title: "Launch", goal: "", created_by: "", created_at: 1, updated_at: 1, done_note: "", closed_by: "", progress: { done: 0, total: 1, proven: 0, review: 0, doing: 1 } },
  zones: (["next", "doing", "review", "done"] as const).map((lane) => ({
    id: `lane_${lane}`,
    rev: 1,
    title: lane[0].toUpperCase() + lane.slice(1),
    kind: "lane",
    rows:
      lane === "doing"
        ? [{ id: "row_0985", rev: 1, pointer: { kind: "issue", id: "iss_985" }, note: "", status: "none", lane, attachments: [], live: { state: "open", kind: "native", number: 985, title: "A project's Sessions page" }, created_at: 1, updated_at: 1 }]
        : [],
  })),
  groups: [],
  docs: [],
};

/** A fresh pane, the cloud's answers, then the app at the project's Sessions. */
async function openSessions(page: Page, cards: "by-card" | "not yet" = "by-card"): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => (cards === "by-card" ? r.fulfill({ json: byCard(M, pane) }) : r.fulfill({ status: 404, json: { error: "not found" } })));
  await page.route(new RegExp(`/api/projects/${P}/sessions(\\?.*)?$`), (r) => r.fulfill({ json: wave("sams-laptop") }));
  await page.route(`**/api/projects/${P}/boards`, (r) => r.fulfill({ json: { project: P, boards: [board.board] } }));
  await page.route("**/api/boards/brd_00e2e0", (r) => r.fulfill({ json: board }));
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(`/p/${P}/sessions`);
  return pane;
}

const groups = (page: Page) => page.locator("[data-sessions-group]");

test("every session in the project, by card: what needs you first, Not on a card last, finished folded", async ({ page }, info) => {
  await openSessions(page);
  await expect(groups(page)).toHaveCount(4);
  expect(await groups(page).evaluateAll((gs) => gs.map((g) => (g as HTMLElement).dataset.sessionsGroup))).toEqual(["iss_985", "iss_970", "iss_990", "none"]);
  // A card's head: its board and lane, and Open card to its row.
  const first = groups(page).nth(0);
  await expect(first.locator("[data-sessions-card]")).toHaveText("#985 A project's Sessions page");
  await expect(first.locator("[data-sessions-card-place]")).toHaveText("Launch · Doing");
  await expect(first.locator("[data-session-waiting]")).toContainText("its question");
  // A hosted session and a computer's on the same card, as rows alike.
  const rail = page.locator('[data-sessions-group="iss_970"]');
  await expect(rail.locator('[data-session-row="machine:7"]')).toContainText("on sams-laptop, yours");
  // Finished sessions fold to a line per group.
  await expect(rail.locator('[data-session-row="ses_0970bbbb"]')).toHaveCount(0);
  await rail.locator('[data-sessions-finished="1"]').click();
  await expect(rail.locator('[data-session-row="ses_0970bbbb"]')).toBeVisible();
  // Not on a card: the release terminal on a teammate's computer.
  const none = page.locator('[data-sessions-group="none"]');
  await expect(none.locator("[data-sessions-card]")).toHaveText("Not on a card");
  await expect(none.locator("[data-sessions-open-card]")).toHaveCount(0);
  await expect(none.locator('[data-session-row="machine:2"]')).toContainText("on danas-mini");
  // Runs and Workspaces stay at the top; the filters stay.
  await expect(page.locator("[data-sessions-runs]")).toBeVisible();
  await expect(page.locator("[data-sessions-label]")).toBeVisible();
  await expect(page.locator("[data-sessions-asker]")).toBeVisible();
  await page.screenshot({ path: info.outputPath("sessions-by-card.png") });
});

test("a computer's session opens to its panes: Open shows one live here, Open on its machine goes to it", async ({ page }, info) => {
  const pane = await openSessions(page);
  const row = page.locator('[data-session-row="machine:7"]');
  await row.locator("[data-session-toggle]").click();
  const line = row.locator(`[data-session-pane="${pane}"]`);
  await expect(line).toBeVisible();
  // Its line, from the machine's own summary.
  await expect(line.locator("[data-session-pane-line]")).not.toBeEmpty();
  await line.locator("[data-session-open]").click();
  const live = page.locator("[data-sessions-live]");
  await expect(live.locator(`[data-live-slot="${pane}"]`)).toBeVisible();
  await page.screenshot({ path: info.outputPath("sessions-live.png") });
  await live.locator("[data-sessions-live-close]").click();
  await expect(live).toHaveCount(0);
  // Two clicks from the list to its machine's page, on the pane.
  await line.locator("[data-session-on-machine]").click();
  // (The daemon's own page names its machine by no id, so the page may drop it.)
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]+[^/?]*)?\\?pane=${pane}$`));
  await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
});

test("Open card opens the card's drawer on its board", async ({ page }) => {
  await openSessions(page);
  await page.locator('[data-sessions-group="iss_985"] [data-sessions-open-card]').click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards/brd_00e2e0`));
  await expect(page.locator('[data-board-drawer="row_0985"]')).toBeVisible();
});

test("from a cloud without the by-card route, the hosted sessions by their issues, with the filters", async ({ page }) => {
  await openSessions(page, "not yet");
  await expect(groups(page)).toHaveCount(3);
  expect(await groups(page).evaluateAll((gs) => gs.map((g) => (g as HTMLElement).dataset.sessionsGroup))).toEqual(["iss_985", "iss_970", "iss_990"]);
  await expect(page.locator("[data-session-row^='machine:']")).toHaveCount(0);
  await page.locator("[data-sessions-going]").check();
  await expect(page).toHaveURL(/[?&]going=/);
  await expect(page.locator("[data-sessions-clear]")).toBeVisible();
});
