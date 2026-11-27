// A project's Boards on the list page (Arugula #1128): a row a board, its
// progress as `13/18` and the lanes' bar; From Plane and Done (folded) as
// groups; a board opens on its own page (Lift 5a), so an old board link
// opens it; New board beside the title; Not on a board the pinned first row
// (Arugula #1130), its issues in the middle, Order and the labels with their
// counts on the right, and one dragged onto a board's row goes on that
// board; on a phone the list is the page and Not on a board its first row.
// A rename, close or reopen on the board's page moves its row (Arugula
// #1159); a row's drawer is a sheet; on a phone Escape on a row's menu stays
// on the board (Arugula #1160). On the app's frame fixture
// (app-frame-page.tsx), with the cloud's answers made up here.
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { reset } from "./helpers";

const P = "prj_aaaa0002";

test.use({ permissions: ["local-network-access"] });

const progress = (done: number, total: number, review = 0, doing = 0) => ({ done, total, proven: 0, review, doing });
const board = (id: string, title: string, p: ReturnType<typeof progress>, more: Record<string, unknown> = {}) => ({ id, project: P, title, goal: `${title}'s goal`, created_by: "acct_a", created_at: 1, updated_at: 1, done_note: "", closed_by: "", progress: p, ...more });
const plane = (key: string, started: boolean) => ({ source: { provider: "plane", key, url: "", connection: "ARUGU", left_out: 0, started } });

const BOARDS = [
  // Launch was touched last, Someday made last: Updated puts Launch first, Created Someday.
  board("brd_0a01", "Launch", progress(13, 18, 2, 1), { created_at: 10, updated_at: 30 }),
  board("brd_0a02", "Someday", progress(0, 0), { created_at: 20, updated_at: 15 }),
  board("brd_0a03", "Epic: search", progress(2, 5, 1, 0), plane("ARUGU-10", true)),
  board("brd_0a04", "Epic: billing", progress(0, 3), plane("ARUGU-11", false)),
  board("brd_0a05", "Last year", progress(4, 4), { closed_at: 2, done_note: "Shipped." }),
];

const row = (id: string, lane: string, number: number, title: string) => ({ id, rev: 1, pointer: { kind: "issue", id: `iss_${number}` }, note: "", status: "none", lane, attachments: [], live: { state: "open", kind: "native", number, title }, created_at: 1, updated_at: 1 });
/** A board's rows: one in Doing; Epic: search has a group, Phase 1, with that row in it and a second row in none. */
const view = (b: (typeof BOARDS)[number]) => {
  const grouped = b.id === "brd_0a03";
  const rows = (lane: string) =>
    lane !== "doing" ? [] : [{ ...row(`row_${b.id}`, lane, 900, `${b.title}'s first issue`), group: grouped ? "zone_g1" : null }, ...(grouped ? [row("row_loose", lane, 901, "Loose issue")] : [])];
  return {
    board: b,
    zones: (["next", "doing", "review", "done"] as const).map((lane) => ({ id: `lane_${lane}`, rev: 1, title: lane[0].toUpperCase() + lane.slice(1), kind: "lane", rows: rows(lane) })),
    groups: grouped ? [{ id: "zone_g1", rev: 1, title: "Phase 1", progress: progress(0, 1) }] : [],
    docs: [],
  };
};

const issue = (number: number, title: string) => ({ id: `iss_${String(number).padStart(32, "0")}`, project: P, number, title, body: "", state: "open", close_reason: null, author: "acct_a", agent: null, labels: [], assignees: [], github: null, created_at: 1, updated_at: 1, closed_at: null });
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
  await page.route(`**/api/projects/${P}/issue-home`, (r) => r.fulfill({ json: { project: P, name: "Arugula", home: "native", chosen: true, repos: [], native_issues: 3, both: false, labels: [], team: true } }));
  await page.route(new RegExp(`/api/projects/${P}/issues\\?`), (r) => r.fulfill({ json: { project: P, issues: [issue(41, "Flaky login"), issue(42, "Dark mode"), issue(43, "On a board")], more: false } }));
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
}

