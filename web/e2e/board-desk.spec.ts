// A board's own page (Lift 5a, Arugula #1481): the desk. On the left two
// thirds its tabs (the board's own, then what's opened beside it: Lift 5b,
// place-layout.spec.ts),
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
  doing: [row("row_1448", "doing", 1448, "Row hover shows actions", { ask: ask("ask_1", true, "Truncate the middle, or hide the project?", ["Truncate middle", "Hide project"]), attachments: [{ id: "att_w", role: "work", pointer: { kind: "block", machine: "m_geek", pane: "%3" }, block: { state: "working", title: "claude@geek" } }] })],
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

test("the board fills its tab body to the right panel, as a Doc tab does, at 1440, 2000 and 2560 wide (#1558)", async ({ page }, info) => {
  await openDesk(page);
  for (const width of [1440, 2000, 2560]) {
    await page.setViewportSize({ width, height: 900 });
    const main = (await page.locator(".board-desk-main").boundingBox())!;
    const board = (await page.locator(`.board-desk-main > [data-board="${B}"]`).boundingBox())!;
    // Its scroller meets the card's edge (inside its 1px border), so no dead column and its scrollbar by the panel.
    expect(Math.abs(board.x + board.width - (main.x + main.width - 1))).toBeLessThan(1.5);
    await page.screenshot({ path: info.outputPath(`board-desk-fill-${width}.png`) });
  }
});

test("the right third is the agent panel (#1520): ✦ alone, and the board's one tab flipping Lead 🔒 ⇄ # channel; Also leads names its other boards", async ({ page }, info) => {
  await openDesk(page);
  // The same lead (claude@dust on mch_dust) leads another board of the project.
  const other = { ...VIEW.board, id: "brd_0d02", title: "One Inbox" };
  await page.route(/\/api\/projects\/prj_[0-9a-f]+\/boards$/, (r) => r.fulfill({ json: { project: P, boards: r.request().url().includes(P) ? [VIEW.board, other] : [] } }));
  await page.reload();
  const side = page.locator(".board-desk-side");
  const tabs = side.locator("[data-kit=panel] [role=tab]");
  await expect(tabs).toHaveCount(2);
  // ✦: an icon, named for the chief of staff.
  await expect(tabs.first()).toHaveAttribute("aria-label", "Chief of staff");
  await expect(tabs.first()).toHaveText("✦");
  // The board's tab, in front: its lead, privately, with what else it leads.
  await expect(tabs.nth(1)).toHaveText("Lead 🔒");
  await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(side.locator("[data-panel-note]")).toHaveText("Also leads: One Inbox");
  await expect(side.locator("[data-board-needs]")).toBeVisible();
  await page.screenshot({ path: info.outputPath("board-panel-lead.png") });
  // ⇄ flips it to the board's channel, its thread; clicking the tab again flips it back.
  const toggle = side.locator("[data-panel-toggle]");
  await expect(toggle).toHaveText("⇄ channel");
  await toggle.click();
  await expect(tabs.nth(1)).toHaveText("# Product improvements");
  await expect(toggle).toHaveText("⇄ lead");
  await tabs.nth(1).click();
  await expect(tabs.nth(1)).toHaveText("Lead 🔒");
  // ✦ the chief of staff; the board's tab stays, with no ×.
  await tabs.first().click();
  await expect(tabs.first()).toHaveAttribute("aria-selected", "true");
  await expect(side.locator("[data-tab-x]")).toHaveCount(0);
  // No key of its own (#1534): the switch is clicked.
  const ids = await page.evaluate(() => (window as unknown as { __arugulaKeys(): string[] }).__arugulaKeys());
  expect(ids.filter((k) => /toggle|channel/.test(k))).toEqual([]);
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
  await expect(others).toContainText("· 1 for Jake ›");
  await expect(needs.locator('[data-board-ask="ask_2"]')).toHaveCount(0);
  await others.locator(".k-row-line").first().click();
  await expect(needs.locator('[data-board-ask="ask_2"]')).toContainText("asks Jake");
});

