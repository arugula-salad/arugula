// A board card's live session in the app (Arugula
// iss_c62e00eee3b4b953ff054d2951827de8): the drawer draws the card's agent
// pane live with the app's own client to its machine, with Hand off to me
// and Give it back (today's driver), says while you drive, answers the
// pane's question at its top as Swarm does, opens the pane on its machine,
// says when a card has no session yet, and on a phone fills the screen with
// the key bar while you drive. On the app's fixture page
// (app-frame-page.tsx), where the machine `m_suite` is the suite's daemon;
// the board is made up, its agent block a real terminal of the daemon.
import { fileURLToPath } from "node:url";
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

const P = "prj_aaaa0002";
const B = "brd_0123456789abcdef0123456789abcdef";
const hookInput = fileURLToPath(new URL("../../crates/daemon/tests/fixtures/s13-hook-ask.json", import.meta.url));
const cli = fileURLToPath(new URL("../../target/debug/arugula", import.meta.url));

test.use({ permissions: ["local-network-access"] });

const row = (id: string, note: string, attachments: unknown[]) => ({ id, rev: 1, pointer: { kind: "none" }, note, status: "none", lane: "doing", attachments, updated_at: 1 });

/** The board: a card whose agent block is `pane` of the suite's daemon, with a PR beside it, and a card with no session. */
function board(pane: number) {
  const agent = { id: "att_agent", role: "work", pointer: { kind: "block", machine: "m_suite", pane: String(pane) }, block: { state: "working", title: "Fixer", branch: "arugula/fix", head: "abc1234", diffstat: { files: 2, added: 5, removed: 1 } } };
  const pr = { id: "att_pr", role: "work", pointer: { kind: "link", url: "https://example.com/pr/7" } };
  return {
    board: { id: B, project: P, title: "Launch", created_by: "acct_a", created_at: 1, updated_at: 1, goal: "", done_note: "", closed_by: "", progress: { done: 0, total: 0, proven: 0, review: 0, doing: 0 } },
    zones: [{ id: "z_doing", rev: 1, title: "Doing", kind: "doing", rows: [row("r_live", "The live one", [agent, pr]), row("r_none", "Not started", [])] }],
    groups: [],
    docs: [],
  };
}

/** A fresh pane on the daemon, then the board in the app, with a row's drawer open. */
async function openDrawer(page: Page, which: string): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.route(new RegExp(`/api/boards/${B}$`), (r) => r.fulfill({ contentType: "application/json", body: JSON.stringify(board(pane)) }));
  await page.goto(`/p/${P}/boards/${B}`);
  await page.locator(`[data-board-row="${which}"] [data-board-title]`).click();
  await expect(page.locator(`[data-board-drawer="${which}"]`)).toBeVisible();
  return pane;
}