/** A board's address, with or without its title's slug. */
const at = (id: string) => new RegExp(`/p/${P}[^/]*/boards/${id}(-[^/]*)?$`);

const item = (page: Page, id: string) => page.locator(`[data-board-item="${id}"]`);

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("boards sort by Updated, newest first, or by Created; the choice stays (Arugula #1369)", async ({ page }) => {
    await openApp(page, `/p/${P}/boards`);
    const titles = () => page.locator("[data-boards-page] [data-board-item] [data-board-title]").allTextContents();
    const sort = page.locator("[data-boards-page] [data-list-sort]");
    await expect(sort).toHaveValue("updated");
    await expect.poll(titles).toEqual(["Launch", "Someday", "Epic: search", "Epic: billing"]);
    await sort.selectOption("created");
    await expect.poll(titles).toEqual(["Someday", "Launch", "Epic: search", "Epic: billing"]);
    expect(await page.evaluate(() => localStorage.getItem("boards-sort"))).toBe("created");
    await page.reload();
    await expect(sort).toHaveValue("created");
    await expect.poll(titles).toEqual(["Someday", "Launch", "Epic: search", "Epic: billing"]);
  });

  test("boards are rows; one opens on its own page", async ({ page }) => {
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
    // The search filters the rows.
    await page.locator("[data-list-search]").fill("launch");
    await expect(list.locator("[data-board-item]")).toHaveCount(1);
    await page.locator("[data-list-search]").fill("");
    // A board opens on its own page (Lift 5a), its address the board's.
    await item(page, "brd_0a03").click();
    await expect(page).toHaveURL(at("brd_0a03"));
    await expect(page.locator('[data-board-desk="brd_0a03"] [data-board-row="row_brd_0a03"]')).toBeVisible();
  });

  test("an old board link opens its page, a Done one's too", async ({ page }) => {
    await openApp(page, `/p/${P}/boards/brd_0a05`);
    await expect(page.locator('[data-board-desk="brd_0a05"] [data-board-done]')).toBeVisible();
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

  test("Not on a board is the first row; its issues fill the middle, and one dragged onto a board's row goes on it", async ({ page }) => {
    const added: string[] = [];
    await openApp(page, `/p/${P}/boards`, added);
    const loose = page.locator("[data-list-rows] [data-boards-loose]");
    await expect(page.locator("[data-list-rows] [role=option]").first()).toHaveAttribute("data-boards-loose", "");
    await expect(loose.locator("[data-boards-loose-n]")).toHaveText("2");
    // Pinned: a search that hides every board keeps it.
    await page.locator("[data-list-search]").fill("nothing like this");
    await expect(loose).toBeVisible();
    await page.locator("[data-list-search]").fill("");
    await loose.click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/triage$`));
    await expect(loose).toHaveAttribute("aria-selected", "true");
    const pane = page.locator("[data-list-preview] [data-boards-loose-pane]");
    await expect(pane.locator("[data-triage-row]")).toHaveCount(2);
    // Order and the labels, with their counts, on the right.
    await expect(pane.locator(".list-preview-aside [data-triage-order]")).toBeVisible();
    await expect(pane.locator('.list-preview-aside [data-triage-label="ui"]')).toContainText("2");
    await pane.locator('[data-triage-row="#41"]').dragTo(item(page, "brd_0a01"));
    await expect.poll(() => added).toEqual(["brd_0a01"]);
    // A board's row goes back to the board, and Back to Not on a board.
    await item(page, "brd_0a03").click();
    await expect(page).toHaveURL(at("brd_0a03"));
    await page.goBack();
    await expect(page).toHaveURL(/\/triage$/);
    await expect(loose).toHaveAttribute("aria-selected", "true");
  });

  test("All issues is a row; the project's issues fill the middle, and the Issues page's address selects it (Lift 5a)", async ({ page }) => {
    await openApp(page, `/p/${P}/boards`);
    const all = page.locator("[data-list-rows] [data-boards-all]");
    await expect(all.locator("[data-boards-all-n]")).toHaveText("3");
    await all.click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/issues$`));
    await expect(all).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("[data-list-preview] [data-issues-list] [data-issue-row]")).toHaveCount(3);
    await expect(page.locator('[data-path="issues"]')).toHaveText("All issues");
    await page.goto(`/p/${P}/issues?state=closed`);
    await expect(page.locator("[data-list-rows] [data-boards-all]")).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("[data-list-preview] [data-issues-closed]")).toHaveAttribute("aria-pressed", "true");
  });

  test("New issue's old address opens All issues with the New issue sheet (Lift 5a)", async ({ page }) => {
    await openApp(page, `/p/${P}/issues/new`);
    await expect(page.locator("[data-list-rows] [data-boards-all]")).toHaveAttribute("aria-selected", "true");
    const sheet = page.getByRole("dialog", { name: "New issue" });
    await expect(sheet.locator("[data-add-row-title]")).toBeFocused();
    await expect(sheet.locator("[data-add-row-add]")).toHaveText("Open issue");
  });

  test("Not on a board shows only while it has issues", async ({ page }) => {
    await openApp(page, `/p/${P}/boards`);
    await page.route(new RegExp(`/api/projects/${P}/work\\?`), (r) => r.fulfill({ json: { project: P, items: [], next: null, total: 0, labels: [] } }));
    await page.reload();
    await expect(page.locator("[data-list-rows] [data-boards-all]")).toBeVisible();
    await expect(page.locator("[data-list-rows] [data-boards-loose]")).toHaveCount(0);
  });

  test("the triage link selects Not on a board's row", async ({ page }) => {
    await openApp(page, `/p/${P}/triage`);
    await expect(page.locator("[data-list-rows] [data-boards-loose]")).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("[data-list-preview] [data-boards-loose-pane] [data-triage-row]")).toHaveCount(2);
  });
});

