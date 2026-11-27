// Running (Lift 6a, design lift §3.9, Arugula #1483): the top bar's
// "N running" opens one drawer at the right, every live process across the
// page's machines (here the suite's daemon), the project's red branches on
// top and its CI runs after. A pane's last lines peek under the list, and
// over a board Pin to desk opens it beside the board. No key of its own
// until #1534's list. On the app's frame fixture.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

const P = "prj_aaaa0002";
const B = "brd_0123456789abcdef0123456789abcdef";

test.use({ permissions: ["local-network-access"], viewport: { width: 1440, height: 900 } });

const VIEW = {
  board: { id: B, project: P, title: "Launch", created_by: "acct_a", created_at: 1, updated_at: 1, goal: "", done_note: "", closed_by: "", progress: { done: 0, total: 0, proven: 0, review: 0, doing: 0 } },
  zones: [{ id: "lane_doing", rev: 1, title: "Doing", kind: "lane", rows: [] }],
  groups: [],
  docs: [],
};

/** A pane on the suite's daemon, then the board in the app; `posted` hears the layout changes. */
async function openBoard(page: Page, posted: unknown[] = []): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.route(new RegExp(`/api/boards/${B}$`), (r) => r.fulfill({ json: VIEW }));
  await page.route(`**/api/projects/${P}/ci-alerts`, (r) => r.fulfill({ json: { project: P, on: true, url: "", people: [], assign: false, key_made_by: null, key_made_at: null, last_event_at: null, open: [{ branch: "main", issue: "iss_red", number: 77, opened_at: Date.now() - 3 * 3_600_000 - 60_000 }] } }));
  await page.route(`**/api/projects/${P}/runs?*`, (r) => r.fulfill({ json: { project: P, runs: [{ id: "run_ci1", project: P, workspace: null, trigger: "push", source: {}, op: "", op_hash: "", daemon: "", state: "running", started_by: "acct_a", started_by_agent: null, grant: null, last_seq: 0, created_at: Date.now() - 120_000, started_at: Date.now() - 120_000, ended_at: null, exit_code: null, pane: null }] } }));
  await page.route(`**/api/layouts/${encodeURIComponent(`board:${B}`)}`, (r) => {
    if (r.request().method() === "POST") posted.push(r.request().postDataJSON());
    return r.fulfill({ status: 404, json: { error: "not here" } });
  });
  await page.goto(`/p/${P}/boards/${B}`);
  return pane;
}

test("N running in the top bar opens the drawer at the right: alerts first, the machine's live panes, then CI runs; Esc closes it", async ({ page }, info) => {
  const pane = await openBoard(page);
  const open = page.locator("[data-open-running]");
  await expect(open).toBeVisible();
  await expect(open).toContainText("running");
  await open.click();
  const drawer = page.getByRole("dialog", { name: "Running" });
  await expect(drawer.locator("[data-kit=sheet]")).toBeVisible();
  // A side sheet: the window's right edge, its height.
  const box = (await drawer.locator("[data-kit=sheet]").boundingBox())!;
  expect(Math.round(box.x + box.width)).toBe(1440);
  await expect(drawer.locator('[data-running-alert="main"]')).toContainText("main has been red for 3 hours");
  await expect(drawer.locator(`[data-running-row$=":${pane}"]`)).toHaveCount(1);
  await expect(drawer.locator('[data-running-row="run_ci1"]')).toContainText("ci run");
  // The alerts come before the machines, the runs after.
  const order = await drawer.locator("[data-running-alerts], [data-running-host], [data-running-runs]").evaluateAll((els) => els.map((e) => (e.hasAttribute("data-running-alerts") ? "alerts" : e.hasAttribute("data-running-runs") ? "runs" : "host")));
  expect(order[0]).toBe("alerts");
  expect(order.at(-1)).toBe("runs");
  await page.screenshot({ path: info.outputPath("running-drawer.png") });
  await page.keyboard.press("Escape");
  await expect(drawer).toHaveCount(0);
  await expect(open).toBeFocused();
});

test("a pane's last lines peek under the list; over a board, Pin to desk shows it beside the board", async ({ page }) => {
  const posted: unknown[] = [];
  const pane = await openBoard(page, posted);
  await page.locator("[data-open-running]").click();
  const drawer = page.getByRole("dialog", { name: "Running" });
  await drawer.locator(`[data-running-row$=":${pane}"] .k-row-line`).click();
  const peek = drawer.locator(`[data-running-peek$=":${pane}"]`);
  await expect(peek.locator("[data-kit=well]")).toBeVisible();
  await peek.locator("[data-running-pin]").click();
  await expect.poll(() => posted).toEqual([{ op: "show", pane: expect.objectContaining({ kind: "terminal", ref: `%${pane}` }), intent: "beside", tab: null, in_place_of: null }]);
  // No key of its own (#1534): nothing is bound for it.
  const ids = await page.evaluate(() => (window as unknown as { __arugulaKeys(): string[] }).__arugulaKeys());
  expect(ids.filter((k) => /running|pin/.test(k))).toEqual([]);
});
