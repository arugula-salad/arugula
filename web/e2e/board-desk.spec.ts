// A board's own page (Lift 5a, Arugula #1481): the desk. On the left two
// thirds its tabs (`1`–`4`: the board, its Docs, a PR opened from the fold),
// the red alert strip (a mark and an edge, no fill), the open PRs one row
// each, and the title block (meta, lead, machine, progress); on the right
// third the board's Needs you as rows, a teammate's asks folded, above the
// lead's panel. The sidebar is gone: Filter and Add row are in the title
// block, the rest in its ⋯. On the app's frame fixture (app-frame-page.tsx),
// with the cloud's answers made up here.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { reset } from "./helpers";

const P = "prj_aaaa0002";
const B = "brd_0d01";

test.use({ permissions: ["local-network-access"], viewport: { width: 1440, height: 900 } });

const HOUR = 3_600_000;
const ask = (id: string, mine: boolean, question: string, options: string[]) => ({ id, who: mine ? "acct_a" : "acct_j", who_name: mine ? "you" : "Jake", question, options, asked_by: "claude@geek", raised: false, mine, can_answer: mine, created_at: Date.now() - 7 * 60_000 });
const row = (id: string, lane: string, number: number, title: string, more: Record<string, unknown> = {}) => ({ id, rev: 1, pointer: { kind: "issue", id: `iss_${number}` }, note: "", status: "none", lane, attachments: [], live: { state: "open", kind: "native", number, title }, created_at: 1, updated_at: 1, ...more });
const pr = { id: "att_pr", role: "work", pointer: { kind: "forge_item", id: "fi_2868" }, live: { state: "checks_running", kind: "pr", repo: null, number: 2868, title: "Sidebar caret sits beside the crumb", queued: true, queued_since: Date.now() - 2 * 60_000 } };

const ROWS = {
  next: [row("row_1622", "next", 1622, "Chat: polls, as cards")],
  doing: [row("row_1448", "doing", 1448, "Row hover shows actions", { ask: ask("ask_1", true, "Truncate the middle, or hide the project?", ["Truncate middle", "Hide project"]) })],
  review: [row("row_1443", "review", 1443, "Sidebar caret beside the crumb", { attachments: [pr] }), row("row_1416", "review", 1416, "Run this board card", { ask: ask("ask_2", false, "What should the card say?", []) })],
  done: [1444, 1445, 1446, 1447].map((n) => row(`row_${n}`, "done", n, `Done ${n}`, { live: { state: "merged", kind: "pr", number: n, title: `Done ${n}` }, proof: n === 1444 ? { by: "ci" } : undefined })),
};

const VIEW = {
  board: { id: B, project: P, title: "Product improvements", goal: "Make Arugula better to use.", created_by: "acct_a", created_at: 1, updated_at: 1, done_note: "", closed_by: "", progress: { done: 4, total: 8, proven: 1, review: 2, doing: 1 }, lead: { machine: "mch_dust", machine_name: "dust", session: "claude@dust", at: 1 } },
  zones: (["next", "doing", "review", "done"] as const).map((lane) => ({ id: `lane_${lane}`, rev: 1, title: lane[0].toUpperCase() + lane.slice(1), kind: "lane", rows: ROWS[lane] })),
  groups: [],
  docs: [{ id: "bdc_1", pointer: { kind: "doc", id: "pdc_design" }, title: "Design pass", sections: 3, written: 3, added_by: "acct_a", created_at: 1 }],
};

/** The cloud's answers, then the board's page. `answers` hears each answer to an ask. */
async function openDesk(page: Page, answers: unknown[] = [], path = `/p/${P}/boards/${B}`) {
  await reset(page);
  // Only this project has boards.
  await page.route(/\/api\/projects\/prj_[0-9a-f]+\/boards$/, (r) => r.fulfill({ json: { project: P, boards: r.request().url().includes(P) ? [VIEW.board] : [] } }));
  await page.route(`**/api/boards/${B}`, (r) => r.fulfill({ json: VIEW }));
  await page.route(`**/api/boards/${B}/asks/*/answer`, (r) => (answers.push(r.request().postDataJSON()), r.fulfill({ json: VIEW })));
  await page.route(`**/api/projects/${P}/ci-alerts`, (r) => r.fulfill({ json: { project: P, on: true, url: "", people: [], assign: false, key_made_by: null, key_made_at: null, last_event_at: null, open: [{ branch: "main", issue: "iss_red", number: 77, opened_at: Date.now() - 3 * HOUR - 60_000 }] } }));
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => r.fulfill({ json: { project: P, groups: [] } }));
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route((u) => u.pathname.startsWith("/boards") || u.pathname.startsWith("/p/"), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
  if (path.startsWith("/p/")) await expect(page.locator(`[data-board-desk="${B}"]`)).toBeVisible();
}

