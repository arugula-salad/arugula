// The Sessions page folded away (Arugula #1082): each card's sessions on
// its board row and in its drawer, from the hub's sessions by card (a
// computer's session put on the card, a hosted one), less what the row
// holds already and what has finished; a computer's session opens its pane
// under Machines. The Machines page has Agents only (Arugula #1346). On the
// app's frame fixture (app-frame-page.tsx), with the
// cloud's answers from sessions-fixture.ts and a real pane of the suite's
// daemon.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { inProject, panes, reset } from "./helpers";
import { PROJECT as P, byCard } from "./sessions-fixture";

/** The reader's computer, as the cloud names it: the fixture page reaches
 * `m_suite` (the suite's daemon) for a live pane. */
const M = "m_suite";

test.use({ permissions: ["local-network-access"], viewport: { width: 1280, height: 800 } });

const row = (id: string, issue: string, number: number, title: string) => ({ id, rev: 1, pointer: { kind: "issue", id: issue }, note: "", status: "none", lane: "doing", attachments: [], live: { state: "open", kind: "native", number, title }, created_at: 1, updated_at: 1 });

const board = {
  board: { id: "brd_00e2e0", project: P, title: "Launch", goal: "", created_by: "", created_at: 1, updated_at: 1, done_note: "", closed_by: "", progress: { done: 0, total: 2, proven: 0, review: 0, doing: 2 } },
  zones: (["next", "doing", "review", "done"] as const).map((lane) => ({
    id: `lane_${lane}`,
    rev: 1,
    title: lane[0].toUpperCase() + lane.slice(1),
    kind: "lane",
    rows: lane === "doing" ? [row("row_0985", "iss_985", 985, "A project's Sessions page"), row("row_0970", "iss_970", 970, "The rail")] : [],
  })),
  groups: [],
  docs: [],
};

/** A fresh pane, the cloud's answers, then the app at `path`. */
async function openApp(page: Page, path: (pane: number) => string): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  await inProject(page, P, "Arugula");
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => r.fulfill({ json: byCard(M, pane) }));
  await page.route(`**/api/projects/${P}/boards`, (r) => r.fulfill({ json: { project: P, boards: [board.board] } }));
  await page.route("**/api/boards/brd_00e2e0", (r) => r.fulfill({ json: board }));
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path(pane));
  return pane;
}

