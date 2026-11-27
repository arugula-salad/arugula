// The shell's blocks inside the app read as the person signed in there
// (Arugula #989): a Board block in a machine's pane draws its board, not
// "Open this machine in Arugula cloud, signed in", which it said before the
// app gave the shell its platform session. And the platform is the app's
// own page there: a board row's link and the session menu's card go to the
// app's own `/p/…` places, in place, not to a tab of their own. On the
// app's fixture page (app-frame-page.tsx), signed in as sam; the block is a
// real one of the suite's daemon, the board and issue it names made up.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { inProject, panes, reset } from "./helpers";

const P = "prj_aaaa0002";
const B = "brd_0123456789abcdef0123456789abcdef";
const ISSUE = "iss_00000000000000000000000000000001";

test.use({ permissions: ["local-network-access"] });

const row = (id: string, note: string, pointer: unknown) => ({ id, rev: 1, pointer, note, status: "none", lane: "doing", attachments: [], updated_at: 1 });
const board = {
  board: { id: B, project: P, title: "Launch", created_by: "acct_a", created_at: 1, updated_at: 1, goal: "", done_note: "", closed_by: "", progress: { done: 0, total: 0, proven: 0, review: 0, doing: 0 } },
  zones: [{ id: "z_doing", rev: 1, title: "Doing", kind: "doing", rows: [row("r_one", "Ship it", { kind: "none" }), row("r_issue", "", { kind: "issue", id: ISSUE })] }],
  groups: [],
  docs: [],
};
const issue = { id: ISSUE, project: P, number: 1, title: "Tabs vanish", body: "", state: "open", labels: [], assignees: [], author: "acct_a", created_at: 1, updated_at: 1, closed_at: null, close_reason: null };

/** A fresh pane on the daemon, `before` done to its session there, then
 * the app on the pane's machine page; the board and the issue answered
 * as the platform would (the board `view`). Counts the board's reads. */
async function openApp(page: Page, before: (pane: number) => Promise<unknown>, view: unknown = board) {
  await reset(page);
  const [pane] = await panes(page);
  await inProject(page, P, "Arugula");
  await before(pane);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  const reads = { board: 0 };
  await page.route(new RegExp(`/api/boards/${B}$`), (r) => {
    reads.board++;
    return r.fulfill({ contentType: "application/json", body: JSON.stringify(view) });
  });
  await page.route(new RegExp(`/api/issues/${ISSUE}$`), (r) => r.fulfill({ contentType: "application/json", body: JSON.stringify(issue) }));
  await page.goto(`/p/${P}/machines?pane=${pane}`);
  return { pane, reads };
}

/** The Board block beside `pane`, as "Board…" pins one. */
const pinBoard = (page: Page, pane: number) =>
  page.evaluate(([pane, board]) => window.__arugula.client.request("POST", "/api/blocks", { type: "board", config: { board }, local: true, split: pane, from_pane: pane }), [pane, B] as const);

test("a Board block in a machine's pane, in the app, shows its board", async ({ page }) => {
  const { reads } = await openApp(page, (pane) => pinBoard(page, pane));
  const block = page.locator(`[data-board-block="${B}"]`);
  await expect(block.locator('[data-board-row="r_one"]')).toBeVisible();
  await expect(page.locator("[data-board-needs-control]")).toHaveCount(0);
  // Read on the app's own origin, as the person.
  expect(reads.board).toBeGreaterThan(0);
});

test("a row's open ask in a Board block is its NeedRow, answered on the tile", async ({ page }) => {
  const ask = { id: "ask_1", who: "acct_a", who_name: "you", question: "Truncate the middle, or hide the project?", options: ["Truncate middle", "Hide project"], recommend: "Truncate middle", asked_by: "claude@geek", raised: false, mine: true, can_answer: true, created_at: Date.now() - 7 * 60_000 };
  const asking = { ...board, zones: [{ ...board.zones[0]!, rows: [{ ...board.zones[0]!.rows[0]!, ask }] }] };
  const answers: unknown[] = [];
  await page.route(`**/api/boards/${B}/asks/ask_1/answer`, (r) => (answers.push(r.request().postDataJSON()), r.fulfill({ json: asking })));
  await openApp(page, (pane) => pinBoard(page, pane), asking);
  const need = page.locator(`[data-board-block="${B}"] [data-board-row="r_one"] [data-board-ask="ask_1"] [data-need="ask"]`);
  await expect(need).toContainText("asks you");
  await need.getByRole("button", { name: /^Truncate middle · recommended/ }).click();
  await expect.poll(() => answers).toEqual([{ option: "Truncate middle", text: null }]);
});

test("a board row's link in a block goes to the issue in the app, in place", async ({ page }) => {
  await openApp(page, (pane) => pinBoard(page, pane));
  const link = page.locator(`[data-board-block="${B}"] [data-board-row="r_issue"] [data-board-item-link]`);
  // The app's own path, not control's page or another origin.
  await expect(link).toHaveAttribute("href", `/issues/${ISSUE}`);
  const tabs = page.context().pages().length;
  await page.evaluate(() => ((window as unknown as { stayed: boolean }).stayed = true));
  const entries = await page.evaluate(() => history.length);
  await link.click();
  // The app finds the issue's project and puts its full address in place.
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/issues/${ISSUE}[^/]*$`));
  expect(page.context().pages().length).toBe(tabs);
  // In place: the same page, not a reload.
  expect(await page.evaluate(() => (window as unknown as { stayed?: boolean }).stayed)).toBe(true);
  // One step pushed (the find's address replaced by the issue's, not a
  // second step), and landed before Back reads the history (Arugula #1091).
  await expect.poll(() => page.evaluate(() => history.length)).toBe(entries + 1);
  // Back climbs to the machine's pane.
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines`));
});

test("the session menu's Open #N goes to its card in the app, in place", async ({ page }) => {
  await openApp(page, () =>
    page.evaluate(
      ([project, item]) => {
        const c = window.__arugula.client;
        const session = c.session!;
        c.intent({ op: "set_session_project", session, project: { id: project, name: "Arugula" } });
        c.intent({ op: "set_session_work", session, work: { project, item }, project_name: "Arugula" });
      },
      [P, ISSUE] as const,
    ),
  );
  // #1090: the card is in the session's menu, not on the pane's bar.
  await page.locator("[data-crumb-menu]").click();
  const open = page.locator("[data-card-open]");
  await expect(open).toHaveText(new RegExp(`Open ${ISSUE}`));
  await expect(page.locator(".pane-header [data-pane-card]")).toHaveCount(0);
  const tabs = page.context().pages().length;
  await page.evaluate(() => ((window as unknown as { stayed: boolean }).stayed = true));
  await open.click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/issues/${ISSUE}[^/]*$`));
  expect(page.context().pages().length).toBe(tabs);
  // In place: the same page, not a reload.
  expect(await page.evaluate(() => (window as unknown as { stayed?: boolean }).stayed)).toBe(true);
});