test("the board on the left two thirds, its Needs you and lead on the right third; no sidebar", async ({ page }, info) => {
  await openDesk(page);
  const main = (await page.locator(".board-desk-main").boundingBox())!;
  const side = (await page.locator(".board-desk-side").boundingBox())!;
  expect(side.x).toBeGreaterThan(main.x + main.width - 1);
  expect(main.width).toBeGreaterThan(side.width * 1.6);
  await expect(page.locator("[data-board-aside]")).toHaveCount(0);
  // The crumb climbs: Boards, then the board.
  await expect(page.locator('[data-path="board"]')).toHaveText("Product improvements");
  // Its asks are in Needs you, not on its tiles.
  await expect(page.locator(`[data-board="${B}"] .board-ask`)).toHaveCount(0);
  // The title block: the meta line, the lead and its machine, the progress line.
  await expect(page.locator("[data-board-heading]")).toHaveText("Product improvements");
  await expect(page.locator("[data-board-meta]")).toContainText("4/8");
  await expect(page.locator("[data-board-meta]")).toContainText("1 asks you");
  await expect(page.locator("[data-board-meta]")).toContainText("#2868 queued");
  await expect(page.locator("[data-board-lead-facts]")).toContainText("dust");
  await expect(page.locator("[data-board-progress-words]")).toHaveText("50% · 4 of 8 done · 1 proven · 2 in review");
  await page.screenshot({ path: info.outputPath("board-desk-1440.png") });
});

test("the red alert strip says how long main has been red, with a mark and an edge and no fill", async ({ page }) => {
  await openDesk(page);
  const strip = page.locator('[data-board-alert="main"]');
  await expect(strip).toContainText("main has been red for 3 hours");
  await expect(strip.locator('[data-state="failing"]')).toBeVisible();
  await expect(strip).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  await expect(strip.locator("[data-board-alert-issue]")).toHaveText("Open #77");
});

test("Needs you: your ask is a row with its options, a teammate's folded under one row", async ({ page }) => {
  const answers: unknown[] = [];
  await openDesk(page, answers);
  const needs = page.locator("[data-board-needs]");
  await expect(needs).toHaveAttribute("data-board-needs", "1");
  const mine = needs.locator('[data-board-ask="ask_1"]');
  await expect(mine).toContainText("Truncate the middle, or hide the project?");
  await expect(mine).toContainText("asks you");
  await mine.getByRole("button", { name: "Truncate middle" }).click();
  await expect.poll(() => answers).toEqual([{ option: "Truncate middle", text: null }]);
  // Jake's is folded: one row says so, and opens to his.
  const others = needs.locator("[data-board-needs-others]");
  await expect(others).toContainText("1 ask for Jake");
  await expect(needs.locator('[data-board-ask="ask_2"]')).toHaveCount(0);
  await others.locator(".k-row-line").first().click();
  await expect(needs.locator('[data-board-ask="ask_2"]')).toContainText("asks Jake");
});

test("tabs: 1 is the board, 2 its Doc; a PR from the fold opens as a tab that closes", async ({ page }) => {
  await openDesk(page);
  const tabs = page.locator("[data-board-tabs] [role=tab]");
  await expect(tabs).toHaveText(["Board", "Design pass"]);
  await page.keyboard.press("2");
  await expect(page.locator('[data-board-tab-doc="bdc_1"]')).toBeVisible();
  await expect(page.locator(`[data-board="${B}"]`)).toHaveCount(0);
  await page.keyboard.press("1");
  await expect(page.locator(`[data-board="${B}"]`)).toBeVisible();
  // The PR fold: one row a PR, its queue said.
  const fold = page.locator('[data-board-pr="fi_2868"]');
  await expect(fold).toContainText("#2868 Sidebar caret sits beside the crumb");
  await expect(fold).toContainText("queued");
  await fold.locator(".k-row-line").click();
  await expect(tabs).toHaveText(["Board", "Design pass", "#2868"]);
  await expect(page.locator('[data-board-tab-pr="fi_2868"] [data-board-drawer="row_1443"]')).toBeVisible();
  await page.getByRole("button", { name: "Close #2868" }).click();
  await expect(tabs).toHaveText(["Board", "Design pass"]);
  await expect(page.locator(`[data-board="${B}"]`)).toBeVisible();
});

