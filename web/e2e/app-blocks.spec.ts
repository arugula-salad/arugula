// The shell's blocks inside the app read as the person signed in there
// (Arugula #989): a Board block in a machine's pane draws its board, not
// "Open this machine in Arugula cloud, signed in", which it said before the
// app gave the shell its platform session. And the platform is the app's
// own page there: a board row's link and the pane's card chip go to the
// app's own `/p/…` places, in place, not to a tab of their own. On the
// app's fixture page (app-frame-page.tsx), signed in as sam; the block is a
// real one of the suite's daemon, the board and issue it names made up.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

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
 * as the platform would. Counts the board's reads. */
async function openApp(page: Page, before: (pane: number) => Promise<unknown>) {
  await reset(page);
  const [pane] = await panes(page);
  await before(pane);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  const reads = { board: 0 };
  await page.route(new RegExp(`/api/boards/${B}$`), (r) => {
    reads.board++;
    return r.fulfill({ contentType: "application/json", body: JSON.stringify(board) });
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

test("a board row's link in a block goes to the issue in the app, in place", async ({ page }) => {
  await openApp(page, (pane) => pinBoard(page, pane));
  const link = page.locator(`[data-board-block="${B}"] [data-board-row="r_issue"] [data-board-item-link]`);
  // The app's own path, not control's page or another origin.
  await expect(link).toHaveAttribute("href", `/issues/${ISSUE}`);
  const tabs = page.context().pages().length;
  await page.evaluate(() => ((window as unknown as { stayed: boolean }).stayed = true));
  await link.click();
  // The app finds the issue's project and puts its full address in place.
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/issues/${ISSUE}[^/]*$`));
  expect(page.context().pages().length).toBe(tabs);
  // In place: the same page, not a reload.
  expect(await page.evaluate(() => (window as unknown as { stayed?: boolean }).stayed)).toBe(true);
  // Back climbs to the machine's pane.
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines`));
});

test("the pane's card chip goes to its card in the app, in place", async ({ page }) => {
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
  const chip = page.locator(`[data-pane-card="${ISSUE}"]`);
  await expect(chip).toBeVisible();
  const tabs = page.context().pages().length;
  await page.evaluate(() => ((window as unknown as { stayed: boolean }).stayed = true));
  await chip.click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/issues/${ISSUE}[^/]*$`));
  expect(page.context().pages().length).toBe(tabs);
  // In place: the same page, not a reload.
  expect(await page.evaluate(() => (window as unknown as { stayed?: boolean }).stayed)).toBe(true);
});
