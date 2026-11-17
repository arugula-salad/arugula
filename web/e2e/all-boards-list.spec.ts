// Every project's Boards (`/boards`, the rail's) on the list page, as a
// project's Boards is (Arugula #1131): a group a project, headed by its name
// and how many of its issues are on no board; its open boards as rows, every
// project's Done folded at the foot, each naming its project; the selected
// board in the middle, its address `/boards/:board`, so a link selects its
// row. Not on a board is a project's, so it has no row here: over a board,
// a link goes to it on the project's page. On a phone the
// list is the page and a board opens in its place. On the app's frame
// fixture (app-frame-page.tsx), with the cloud's answers made up here.
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { reset } from "./helpers";

const A = "prj_aaaa0002";
const S = "prj_aaaa0003";

test.use({ permissions: ["local-network-access"] });

const progress = (done: number, total: number, review = 0, doing = 0) => ({ done, total, proven: 0, review, doing });
const board = (project: string, id: string, title: string, p: ReturnType<typeof progress>, more: Record<string, unknown> = {}) => ({ id, project, title, goal: `${title}'s goal`, created_by: "acct_a", created_at: 1, updated_at: 1, done_note: "", closed_by: "", progress: p, ...more });
const plane = (key: string, started: boolean) => ({ source: { provider: "plane", key, url: "", connection: "ARUGU", left_out: 0, started } });

const BOARDS: Record<string, ReturnType<typeof board>[]> = {
  [A]: [
    board(A, "brd_0a01", "Launch", progress(13, 18, 2, 1)),
    board(A, "brd_0a04", "Epic: billing", progress(0, 3), plane("ARUGU-11", false)),
    board(A, "brd_0a05", "Last year", progress(4, 4), { closed_at: 2, done_note: "Shipped." }),
  ],
  [S]: [board(S, "brd_0b01", "Site relaunch", progress(2, 5, 1, 0))],
};
const every = Object.values(BOARDS).flat();

const row = (id: string, lane: string, number: number, title: string) => ({ id, rev: 1, pointer: { kind: "issue", id: `iss_${number}` }, note: "", status: "none", lane, attachments: [], live: { state: "open", kind: "native", number, title }, created_at: 1, updated_at: 1 });
const view = (b: (typeof every)[number]) => ({
  board: b,
  zones: (["next", "doing", "review", "done"] as const).map((lane) => ({ id: `lane_${lane}`, rev: 1, title: lane[0].toUpperCase() + lane.slice(1), kind: "lane", rows: lane === "doing" ? [row(`row_${b.id}`, lane, 900, `${b.title}'s first issue`)] : [] })),
  groups: [],
  docs: [],
});