test("Filter (f) shows the find bar; Add row (a) its sheet; the sidebar's adds are in ⋯", async ({ page }) => {
  await openDesk(page);
  await expect(page.locator("[data-board-find]")).toHaveCount(0);
  await page.keyboard.press("f");
  await expect(page.locator("[data-board-find-input] input, input[data-board-find-input]").first()).toBeFocused();
  await page.keyboard.press("Escape");
  await page.locator("body").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("a");
  await expect(page.getByRole("dialog", { name: "Add row" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Add row" })).toHaveCount(0);
  await page.locator("[data-board-more]").click();
  for (const hook of ["[data-board-add-existing]", "[data-board-doc-add]", "[data-board-add-group]", "[data-board-workers]", "[data-copy-link]"]) await expect(page.locator(`[role=menu] ${hook}`)).toBeVisible();
});

test("every project's old Boards addresses still land: /boards on a project's Boards, /boards/:board on its page", async ({ page }) => {
  await openDesk(page, [], `/boards/${B}?all=1`);
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards/${B}`));
  await expect(page.locator(`[data-board-desk="${B}"]`)).toBeVisible();
  await page.goto("/boards/brd_0f0f");
  await expect(page.locator('[data-all-boards-missing="brd_0f0f"]')).toContainText("That board isn't here.");
  await page.goto("/boards");
  await expect(page).toHaveURL(/\/p\/prj_[^/]+\/boards$/);
  await expect(page.locator("[data-boards-page]")).toBeVisible();
});

test("lanes read Next · Doing · Review · Done; a tile says its state and who it waits on, with no fill; Done folds to N more", async ({ page }, info) => {
  await openDesk(page);
  const lanes = page.locator(`[data-board="${B}"] [data-board-zone]`);
  const x = async (id: string) => (await page.locator(`[data-board-zone="${id}"]`).boundingBox())!.x;
  const xs = await Promise.all(["lane_next", "lane_doing", "lane_review", "lane_done"].map(x));
  expect([...xs].sort((a, b) => a - b)).toEqual(xs);
  await expect(lanes).toHaveCount(4);
  // Yours: its state word, an amber edge, no fill.
  const mine = page.locator('[data-board-row="row_1448"]');
  await expect(mine.locator("[data-board-tile-word]")).toHaveText("asks you");
  await expect(mine).toHaveAttribute("data-needs-you", "");
  await expect(mine).toHaveCSS("border-left-width", "2px");
  // A teammate's, who it waits on; the PR's, where its PR is.
  await expect(page.locator('[data-board-row="row_1416"] [data-board-tile-word]')).toHaveText("asks Jake");
  await expect(page.locator('[data-board-row="row_1443"] [data-board-tile-meta]')).toContainText("PR #2868 · queued");
  await expect(page.locator('[data-board-row="row_1622"] [data-board-tile-meta]')).toHaveText("no agent yet");
  // No tile is tinted by its lane, and none says "no proof".
  const fills = await page.locator("[data-board-row]").evaluateAll((els) => [...new Set(els.map((e) => getComputedStyle(e).backgroundColor))]);
  expect(fills).toHaveLength(1);
  await expect(page.locator(`[data-board="${B}"]`)).not.toContainText("no proof");
  // Done shows two, then 2 more.
  const done = page.locator('[data-board-zone="lane_done"]');
  await expect(done.locator("[data-board-row]")).toHaveCount(2);
  await expect(done.locator("[data-board-row]").first().locator("[data-board-tile-meta]")).toHaveText("merged · proven");
  await page.screenshot({ path: info.outputPath("board-lanes-1440.png") });
  await done.locator("[data-board-lane-more]").click();
  await expect(done.locator("[data-board-row]")).toHaveCount(4);
});

test("j and k move between tiles; Enter opens one", async ({ page }) => {
  await openDesk(page);
  await page.keyboard.press("j");
  await expect(page.locator("[data-board-row]").first()).toBeFocused();
  await page.keyboard.press("j");
  await expect(page.locator("[data-board-row]").nth(1)).toBeFocused();
  await page.keyboard.press("k");
  await expect(page.locator("[data-board-row]").first()).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-board-drawer]")).toBeVisible();
});

test("Add row is a sheet: a title, a description and Start an agent on it; it opens the issue, puts it on the board and asks for its agent", async ({ page }, info) => {
  await openDesk(page);
  const made = { id: `iss_${"9".padStart(32, "0")}`, project: P, number: 1700, title: "Tabs keep their order", body: "", state: "open", close_reason: null, author: "acct_a", agent: null, labels: [], assignees: [], github: null, created_at: 1, updated_at: 1, closed_at: null };
  const calls: string[] = [];
  await page.route(`**/api/projects/${P}/issues`, (r) => (calls.push(`issue ${r.request().postDataJSON().title}`), r.fulfill({ json: made })));
  await page.route(`**/api/boards/${B}/entries`, (r) => (calls.push(`row ${r.request().postDataJSON().pointer.id}`), r.fulfill({ json: VIEW })));
  await page.route(`**/api/issues/${made.id}/agent`, (r) => (calls.push("agent"), r.fulfill({ json: { started: true } })));
  await page.locator("[data-board-add-row]").click();
  const sheet = page.getByRole("dialog", { name: "Add row" });
  await sheet.locator("[data-add-row-title]").fill("Tabs keep their order");
  await sheet.locator("[data-add-row-body]").fill("Across a reload.");
  await sheet.getByRole("switch", { name: "Start an agent on it" }).click();
  await page.screenshot({ path: info.outputPath("add-row.png") });
  await sheet.locator("[data-add-row-add]").click();
  await expect.poll(() => calls).toEqual(["issue Tabs keep their order", `row ${made.id}`, "agent"]);
  await expect(sheet).toHaveCount(0);
});
