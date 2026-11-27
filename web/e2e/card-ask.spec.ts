// An issue on a board is the board's card (Lift 5a, Arugula #1481, plan
// §3.4): its row's properties (Board, Worker, Machine, Lead, Branch, PR)
// beside it, labels and assignees click to edit (no Edit buttons), the row's
// open ask as a NeedRow above Activity, answered in place, and a line in
// Activity for each answer. The crumb climbs Boards › the board › #N. An
// issue on no board is the issue as before. On the app's frame fixture
// (app-frame-page.tsx), with the cloud's answers made up here.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { reset } from "./helpers";

const P = "prj_aaaa0002";
const B = "brd_0c01";
const ISSUE = `iss_${"1443".padStart(32, "0")}`;
const LOOSE = `iss_${"77".padStart(32, "0")}`;

test.use({ permissions: ["local-network-access"], viewport: { width: 1440, height: 900 } });

const issue = (id: string, number: number, title: string) => ({ id, project: P, number, title, body: "The caret moves into the crumb.", state: "open", close_reason: null, author: "acct_a", agent: null, labels: ["ui"], assignees: [], github: null, created_at: 1, updated_at: 1, closed_at: null });

const ROW = {
  id: "row_1443",
  rev: 1,
  pointer: { kind: "issue", id: ISSUE },
  note: "",
  status: "none",
  lane: "review",
  live: { state: "open", kind: "native", number: 1443, title: "Sidebar caret beside the crumb" },
  attachments: [
    { id: "att_w", role: "work", pointer: { kind: "block", machine: "m_geek", pane: "7" }, block: { state: "working", title: "1443 sidebar caret", branch: "feat/1443-sidebar-caret" } },
    { id: "att_pr", role: "work", pointer: { kind: "forge_item", id: "fi_2868" }, live: { state: "checks_running", kind: "pr", repo: null, number: 2868, title: "Sidebar caret", queued: true, queued_since: Date.now() - 120_000 } },
  ],
  ask: { id: "ask_1", who: "acct_a", who_name: "you", question: "Crumb at 900px: truncate the middle, or hide the project?", options: ["Truncate middle", "Hide project"], asked_by: "claude@geek", raised: false, mine: true, can_answer: true, created_at: Date.now() - 7 * 60_000 },
  asked: [{ id: "ask_0", question: "Squash or merge?", answer: "Squash", asked_by: "claude@geek", answered_by: "aaron", answered_at: 5_000 }],
  created_at: 1,
  updated_at: 1,
};
const VIEW = {
  board: { id: B, project: P, title: "Product improvements", goal: "", created_by: "acct_a", created_at: 1, updated_at: 1, done_note: "", closed_by: "", progress: { done: 0, total: 1, proven: 0, review: 1, doing: 0 }, lead: { machine: "m_dust", machine_name: "dust", session: "claude@dust", at: 1 } },
  zones: [{ id: "lane_review", rev: 1, title: "Review", kind: "lane", rows: [ROW] }],
  groups: [],
  docs: [],
};

/** The cloud's answers, then the issue's page. `answers` hears each answer to an ask. */
async function openCard(page: Page, id: string, answers: unknown[] = []) {
  await reset(page);
  await page.route(`**/api/projects/${P}/boards/views`, (r) => r.fulfill({ json: [VIEW] }));
  await page.route(`**/api/boards/${B}/asks/*/answer`, (r) => (answers.push(r.request().postDataJSON()), r.fulfill({ json: VIEW })));
  await page.route(/\/api\/issues\/iss_[0-9]+$/, (r) => r.fulfill({ json: r.request().url().endsWith(ISSUE) ? issue(ISSUE, 1443, "Sidebar caret beside the crumb") : issue(LOOSE, 77, "On no board") }));
  await page.route(/\/api\/issues\/iss_[0-9]+\/events$/, (r) => r.fulfill({ json: { events: [] } }));
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(`/p/${P}/issues/${id}`);
  await expect(page.locator("[data-issue-title]")).toBeVisible();
}

test("an issue on a board is its card: the row's properties, the crumb through the board", async ({ page }, info) => {
  await openCard(page, ISSUE);
  const props = page.locator("[data-issue-props]");
  await expect(props.locator("[data-card-board]")).toContainText("Product improvements");
  await expect(props.locator("[data-card-worker]")).toContainText("1443 sidebar caret · waiting for you");
  await expect(props.locator("[data-card-lead]")).toContainText("claude@dust");
  await expect(props.locator("[data-card-branch]")).toContainText("feat/1443-sidebar-caret");
  await expect(props.locator("[data-card-pr]")).toContainText("#2868 · queued");
  await expect(page.locator('[data-path="board"]')).toHaveText("Product improvements");
  await expect(page.locator('[data-path="card"]')).toHaveText("#1443");
  await page.screenshot({ path: info.outputPath("card-1443.png") });
  // Labels are their own button: no Edit.
  await expect(props.getByRole("button", { name: "Edit", exact: true })).toHaveCount(0);
  await props.locator("[data-issue-labels-edit]").click();
  await expect(page.getByRole("menu")).toBeVisible();
});

test("the open ask is above Activity, answered in place; an answer is a line in Activity", async ({ page }) => {
  const answers: unknown[] = [];
  await openCard(page, ISSUE, answers);
  const ask = page.locator('[data-card-ask="ask_1"]');
  await expect(ask).toContainText("Open ask asks you");
  await expect(ask).toContainText("Crumb at 900px: truncate the middle, or hide the project?");
  const top = async (sel: string) => (await page.locator(sel).first().boundingBox())!.y;
  expect(await top('[data-card-ask="ask_1"]')).toBeLessThan(await top(".issue-activity-head"));
  await ask.getByRole("button", { name: "Truncate middle" }).click();
  await expect.poll(() => answers).toEqual([{ option: "Truncate middle", text: null }]);
  await expect(page.locator('[data-card-answer="ask_0"]')).toContainText("aaron answered Squash");
});

test("an issue on no board is the issue, with no card parts", async ({ page }) => {
  await openCard(page, LOOSE);
  await expect(page.locator("[data-issue-props] [data-card-board]")).toHaveCount(0);
  await expect(page.locator("[data-card-ask]")).toHaveCount(0);
  await expect(page.locator("[data-issue-labels]")).toBeVisible();
});