test("a card's session is on its tile, what needs you first; the drawer has them all, finished ones left out; one opens its pane", async ({ page }, info) => {
  const pane = await openApp(page, () => `/p/${P}/boards/brd_00e2e0`);
  const rail = page.locator('[data-board-row="row_0970"]');
  // The computer's session put on #970, on its tile's meta line (Lift 5a).
  await expect(rail.locator("[data-board-tile-meta]")).toContainText("rail-work · working");
  // #985's hosted session waits on you: its tile says so, with an amber edge.
  const needs = page.locator('[data-board-row="row_0985"]');
  await expect(needs).toHaveAttribute("data-needs-you", "");
  await expect(needs.locator("[data-board-tile-word]")).toHaveText("needs you");
  await expect(needs.locator("[data-board-tile-meta]")).toContainText("needs input");
  await page.screenshot({ path: info.outputPath("board-card-sessions.png") });
  // The drawer says them too.
  await rail.locator("[data-board-ptr]").click();
  const drawer = page.locator('[data-board-drawer="row_0970"] [data-board-drawer-card-sessions]');
  await expect(drawer.locator("[data-board-card-session]")).toHaveCount(2);
  // A computer's session opens its pane, in Build.
  await drawer.locator(`[data-board-card-session="machine:${M}:7"] [data-board-card-session-open]`).click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]+[^/?]*)?\\?pane=${pane}$`));
  await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
});

test("the Sessions page's old address lands on Boards, slug and all, and stays there", async ({ page }) => {
  // Its move and the slug's rewrite were two effects of one render: when
  // the effects ran in the frame after it, before any other news re-drew the
  // page, the rewrite ran second and put /sessions back (Arugula #1155).
  // Run them that soon here, every time.
  await page.addInitScript(() => {
    window.requestAnimationFrame = (f) => (queueMicrotask(() => f(performance.now())), 1);
  });
  await openApp(page, () => `/p/${P}/sessions`);
  await expect(page).toHaveURL(new RegExp(`/p/${P}-arugula/boards$`));
  await expect(page.locator('[data-board-item="brd_00e2e0"]')).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`/p/${P}-arugula/boards$`));
});

test("a session's state shows on its row as it moves, without a reload: its live event reads it again", async ({ page }, info) => {
  // #985's hosted session, from the by-card list; #970's own, from the board's read.
  let hub = "working";
  let own = "working";
  const answer = (pane: number) => {
    const a = byCard(M, pane);
    const s = a.groups[1].sessions[0];
    if (s.kind === "hosted") s.state = hub;
    return a;
  };
  const held = () => ({
    ...board,
    zones: board.zones.map((z) => ({ ...z, rows: z.rows.map((r) => (r.id === "row_0970" ? { ...r, sessions: [{ session: "ses_0970eeee", state: own, live: true, branch: "arugula/0970eeee-rail" }] } : r)) })),
  });
  const pane = await openApp(page, () => `/p/${P}/boards/brd_00e2e0`);
  await page.route(`**/api/projects/${P}/sessions/by-card`, (r) => r.fulfill({ json: answer(pane) }));
  await page.route("**/api/boards/brd_00e2e0", (r) => r.fulfill({ json: held() }));
  await page.reload();
  const tile = page.locator('[data-board-row="row_0985"]');
  const chip = tile.locator("[data-board-tile-meta]");
  const mine = page.locator('[data-board-row="row_0970"] [data-board-tile-meta]');
  await expect(chip).toContainText("working");
  await expect(mine).toContainText("working");
  /** What the platform pushes as a session's state moves (chat's `session` event). */
  const push = (ses: string) => page.evaluate((ses) => (window as unknown as { __chat: { heard(ev: unknown): void } }).__chat.heard({ type: "session", room: "r", ses }), ses);
  // Working → needs input → done, on each row, and no reload.
  hub = own = "needs_input";
  await push("ses_0985aaaa");
  await push("ses_0970eeee");
  await expect(chip).toContainText("needs input");
  await expect(tile).toHaveAttribute("data-needs-you", "");
  await expect(mine).toContainText("needs input");
  await page.screenshot({ path: info.outputPath("board-sessions-live.png") });
  hub = own = "done";
  await push("ses_0985aaaa");
  await push("ses_0970eeee");
  // The list is read again no sooner than BY_CARD_SOON_MS after the last read.
  await expect(chip).toContainText("done", { timeout: 15_000 });
  await expect(tile).not.toHaveAttribute("data-needs-you", "");
  await expect(mine).toContainText("done");
});

test("the Machines page has Agents only; it says when there are none (Arugula #1346)", async ({ page }, info) => {
  const pane = await openApp(page, () => `/p/${P}/machines`);
  const list = page.locator("[data-machines-list]");
  await expect(list.locator(`[data-panes-pane="${pane}"]`)).toBeVisible();
  const filter = list.locator("[data-machines-agents]");
  await expect(filter).toHaveAttribute("aria-pressed", "false");
  // The suite's panes are terminals: no agent on any machine.
  await filter.click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines\\?agents$`));
  await expect(filter).toHaveAttribute("aria-pressed", "true");
  await expect(list.locator("[data-panes-pane]")).toHaveCount(0);
  await expect(list.locator("[data-machines-no-agents]")).toContainText("No agents on your machines.");
  await page.screenshot({ path: info.outputPath("machines-agents-only.png") });
  await filter.click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
  await expect(list.locator(`[data-panes-pane="${pane}"]`)).toBeVisible();
  // Every session, from the empty list, is the list unfiltered; the old address keeps the filter.
  await page.goto(`/p/${P}/machines/panes?agents`);
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines\\?agents$`));
  await page.locator("[data-machines-no-agents]").getByRole("link", { name: "Every session" }).click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
  await expect(list.locator(`[data-panes-pane="${pane}"]`)).toBeVisible();
});