/** Type a line into a terminal through the daemon's API, as another device would. */
async function send(page: Page, pane: number, line: string) {
  await page.evaluate(
    async ({ pane, line }) => {
      await fetch(`/api/panes/${pane}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: line, enter: true }) });
    },
    { pane, line },
  );
}

/** The pane's screen, as its machine has it. */
const text = (page: Page, pane: number) => page.evaluate(async (p) => (await fetch(`/api/panes/${p}/capture?format=text`)).text(), pane);

/** What the drawer's own client shows of the pane. */
const shown = (page: Page, pane: number) =>
  page.evaluate((p) => (window as unknown as { __live: { client(m: string): { panes: Map<number, { view: { text(): string } }> } | undefined } }).__live.client("m_suite")?.panes.get(p)?.view.text() ?? "", pane);

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the card's session is drawn live; Hand off to me drives it, Give it back lets go; Attached keeps the PR", async ({ page }, info) => {
    const pane = await openDrawer(page, "r_live");
    const drawer = page.locator('[data-board-drawer="r_live"]');
    const live = drawer.locator("[data-board-drawer-live]");
    // The state card, with the block's hooks, and the pane under it.
    await expect(live.locator("[data-board-live-card]")).toHaveAttribute("data-board-attachment", "att_agent");
    await expect(live.locator("[data-board-block-facts]")).toHaveText("arugula/fix@abc1234 · 2 files, +5 −1");
    await expect(live.locator(`[data-live-slot="${pane}"] .xterm`)).toBeVisible();
    // What runs there shows here, drawn by the drawer's own client.
    await send(page, pane, "echo live-$((6*7))");
    await expect.poll(() => shown(page, pane)).toContain("live-42");
    // The agent block left Attached; the PR stayed.
    await expect(drawer.locator("[data-board-drawer-attachments] [data-board-attachment]")).toHaveCount(1);
    await expect(drawer.locator('[data-board-drawer-attachments] [data-board-attachment="att_pr"]')).toBeVisible();

    // Hand off to me: you drive, the card says so, and the slot is outlined.
    await live.locator("[data-live-hand-off]").click();
    await expect(live.locator("[data-live-you-drive]")).toHaveText("you are driving");
    await expect(live.locator(".live-pane.driving")).toBeVisible();
    await page.screenshot({ path: info.outputPath("board-live-driving.png") });
    // Typed in the drawer, run on the machine.
    await live.locator(`[data-live-slot="${pane}"]`).click();
    await page.keyboard.type("echo drove-$((2*21))\n");
    await expect.poll(() => text(page, pane)).toContain("drove-42");
    // Escape is the terminal's while you drive: the drawer stays.
    await page.keyboard.press("Escape");
    await expect(drawer).toBeVisible();
    // Give it back.
    await live.locator("[data-live-give-back]").click();
    await expect(live.locator("[data-live-hand-off]")).toBeVisible();
    await expect(live.locator("[data-live-you-drive]")).toHaveCount(0);

    // Open on its machine: the pane, on its machine's page in the app.
    await live.locator("[data-board-live-open]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines.*pane=${pane}`));
  });

  test("the session's question is at the top of the drawer, answered in place", async ({ page }) => {
    const pane = await openDrawer(page, "r_live");
    await send(page, pane, `out=$(${cli} ask < ${hookInput}); echo "$out" | grep -o '"Which [a-z ]*?":"[^"]*"'`);
    const drawer = page.locator('[data-board-drawer="r_live"]');
    const ask = drawer.locator("[data-board-drawer-ask]");
    await expect(ask).toBeVisible();
    await expect(ask.locator("h3")).toHaveText("Claude Code asks");
    // Above everything else the drawer says.
    const top = await ask.boundingBox();
    const lane = await drawer.locator(".board-drawer-lane").boundingBox();
    expect(top!.y).toBeLessThan(lane!.y);
    await ask.locator('[data-question="0"]').getByRole("radio", { name: /^Blue/ }).click();
    await ask.locator('[data-question="1"]').getByRole("checkbox", { name: /Pear/ }).check();
    await ask.locator('[data-question="2"]').getByRole("radio", { name: /^Cat/ }).click();
    await ask.getByRole("button", { name: "Submit" }).click();
    await expect(ask).toHaveCount(0);
    await expect.poll(() => text(page, pane)).toContain('"Which colour do you prefer?":"Blue"');
  });

  test("on a teammate's computer you watch: Hand off is off, and the slot says why", async ({ page }) => {
    const pane = await openDrawer(page, "r_live");
    const live = page.locator('[data-board-drawer="r_live"] [data-board-drawer-live]');
    await expect(live.locator("[data-live-hand-off]")).toBeVisible();
    // As the daemon tells a guest: the roles it has in each session (a teammate's machine).
    await page.evaluate((p) => {
      const c = (window as unknown as { __live: { client(m: string): { state: { roles?: unknown; tabs: unknown }; sessionOfPane(p: number): number | null; emit(): void } } }).__live.client("m_suite");
      c.state.roles = [[c.sessionOfPane(p), "editor"]];
      c.emit();
    }, pane);
    await expect(live.locator("[data-live-watching]")).toContainText("teammate's computer");
    await expect(live.locator("[data-live-hand-off]")).toHaveCount(0);
    await expect(live.locator(`[data-live-slot="${pane}"] .xterm`)).toBeVisible();
  });

  test("a card with no session says so, and starts none", async ({ page }) => {
    await openDrawer(page, "r_none");
    const live = page.locator('[data-board-drawer="r_none"] [data-board-drawer-live]');
    await expect(live.locator("[data-board-live-none]")).toHaveText("No session yet.");
    await expect(live.locator("button")).toHaveCount(0);
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the drawer is the whole screen, with the key bar while you drive", async ({ page }, info) => {
    const pane = await openDrawer(page, "r_live");
    const drawer = page.locator('[data-board-drawer="r_live"]');
    const live = drawer.locator("[data-board-drawer-live]");
    await expect(live.locator(`[data-live-slot="${pane}"] .xterm`)).toBeVisible();
    const box = await drawer.boundingBox();
    expect(box!.y).toBeLessThanOrEqual(1);
    expect(box!.height).toBeGreaterThanOrEqual(viewport.height - 2);
    await expect(live.locator(".keybar")).toHaveCount(0);
    await live.locator("[data-live-hand-off]").tap();
    await expect(live.locator(".keybar")).toBeVisible();
    await page.screenshot({ path: info.outputPath("board-live-phone.png") });
    // A key from the bar reaches the pane.
    await send(page, pane, "IFS= read -r x; printf 'got[%q]\\n' \"$x\"");
    await live.locator(".keybar").getByRole("button", { name: "Tab" }).tap();
    await send(page, pane, "");
    await expect.poll(() => text(page, pane)).toContain("got[$'\\t']");
  });
});
