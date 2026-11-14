// The shell's blocks inside the app read as the person signed in there
// (Arugula #989): a Board block in a machine's pane draws its board, not
// "Open this machine in Arugula cloud, signed in", which it said before the
// app gave the shell its platform session. On the app's fixture page
// (app-frame-page.tsx), signed in as sam; the block is a real one of the
// suite's daemon, the board it names made up.
import { expect, test } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

const P = "prj_aaaa0002";
const B = "brd_0123456789abcdef0123456789abcdef";

test.use({ permissions: ["local-network-access"] });

const board = {
  board: { id: B, project: P, title: "Launch", created_by: "acct_a", created_at: 1, updated_at: 1, goal: "", done_note: "", closed_by: "", progress: { done: 0, total: 0, proven: 0, review: 0, doing: 0 } },
  zones: [{ id: "z_doing", rev: 1, title: "Doing", kind: "doing", rows: [{ id: "r_one", rev: 1, pointer: { kind: "none" }, note: "Ship it", status: "none", lane: "doing", attachments: [], updated_at: 1 }] }],
  groups: [],
  docs: [],
};

test("a Board block in a machine's pane, in the app, shows its board", async ({ page }) => {
  await reset(page);
  const [pane] = await panes(page);
  // The block beside the pane, as "Board…" pins one.
  await page.evaluate(
    ([pane, board]) => window.__arugula.client.request("POST", "/api/blocks", { type: "board", config: { board }, local: true, split: pane, from_pane: pane }),
    [pane, B] as const,
  );
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  let read = 0;
  await page.route(new RegExp(`/api/boards/${B}$`), (r) => {
    read++;
    return r.fulfill({ contentType: "application/json", body: JSON.stringify(board) });
  });
  await page.goto(`/p/${P}/machines?pane=${pane}`);
  const block = page.locator(`[data-board-block="${B}"]`);
  await expect(block.locator('[data-board-row="r_one"]')).toBeVisible();
  await expect(page.locator("[data-board-needs-control]")).toHaveCount(0);
  // Read on the app's own origin, as the person.
  expect(read).toBeGreaterThan(0);
});