// The agents' items on the board's issues, from the project's Inbox (Arugula #1469): a to-do for you, an agent's ask for Jo.
const ITEMS = [
  { id: "prp_t1", kind: "todo", group: "decide", counts: true, title: "Rotate the CI deploy key", lines: ["Why: the deploy key expired"], from: { agent: "claude@geek", machine: "m_geek" }, subject: { type: "proposal", id: "prp_t1", url: "/inbox", project: P, issue: "iss_1622" }, answer: { via: "todo", options: ["Done", "Can't"] }, since: Date.now() - 3 * 60_000 },
  // Jo's to-do, as the cloud gives a teammate's card: what's asked is Jo's alone (no lines, no answer, no address).
  { id: "prp_j2", kind: "todo", group: "decide", counts: false, for: "acct_b", title: "A to-do", from: { agent: "claude@jo" }, subject: { type: "proposal", id: "prp_j2", url: "", project: P, issue: "iss_1443" }, since: Date.now() - 30_000 },
  { id: "ses_j:1", kind: "agent_ask", group: "decide", counts: false, for: "acct_b", title: "Which crumb width?", from: { agent: "ses_j" }, subject: { type: "session", id: "ses_j", url: `/p/${P}/chat/room_j?msg=1`, project: P, issue: "iss_1443" }, answer: { via: "session_action", options: [] }, since: Date.now() - 60_000 },
];

test("Needs you takes the agents' items on its issues: yours answered there, a teammate's neutral, folded and named; tiles say who is asked", async ({ page }, info) => {
  await page.addInitScript((items) => ((window as unknown as { __inbox: unknown }).__inbox = items), ITEMS);
  await openDesk(page);
  const needs = page.locator("[data-board-needs]");
  // Yours: your ask and your to-do.
  await expect(needs).toHaveAttribute("data-board-needs", "2");
  const todo = needs.locator('[data-board-item="prp_t1"]');
  await expect(todo).toContainText("waits on you");
  await expect(todo).toContainText("#1622");
  // A teammate's: one neutral fold each, naming them, and no row of theirs until it opens.
  const jo = needs.locator('[data-board-needs-for="Jo"]');
  await expect(jo).toContainText("· 2 for Jo ›");
  await expect(needs.locator('[data-board-needs-for="Jake"]')).toContainText("· 1 for Jake ›");
  await expect(jo.locator(".k-mark")).toHaveCount(0);
  await expect(jo).not.toHaveClass(/is-need/);
  await expect(needs.locator('[data-board-item="ses_j:1"]')).toHaveCount(0);
  await jo.locator(".k-row-line").first().click();
  const theirs = needs.locator('[data-board-item="ses_j:1"] [data-need]');
  await expect(theirs).toContainText("asks Jo");
  await expect(theirs).toHaveAttribute("data-need-for", "Jo");
  await expect(theirs).toHaveClass(/is-plain/);
  await expect(theirs.getByRole("button", { name: "Send" })).toHaveCount(0);
  // Jo's to-do: neutral, naming Jo, nothing of what's asked, and nothing to press.
  const card = needs.locator('[data-board-item="prp_j2"]');
  await card.locator(".k-row-line").first().click();
  await expect(card.locator("[data-need]")).toContainText("waits on Jo");
  await expect(card.locator("[data-need]")).toHaveClass(/is-plain/);
  await expect(card.locator(".k-row-body p, .k-row-body pre")).toHaveCount(0);
  await expect(card.locator("button, a").filter({ hasNotText: /A to-do/ })).toHaveCount(0);
  // Its mark isn't amber: yours is.
  const colour = (l: ReturnType<Page["locator"]>) => l.locator(".k-mark").first().evaluate((e) => getComputedStyle(e).color);
  expect(await colour(theirs)).not.toBe(await colour(needs.locator('[data-board-ask="ask_1"] [data-need]')));
  // The tiles name who is asked.
  await expect(page.locator('[data-board-row="row_1622"] [data-board-tile-word]')).toHaveText("waits on you");
  await expect(page.locator('[data-board-row="row_1443"] [data-board-tile-word]')).toHaveText("asks Jo");
  await page.screenshot({ path: info.outputPath("board-needs-1440.png") });
  // Your to-do answers in place, with its note.
  await todo.locator(".k-row-line").first().click();
  await todo.getByLabel("Note").fill("rotated in the vault");
  await todo.getByRole("button", { name: "Done" }).click();
  await expect.poll(() => page.evaluate(() => (window as unknown as { __inboxPosted?: unknown[] }).__inboxPosted)).toEqual([{ id: "prp_t1", choice: "Done", note: "rotated in the vault" }]);
  await page.setViewportSize({ width: 360, height: 800 });
  await needs.screenshot({ path: info.outputPath("board-needs-360.png") });
});