/** The cloud's answers, then the app at `path`: Arugula and Site have boards, Personal none. */
async function openApp(page: Page, path: string) {
  await reset(page);
  await page.route(/\/api\/projects\/prj_[0-9a-f]+\/boards$/, (r) => {
    const project = r.request().url().split("/").at(-2)!;
    return r.fulfill({ json: { project, boards: BOARDS[project] ?? [] } });
  });
  await page.route(/\/api\/boards\/brd_[0-9a-f]+$/, (r) => {
    const b = every.find((x) => r.request().url().endsWith(x.id));
    return b ? r.fulfill({ json: view(b) }) : r.fulfill({ status: 404, json: { error: "no such board" } });
  });
  // Two of Arugula's issues are on no board, none of Site's.
  await page.route(/\/api\/projects\/prj_[0-9a-f]+\/work\?/, (r) => {
    const project = r.request().url().split("/").at(-2)!;
    const total = project === A ? 2 : 0;
    return r.fulfill({ json: { project, items: [], next: null, total, labels: [] } });
  });
  await page.route(/\/api\/projects\/prj_[0-9a-f]+\/sessions\/by-card$/, (r) => r.fulfill({ json: { groups: [] } }));
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route((u) => u.pathname === "/boards" || u.pathname.startsWith("/boards/") || u.pathname.startsWith(`/p/${A}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
}

/** A board's address on this page, with or without its title's slug. */
const at = (id: string) => new RegExp(`/boards/${id}(-[^/?]*)?\\?all=1$`);

const item = (page: Page, id: string) => page.locator(`[data-all-boards-page] [data-board-item="${id}"]`);

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("a group a project, its boards as rows; the selected one in the middle, its address the page's", async ({ page }) => {
    await openApp(page, "/boards?all=1");
    const list = page.locator("[data-all-boards-page] [data-list-rows]");
    // Arugula and Site, Personal (no boards) left out; no Not on a board row.
    await expect(list.locator("[data-all-boards-project]")).toHaveCount(2);
    await expect(list.locator(`[data-all-boards-project="${A}"] .list-group-title`)).toContainText("Arugula");
    await expect(list.locator(`[data-all-boards-project="${A}"] [data-all-boards-untriaged="${A}"]`)).toHaveText("2 not on a board");
    await expect(list.locator(`[data-all-boards-untriaged="${S}"]`)).toHaveCount(0);
    await expect(list.locator("[data-boards-loose]")).toHaveCount(0);
    await expect(list.locator(`[data-all-boards-project="${A}"] [data-board-item]`)).toHaveCount(2);
    await expect(list.locator(`[data-all-boards-project="${S}"] [data-board-item]`)).toHaveCount(1);
    // The project's page's row: `done/total`, the lanes' bar, not started marked.
    await expect(item(page, "brd_0a01").locator("[data-board-tally]")).toHaveText("13/18");
    await expect(item(page, "brd_0a01").locator("[data-board-progress] i")).toHaveCount(3);
    await expect(item(page, "brd_0a04").locator("[data-board-not-started]")).toBeVisible();
    // Done folded, every project's; unfolded, a row names its project.
    await expect(page.locator("[data-boards-done] [data-board-item]")).toHaveCount(0);
    await page.locator("[data-boards-done] [data-list-group-fold]").click();
    await expect(item(page, "brd_0a05").locator(`[data-board-project="${A}"]`)).toHaveText("Arugula");
    // The first board in the middle, with the way to its project's page.
    const middle = page.locator("[data-all-boards-page] [data-list-preview]");
    await expect(middle.locator('[data-board-page="brd_0a01"] [data-board-row="row_brd_0a01"]')).toBeVisible();
    await expect(middle.locator(`[data-all-boards-project-link="${A}"]`)).toHaveAttribute("href", new RegExp(`/p/${A}[^/]*/boards/brd_0a01`));
    // And to its issues on no board, Not on a board's row on its page.
    await expect(middle.locator(`[data-all-boards-untriaged-link="${A}"]`)).toHaveText("2 not on a board");
    await expect(middle.locator(`[data-all-boards-untriaged-link="${A}"]`)).toHaveAttribute("href", new RegExp(`/p/${A}[^/]*/triage$`));
    // Another project's board, clicked: the middle and the address follow.
    await item(page, "brd_0b01").click();
    await expect(page).toHaveURL(at("brd_0b01"));
    await expect(middle.locator('[data-board-page="brd_0b01"] [data-board-row="row_brd_0b01"]')).toBeVisible();
    await expect(item(page, "brd_0b01")).toHaveAttribute("aria-selected", "true");
    await expect(middle.locator("[data-all-boards-untriaged-link]")).toHaveCount(0);
    await page.keyboard.press("ArrowUp");
    await expect(page).toHaveURL(at("brd_0a04"));
    // The search filters across the projects; a project with no match goes.
    await page.locator("[data-all-boards-page] [data-list-search]").fill("relaunch");
    await expect(list.locator("[data-board-item]")).toHaveCount(1);
    await expect(list.locator("[data-all-boards-project]")).toHaveCount(1);
    await page.locator("[data-all-boards-page] [data-list-search]").fill("nothing like this");
    await expect(page.locator("[data-all-boards-page] [data-boards-filter-none]")).toBeVisible();
  });

  test("a board's link selects its row, a Done one's too", async ({ page }) => {
    await openApp(page, "/boards/brd_0a05?all=1");
    await expect(item(page, "brd_0a05")).toHaveAttribute("aria-selected", "true");
    await expect(page.locator('[data-list-preview] [data-board-page="brd_0a05"]')).toBeVisible();
  });

  test("a link to a board in none of the projects says so", async ({ page }) => {
    await openApp(page, "/boards/brd_0f0f?all=1");
    await expect(page.locator('[data-list-preview] [data-all-boards-missing="brd_0f0f"]')).toBeVisible();
    await page.locator("[data-list-preview] [data-boards-back]").click();
    await expect(page).toHaveURL(/\/boards\?all=1$/);
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the list is the page, and a board opens in its place", async ({ page }) => {
    await openApp(page, "/boards?all=1");
    await expect(item(page, "brd_0b01")).toBeVisible();
    await expect(page.locator("[data-list-preview]")).toBeHidden();
    await item(page, "brd_0b01").click();
    await expect(page.locator('[data-board-page="brd_0b01"]')).toBeVisible();
    await page.locator("[data-list-back]").click();
    await expect(item(page, "brd_0b01")).toBeVisible();
  });

  test("a board's link opens that board", async ({ page }) => {
    await openApp(page, "/boards/brd_0b01?all=1");
    await expect(page.locator('[data-board-page="brd_0b01"] [data-board-row="row_brd_0b01"]')).toBeVisible();
  });
});