// Arugula #1159: a write in the middle that changes what the board's row
// shows (its title, whether it's closed) moves the row, without a reload.
test.describe("the list follows the board's page", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("renamed on its page, the crumb and its row say so; closed, its row goes to Done; reopened, back", async ({ page }) => {
    await openApp(page, `/p/${P}/boards/brd_0a01`);
    // Launch as the platform has it now: each write changes it, and the list reads it.
    const launch: Record<string, unknown> = { ...BOARDS[0] };
    const now = () => launch as (typeof BOARDS)[number];
    await page.route(`**/api/projects/${P}/boards`, (r) => (r.request().method() === "GET" ? r.fulfill({ json: { project: P, boards: BOARDS.map((b) => (b.id === "brd_0a01" ? now() : b)) } }) : r.fallback()));
    await page.route(/\/api\/boards\/brd_0a01(\/close|\/reopen)?$/, (r) => {
      const url = r.request().url();
      if (url.endsWith("/close")) Object.assign(launch, { closed_at: 3, done_note: r.request().postDataJSON().done_note });
      else if (url.endsWith("/reopen")) delete launch.closed_at;
      else if (r.request().method() === "PATCH") Object.assign(launch, r.request().postDataJSON());
      return r.fulfill({ json: view(now()) });
    });
    const desk = page.locator('[data-board-desk="brd_0a01"]');
    await expect(desk.locator('[data-board-row="row_brd_0a01"]')).toBeVisible();

    await desk.locator("[data-board-more]").click();
    await page.locator("[data-board-rename]").click();
    await page.locator(".prompt input").fill("Launch 1.0");
    await page.locator(".prompt input").press("Enter");
    await expect(page.locator('[data-path="board"]')).toHaveText("Launch 1.0");

    await desk.locator("[data-board-more]").click();
    await page.locator("[data-board-close]").click();
    await desk.locator("[data-board-close-note]").fill("Shipped.");
    await desk.locator("[data-board-close-save]").click();
    await expect(desk.locator("[data-board-reopen]")).toBeVisible();
    await page.locator('[data-path="boards"]').click();
    await page.locator("[data-boards-done] [data-list-group-fold]").click();
    await expect(page.locator('[data-boards-done] [data-board-item="brd_0a01"]')).toBeVisible();
    await expect(page.locator("[data-boards-done] .list-group-n")).toHaveText("2");
    await expect(page.locator('[data-boards-done] [data-board-item="brd_0a01"] [data-board-title]')).toHaveText("Launch 1.0");

    await page.locator('[data-boards-done] [data-board-item="brd_0a01"]').click();
    await desk.locator("[data-board-reopen]").click();
    await page.locator('[data-path="boards"]').click();
    await expect(page.locator('[data-boards-done] [data-board-item="brd_0a01"]')).toHaveCount(0);
    await expect(item(page, "brd_0a01")).toBeVisible();
    await expect(page.locator("[data-boards-done] .list-group-n")).toHaveText("1");
  });
});

