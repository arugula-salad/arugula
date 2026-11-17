// A project's Boards on the list page (Arugula #1128): a row a board, its
// progress as `13/18` and the lanes' bar; From Plane and Done (folded) as
// groups; the selected board in the middle, its address the page's, so an
// old board link selects its row; New board beside the title; Not on a
// board a link over the list, its issues in the middle, and one dragged onto
// a board's row goes on that board; on a phone the list is the page and a
// board opens in its place. On the app's frame fixture (app-frame-page.tsx),
// with the cloud's answers made up here.
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { reset } from "./helpers";

const P = "prj_aaaa0002";

test.use({ permissions: ["local-network-access"] });

const progress = (done: number, total: number, review = 0, doing = 0) => ({ done, total, proven: 0, review, doing });
const board = (id: string, title: string, p: ReturnType<typeof progress>, more: Record<string, unknown> = {}) => ({ id, project: P, title, goal: `${title}'s goal`, created_by: "acct_a", created_at: 1, updated_at: 1, done_note: "", closed_by: "", progress: p, ...more });
const plane = (key: string, started: boolean) => ({ source: { provider: "plane", key, url: "", connection: "ARUGU", left_out: 0, started } });

const BOARDS = [
  board("brd_0a01", "Launch", progress(13, 18, 2, 1)),
  board("brd_0a02", "Someday", progress(0, 0)),
  board("brd_0a03", "Epic: search", progress(2, 5, 1, 0), plane("ARUGU-10", true)),
  board("brd_0a04", "Epic: billing", progress(0, 3), plane("ARUGU-11", false)),
  board("brd_0a05", "Last year", progress(4, 4), { closed_at: 2, done_note: "Shipped." }),
];

const row = (id: string, lane: string, number: number, title: string) => ({ id, rev: 1, pointer: { kind: "issue", id: `iss_${number}` }, note: "", status: "none", lane, attachments: [], live: { state: "open", kind: "native", number, title }, created_at: 1, updated_at: 1 });
const view = (b: (typeof BOARDS)[number]) => ({
  board: b,
  zones: (["next", "doing", "review", "done"] as const).map((lane) => ({ id: `lane_${lane}`, rev: 1, title: lane[0].toUpperCase() + lane.slice(1), kind: "lane", rows: lane === "doing" ? [row(`row_${b.id}`, lane, 900, `${b.title}'s first issue`)] : [] })),
  groups: [],
  docs: [],
});

const loose = (number: number, title: string) => ({ pointer: { kind: "issue", id: `iss_${number}` }, source: "native", kind: "issue", number, title, state: "open", assignees: [], labels: ["ui"], updated_at: 1 });

/** The cloud's answers, then the app at `path`. `added` hears each add to a board. */
async function openApp(page: Page, path: string, added: string[] = []) {
  await reset(page);
  await page.route(`**/api/projects/${P}/boards`, (r) => r.fulfill({ json: { project: P, boards: BOARDS } }));
  await page.route(/\/api\/boards\/brd_[0-9a-f]+$/, (r) => {
    const b = BOARDS.find((x) => r.request().url().endsWith(x.id));
    return b ? r.fulfill({ json: view(b) }) : r.fulfill({ status: 404, json: { error: "no such board" } });
  });
  await page.route(/\/api\/boards\/brd_[0-9a-f]+\/entries\/many$/, (r) => {
    added.push(r.request().url().split("/").at(-3)!);
    return r.fulfill({ json: { added: 1 } });
  });
  await page.route(new RegExp(`/api/projects/${P}/work\\?`), (r) => r.fulfill({ json: { project: P, items: [loose(41, "Flaky login"), loose(42, "Dark mode")], next: null, total: 2, labels: [{ name: "ui", count: 2 }] } }));
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => r.fulfill({ json: { project: P, groups: [] } }));
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
}

/** A board's address, with or without its title's slug. */
const at = (id: string) => new RegExp(`/p/${P}[^/]*/boards/${id}(-[^/]*)?$`);