test("the board's own tab first; its PRs fold one row each, and + pane offers its Docs and PRs (Lift 5b: they open as panes, place-layout.spec)", async ({ page }) => {
  await openDesk(page);
  await expect(page.locator("[data-board-tabs] [role=tab]")).toHaveText(["Product improvements1"]);
  // The PR fold: one row a PR, its queue said.
  const fold = page.locator('[data-board-pr="fi_2868"]');
  await expect(fold).toContainText("#2868 Sidebar caret sits beside the crumb");
  await expect(fold).toContainText("queued");
  // GitHub's mark for its state (#1566): queued, and no "PR" word beside it.
  await expect(fold.locator('[data-pr="queued"]')).toBeVisible();
  await expect(fold.locator(".k-row-kind")).toHaveCount(0);
  await page.locator("[data-board-add-pane-menu]").click();
  await expect(page.locator('[role=menu] [data-board-add-pane="bdc_1"]')).toHaveText("Design pass");
  await expect(page.locator('[role=menu] [data-board-add-pane="fi_2868"]')).toContainText("#2868");
});

test("before the platform keeps layouts (404), a PR from the fold still opens beside the board, for this page alone", async ({ page }) => {
  await page.route(`**/api/layouts/*`, (r) => r.fulfill({ status: 404, json: { error: "not found" } }));
  await openDesk(page);
  await page.locator('[data-board-pr="fi_2868"] .k-row-line').click();
  const pr = page.locator('[data-layout-pane][data-pane-kind="pr"]');
  await expect(pr.locator('[data-board-drawer="row_1443"]')).toBeVisible();
  await expect(page.locator("[data-board-changed]")).toContainText("changed by you");
  await pr.hover();
  await pr.locator("[data-pane-close]").click();
  await expect(pr).toHaveCount(0);
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

test("one key, one action (#1535): `l` writes to the lead without the panel's `l`, `f` filters without flooring the panel", async ({ page }) => {
  await openDesk(page);
  // Here the lead's machine is out of reach, so there's no box to write in: `l` is the board's all the same.
  const panel = page.locator(".board-desk-side [data-kit=panel]").first();
  await expect(panel.getByRole("button", { name: "Full screen" })).toBeVisible();
  await page.keyboard.press("l");
  // The panel's `l` would focus the panel itself, a frame later.
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  await expect(panel).not.toBeFocused();
  await page.keyboard.press("f");
  await expect(page.locator("[data-board-find-input] input, input[data-board-find-input]").first()).toBeFocused();
  await expect(page.locator("[data-panel-floor]")).toHaveCount(0);
});

test("every project's old Boards addresses still land, untouched: /boards (and ?all=1) on a project's Boards, /boards/:board on its page", async ({ page }) => {
  await openDesk(page, [], `/boards/${B}?all=1`);
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards/${B}`));
  await expect(page.locator(`[data-board-desk="${B}"]`)).toBeVisible();
  await page.goto("/boards/brd_0f0f");
  await expect(page.locator('[data-all-boards-missing="brd_0f0f"]')).toContainText("That board isn't here.");
  await page.goto("/boards");
  await expect(page).toHaveURL(/\/p\/prj_[^/]+\/boards$/);
  await expect(page.locator("[data-boards-page]")).toBeVisible();
  // Every project's, as it was (Arugula #881): the same, with nothing touched (#1525).
  await page.goto("/boards?all=1");
  await expect(page).toHaveURL(/\/p\/prj_[^/]+\/boards$/);
  await expect(page.locator("[data-boards-page]")).toBeVisible();
  await expect(page.getByText("Loading boards")).toHaveCount(0);
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

test("j and k move between tiles, peeking each card's worker in the panel; Enter opens the card's page (#1520)", async ({ page }) => {
  await openDesk(page);
  const tabs = page.locator(".board-desk-side [data-kit=panel] [role=tab]");
  const tiles = page.locator("[data-board-row]");
  await expect(tabs).toHaveCount(2);
  await page.keyboard.press("j");
  await expect(tiles.first()).toBeFocused();
  // #1448's worker, a visitor in italics, in front.
  await expect(tiles.first()).toHaveAttribute("data-board-row", "row_1448");
  await expect(tabs).toHaveCount(3);
  await expect(tabs.nth(2)).toHaveText("#1448");
  await expect(tabs.nth(2)).toHaveClass(/is-peek/);
  await expect(tabs.nth(2)).toHaveAttribute("aria-selected", "true");
  // The next has no worker: the board's tab is in front again.
  await page.keyboard.press("j");
  await expect(tiles.nth(1)).toBeFocused();
  await expect(tabs).toHaveCount(2);
  await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("k");
  await expect(tabs).toHaveCount(3);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/issues\/iss_1448$/);
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