test.describe("on a board's page (Lift 5a)", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("a row's drawer is a sheet over the board; Esc shuts it and the row has the focus", async ({ page }) => {
    await openApp(page, `/p/${P}/boards/brd_0a01`);
    await page.locator('[data-board-row="row_brd_0a01"] [data-board-ptr]').click();
    const drawer = page.locator('[data-side-sheet-backdrop] [data-board-drawer="row_brd_0a01"]');
    await expect(drawer).toBeVisible();
    await expect(drawer.locator("[data-board-drawer-title]")).toHaveText("Launch's first issue");
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-board-drawer]")).toHaveCount(0);
    await expect(page.locator('[data-board-row="row_brd_0a01"]')).toBeFocused();
  });

  test("a grouped board has a Group menu under Filter, and it narrows the rows", async ({ page }) => {
    await openApp(page, `/p/${P}/boards/brd_0a03`);
    await expect(page.locator("[data-board-row]")).toHaveCount(2);
    await page.locator("[data-board-filter]").click();
    await page.locator('[data-board-facet-button="group"]').click();
    await page.locator('[data-board-facet-menu="group"] [data-board-facet-value="zone_g1"]').click();
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-board-row]")).toHaveCount(1);
    await expect(page.locator('[data-board-row="row_brd_0a03"]')).toBeVisible();
    await expect(page.locator("[data-board-find-count]")).toHaveText("1 of 2 rows");
    await expect(page).toHaveURL(/#group=zone_g1$/);
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the list is the page, and a board opens on its own", async ({ page }) => {
    await openApp(page, `/p/${P}/boards`);
    await expect(item(page, "brd_0a01")).toBeVisible();
    await expect(page.locator("[data-list-preview]")).toBeHidden();
    await item(page, "brd_0a01").click();
    await expect(page.locator('[data-board-page="brd_0a01"]')).toBeVisible();
    await page.goBack();
    await expect(item(page, "brd_0a01")).toBeVisible();
  });

  test("Not on a board is the first row, and its issues open in the list's place", async ({ page }) => {
    await openApp(page, `/p/${P}/boards`);
    const loose = page.locator("[data-list-rows] [data-boards-loose]");
    await expect(page.locator("[data-list-rows] [role=option]").first()).toHaveAttribute("data-boards-loose", "");
    await loose.click();
    await expect(page.locator('[data-boards-loose-pane] [data-triage-row="#41"]')).toBeVisible();
    await expect(page.locator("[data-list-back]")).toBeVisible();
  });

  test("Escape on a row's menu closes the menu and stays on the board (Arugula #1160)", async ({ page }) => {
    await openApp(page, `/p/${P}/boards/brd_0a01`);
    const row = page.locator('[data-board-page="brd_0a01"] [data-board-row="row_brd_0a01"]');
    await expect(row).toBeVisible();
    await row.locator("[data-board-row-menu]").click();
    await expect(page.getByRole("menu")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("menu")).toHaveCount(0);
    await expect(row).toBeVisible();
  });

  test("a board's link opens that board", async ({ page }) => {
    await openApp(page, `/p/${P}/boards/brd_0a03`);
    await expect(page.locator('[data-board-page="brd_0a03"] [data-board-row="row_brd_0a03"]')).toBeVisible();
  });
});