const item = (page: Page, id: string) => page.locator(`[data-board-item="${id}"]`);

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("boards are rows; the selected one draws in the middle and its address follows", async ({ page }) => {
    await openApp(page, `/p/${P}/boards`);
    const list = page.locator("[data-boards-page] [data-list-rows]");
    await expect(list.locator("[data-board-item]")).toHaveCount(4);
    // A row: its title, `done/total` and the lanes' bar.
    await expect(item(page, "brd_0a01").locator("[data-board-tally]")).toHaveText("13/18");
    await expect(item(page, "brd_0a01").locator("[data-board-progress] i")).toHaveCount(3);
    await expect(item(page, "brd_0a02").locator("[data-board-tally]")).toHaveText("0/0");
    await expect(item(page, "brd_0a02").locator("[data-board-progress]")).toHaveCount(0);
    // From Plane, started first, the other marked; Done folded.
    await expect(page.locator("[data-boards-plane] [data-board-item]")).toHaveCount(2);
    await expect(item(page, "brd_0a04").locator("[data-board-not-started]")).toBeVisible();
    await expect(page.locator("[data-boards-done] [data-board-item]")).toHaveCount(0);
    await page.locator("[data-boards-done] [data-list-group-fold]").click();
    await expect(item(page, "brd_0a05")).toBeVisible();
    // The first board in the middle, then the one clicked, its address the page's.
    await expect(page.locator('[data-list-preview] [data-board-page="brd_0a01"] [data-board-row="row_brd_0a01"]')).toBeVisible();
    await item(page, "brd_0a03").click();
    await expect(page).toHaveURL(at("brd_0a03"));
    await expect(page.locator('[data-list-preview] [data-board-page="brd_0a03"] [data-board-row="row_brd_0a03"]')).toBeVisible();
    await expect(item(page, "brd_0a03")).toHaveAttribute("aria-selected", "true");
    // The keys move the selection, and the address with it.
    await page.keyboard.press("ArrowUp");
    await expect(page).toHaveURL(at("brd_0a02"));
    // The search filters the rows.
    await page.locator("[data-list-search]").fill("launch");
    await expect(list.locator("[data-board-item]")).toHaveCount(1);
  });

  test("an old board link selects its row, a Done one's too", async ({ page }) => {
    await openApp(page, `/p/${P}/boards/brd_0a05`);
    await expect(item(page, "brd_0a05")).toHaveAttribute("aria-selected", "true");
    await expect(page.locator('[data-list-preview] [data-board-page="brd_0a05"]')).toBeVisible();
  });

  test("New board beside the title makes one and selects it", async ({ page }) => {
    await openApp(page, `/p/${P}/boards`);
    await page.route(`**/api/projects/${P}/boards`, (r) => (r.request().method() === "POST" ? r.fulfill({ json: view(board("brd_0a06", "Fresh", progress(0, 0))) }) : r.fallback()));
    await page.locator("[data-list-page] [data-board-new-open]").click();
    await page.locator("[data-board-new-dialog] [data-board-new-title]").fill("Fresh");
    await page.locator("[data-board-new-dialog] [data-board-new]").click();
    await expect(page).toHaveURL(at("brd_0a06"));
    await expect(page.locator("[data-board-new-dialog]")).toHaveCount(0);
  });

  test("Not on a board fills the middle; an issue dragged onto a board's row goes on it", async ({ page }) => {
    const added: string[] = [];
    await openApp(page, `/p/${P}/boards`, added);
    await expect(page.locator("[data-boards-loose-n]")).toHaveText("2");
    await page.locator("[data-boards-loose]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/triage$`));
    await expect(page.locator("[data-boards-loose]")).toHaveAttribute("aria-current", "page");
    const pane = page.locator("[data-list-preview] [data-boards-loose-pane]");
    await expect(pane.locator("[data-triage-row]")).toHaveCount(2);
    await pane.locator('[data-triage-row="#41"]').dragTo(item(page, "brd_0a01"));
    await expect.poll(() => added).toEqual(["brd_0a01"]);
    // A board's row goes back to the board, and Back to Not on a board.
    await item(page, "brd_0a03").click();
    await expect(page).toHaveURL(at("brd_0a03"));
    await page.goBack();
    await expect(page).toHaveURL(/\/triage$/);
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the list is the page, and a board opens in its place", async ({ page }) => {
    await openApp(page, `/p/${P}/boards`);
    await expect(item(page, "brd_0a01")).toBeVisible();
    await expect(page.locator("[data-list-preview]")).toBeHidden();
    await item(page, "brd_0a01").click();
    await expect(page.locator('[data-board-page="brd_0a01"]')).toBeVisible();
    await expect(page.locator("[data-list-back]")).toBeVisible();
    await page.locator("[data-list-back]").click();
    await expect(item(page, "brd_0a01")).toBeVisible();
  });

  test("a board's link opens that board", async ({ page }) => {
    await openApp(page, `/p/${P}/boards/brd_0a03`);
    await expect(page.locator('[data-board-page="brd_0a03"] [data-board-row="row_brd_0a03"]')).toBeVisible();
  });
});
