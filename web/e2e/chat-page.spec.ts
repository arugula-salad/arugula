// Chat on the kit (Lift 6c, Arugula #1485): in the app's frame, three
// columns (the rooms and the stream as one section, the thread or Details
// as another), each room a kit Row, the room the bar's last step, and your
// presence behind your picture. On a fixture page with no daemon
// (chat-page.tsx): the platform page itself over a fake control.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { pixel7 } from "./phones";

const P = "prj_aaaa0002";
const GENERAL = "ch_00000001";
const AGENTS = "ob_00000001";

async function open(page: Page, path: string) {
  const body = await fixturePage("./chat-page.tsx");
  await page.route(/\/p\/prj_aaaa0002/, (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
}

const box = async (page: Page, sel: string) => (await page.locator(sel).first().boundingBox())!;

test.describe("computer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("Chat is the frame's: the room is the bar's last step, each room a kit row, and no head or presence row of its own", async ({ page }) => {
    await open(page, `/p/${P}/chat/${GENERAL}`);
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Chat\s*#general/);
    const row = page.locator(`[data-chat-room="${GENERAL}"]`);
    await expect(row).toHaveAttribute("data-kit", "row");
    await expect(row.locator("a.k-row-line")).toHaveAttribute("aria-current", "page");
    await expect(row.locator("a.k-row-line")).toHaveAttribute("href", new RegExp(`/chat/${GENERAL}`));
    // Unread is a heavier title; Activity, Saved and Threads are rows too.
    await expect(page.locator('[data-chat-room="ch_00000003"]')).toHaveAttribute("data-bold", "true");
    for (const top of ["activity", "saved", "threads"]) await expect(page.locator(`[data-chat-${top}]`)).toHaveAttribute("data-kit", "row");
    await expect(page.locator(".chat-side-head, [data-chat-you]")).toHaveCount(0);
    // ⋯ sits beside search, in Chat's own root.
    await page.locator("[data-chat-control] [data-chat-workspace]").click();
    await expect(page.getByRole("menuitem", { name: "Mark all as read" })).toBeVisible();
    // A click opens a room in place; the bar follows.
    await page.keyboard.press("Escape");
    await page.locator('[data-chat-room="ch_00000002"]').click();
    await expect(page.locator('[data-room-view="ch_00000002"]')).toBeVisible();
    await expect(page.locator("[data-app-path]")).toHaveText(/Chat\s*#design/);
  });

  test("Details and a thread are a column of their own, beside the stream", async ({ page }) => {
    await open(page, `/p/${P}/chat/${GENERAL}`);
    await page.locator("[data-chat-details]").click();
    const details = await box(page, "[data-chat-details-panel]");
    const stream = await box(page, `[data-room-view="${GENERAL}"]`);
    const side = await box(page, ".chat-side");
    // Rooms, then the stream against them, then a gap and Details.
    expect(Math.round(stream.x)).toBe(Math.round(side.x + side.width));
    expect(details.x).toBeGreaterThan(stream.x + stream.width + 4);
    // The kit's composer: a glow and a round send.
    await expect(page.locator(`[data-room-view="${GENERAL}"] [data-kit="composer"]`)).toBeVisible();

    await page.goto(`/p/${P}/chat/${GENERAL}?thread=1`);
    const thread = page.locator('[data-thread-panel="1"]');
    await expect(thread.locator("[data-msg]")).toHaveCount(2);
    expect((await thread.boundingBox())!.x).toBeGreaterThan(stream.x + 100);
    await expect(thread.locator('[data-kit="composer"]')).toBeVisible();
    await thread.getByRole("textbox", { name: "Reply in thread" }).press("Escape");
    await expect(thread).toHaveCount(0);
  });

  test("a dialog of Chat's (New direct message) opens over the stream, and a click on it reaches it", async ({ page }) => {
    await open(page, `/p/${P}/chat/${GENERAL}`);
    await page.locator('[data-room-new="tm_1/dms"]').click();
    const start = page.locator("[data-dm-start]");
    await expect(start).toBeVisible();
    // Nothing of the stream's column is over it (the rooms' column once clipped its dialogs).
    expect(await start.evaluate((b) => {
      const r = b.getBoundingClientRect();
      const top = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      return !!top && (top === b || b.contains(top));
    })).toBe(true);
  });

  test("your presence is behind your picture while chat is on (Arugula #1079)", async ({ page }) => {
    await open(page, `/p/${P}/chat/${GENERAL}`);
    await expect(page.locator(`[data-room-view="${GENERAL}"]`)).toBeVisible();
    await page.locator("[data-rail] [data-signed-in-as]").click();
    await expect(page.locator(".menu", { hasText: "Your presence" })).toBeVisible();
    await expect(page.getByRole("menuitemradio", { name: "Automatic" })).toHaveAttribute("aria-checked", "true");
    await expect(page.getByRole("menuitemradio", { name: "Appear offline" })).toBeVisible();
  });
});

test.describe("Agents (Arugula #1522)", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("the section: the chief of staff, #agents, a channel per open board, closed ones archived; no issue's comments", async ({ page }) => {
    await open(page, `/p/${P}/chat/${GENERAL}`);
    const section = page.locator('[data-chat-group="agents"]');
    await expect(section.locator("[data-kit=row] .k-row-title")).toHaveText(["Chief of staff · Arugula", "agents", "Design lift", "A2P", "Product improvements", "Rail and Machines", "archived · 2"]);
    // The chief of staff is here, not under Direct messages.
    await expect(page.locator('[data-chat-group="tm_1/dms"] [data-chat-room="dm_00000003"]')).toHaveCount(0);
    // An issue's comments are listed nowhere.
    await expect(page.locator('[data-chat-room="ob_00000009"]')).toHaveCount(0);
    // A board's lead is its dot.
    await expect(section.locator('[data-agents-board="brd_00000000000000000000000000000001"] [data-board-lead]')).toHaveCount(1);
    await expect(section.locator('[data-agents-board="brd_00000000000000000000000000000003"] [data-board-lead]')).toHaveCount(0);
    // Archived folds.
    await section.locator("[data-agents-archived] > .k-row-line").click();
    await expect(section.locator('[data-agents-board="brd_00000000000000000000000000000005"]')).toBeVisible();
    // It collapses as the others do.
    await section.locator("h2 button").click();
    await expect(section.locator("[data-kit=row]")).toHaveCount(0);
    await section.locator("h2 button").click();
    await expect(section.locator('[data-agents-room]')).toBeVisible();
  });

  test("#agents: its name, its About beside it, and a thread in place that Esc leaves", async ({ page }) => {
    await open(page, `/p/${P}/chat/${GENERAL}`);
    await page.locator("[data-agents-room] > .k-row-line").click();
    const view = page.locator(`[data-room-view="${AGENTS}"]`);
    await expect(view.locator(".chat-title")).toHaveText("#agents");
    await expect(page.locator("[data-app-path]")).toHaveText(/Chat\s*#agents/);
    await expect(view.getByRole("textbox", { name: "Message #agents" })).toBeVisible();
    const about = page.locator('[data-agents-about="agents"]');
    await expect(about).toContainText("Chief of staff");
    await expect(about.locator("[data-reach-rules]")).toContainText("every lead, no tag needed");
    await expect(view.locator("[data-chat-details]")).toHaveCount(0);
    // A thread opens in the room's own column, the About still beside it.
    await view.locator('[data-thread-footer="2"]').click();
    const thread = page.locator('[data-thread-panel="2"][data-thread-in-place]');
    await expect(thread).toBeVisible();
    await expect(view).toHaveCount(0);
    await expect(about).toBeVisible();
    await expect(thread.locator("[data-thread-back]")).toHaveText("#agents");
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.keyboard.press("Escape");
    await expect(thread).toHaveCount(0);
    await expect(view).toBeVisible();
  });

  test("a board's channel is made on its first click, and opens on the board", async ({ page }) => {
    await open(page, `/p/${P}/chat/${GENERAL}`);
    const a2p = page.locator('[data-agents-board="brd_00000000000000000000000000000002"]');
    await expect(a2p).not.toHaveAttribute("data-chat-room");
    await a2p.locator("> .k-row-line").click();
    await expect(page.locator("[data-app-path]")).toHaveText(/Chat\s*#A2P/);
    await expect(a2p).toHaveAttribute("data-chat-room", /^ob_/);
    await expect(page.locator("[data-agents-open-board]")).toHaveAttribute("href", /\/boards?\/brd_00000000000000000000000000000002|A2P/);
  });

  test("j and k move through the agents' rooms; g a is #agents", async ({ page }) => {
    await open(page, `/p/${P}/chat/${GENERAL}`);
    await expect(page.locator(`[data-room-view="${GENERAL}"]`)).toBeVisible();
    await page.locator("[data-app-path]").click();
    await page.keyboard.press("g");
    await page.keyboard.press("a");
    await expect(page.getByRole("textbox", { name: "Message #agents" })).toBeFocused();
    // Never from the box you write in.
    await page.keyboard.press("j");
    await expect(page.locator("[data-app-path]")).toHaveText(/#agents/);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.keyboard.press("j");
    await expect(page.locator("[data-app-path]")).toHaveText(/#Design lift/);
    // A room opened by a key leaves the keys with the page.
    await expect(page.getByRole("textbox", { name: "Message #Design lift" })).not.toBeFocused();
    await page.keyboard.press("k");
    await expect(page.locator("[data-app-path]")).toHaveText(/#agents/);
    await page.keyboard.press("k");
    await expect(page.locator("[data-app-path]")).toHaveText(/Chief of staff/);
  });
});

test.describe("Tags (Arugula #1522)", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("@ in #agents lists channel, here, cos and each board's lead, with the rule; a pick goes by id and draws as a pill with its receipt", async ({ page }) => {
    await open(page, `/p/${P}/chat/${AGENTS}`);
    const view = page.locator(`[data-room-view="${AGENTS}"]`);
    // Tags already said: pills, and who each post reached.
    await expect(view.locator('[data-msg="2"] [data-tag="board"]')).toHaveText(["@Design lift", "@A2P"]);
    await expect(view.locator('[data-msg="2"] [data-reached]')).toHaveText(/reached: Design lift, A2P/);
    await expect(view.locator('[data-msg="4"] [data-reached]')).toHaveText(/reached: no lead · it tagged none/);
    await expect(view.locator('[data-msg="5"] [data-reached]')).toHaveText(/reached: 2 leads, chief of staff/);
    // The composer says who a post here reaches.
    await expect(view.locator(".k-composer-to")).toHaveText("Every lead gets this, no tag needed");
    const box = view.getByRole("textbox", { name: "Message #agents" });
    await box.fill("Deploying web at 12:30. @");
    const list = page.locator("[data-tag-list=agents]");
    await expect(list.locator("[data-tag-option]")).toHaveCount(7);
    await expect(list.locator("[data-tag-option] b").first()).toHaveText("@channel");
    await expect(list.locator("[data-tag-rule]")).toContainText("A lead's post reaches only the leads it tags.");
    await box.press("A");
    await box.press("2");
    await expect(list.locator("[data-tag-option]")).toHaveCount(1);
    await box.press("Enter");
    await expect(box).toHaveValue("Deploying web at 12:30. @A2P ");
    await box.press("Enter");
    await expect.poll(() => page.evaluate(() => window.__fx.posted.at(-1))).toEqual({ text: "Deploying web at 12:30. @A2P ", tags: [{ kind: "board", board: "brd_00000000000000000000000000000002", text: "A2P" }] });
    const sent = view.locator('[data-msg="7"]');
    await expect(sent.locator('[data-tag="board"]')).toHaveText("@A2P");
    await expect(sent.locator("[data-reached]")).toHaveText(/reached: 2 leads/);
    // No "nobody called" for a tag's first word.
    await box.fill("@Design lift, rebase please");
    await box.press("Escape");
    await box.press("Enter");
    await expect(view.locator('[data-msg="8"]')).toBeVisible();
    await expect(page.getByText(/Nobody here called/)).toHaveCount(0);
  });

  test("a board's channel says its lead gets a post; other rooms have no tag list", async ({ page }) => {
    await open(page, `/p/${P}/chat/ob_00000003`);
    await expect(page.locator('[data-room-view="ob_00000003"] .k-composer-to')).toHaveText("Its lead gets this · @here adds active workers");
    await page.goto(`/p/${P}/chat/${GENERAL}`);
    const box = page.getByRole("textbox", { name: "Message #general" });
    await box.fill("@");
    await expect(page.locator("[data-tag-list]")).toHaveCount(0);
    await expect(page.locator("[data-tag-option]")).toHaveCount(0);
  });
});

test.describe("A room as a pane (Arugula #1522)", () => {
  test.use({ viewport: { width: 1440, height: 900 } });
  const PI = "brd_00000000000000000000000000000003";
  const posted = (page: Page) => page.evaluate(() => window.__fx.layoutsPosted);

  test("a board's + pane offers #agents and its channel; #agents opens beside the board as Chat's room", async ({ page }) => {
    await open(page, `/p/${P}/boards/${PI}`);
    await page.locator("[data-board-add-pane-menu]").click();
    await expect(page.locator('[role=menu] [data-board-add-pane="channel"]')).toHaveText(/#Product improvements/);
    await page.locator('[role=menu] [data-board-add-pane="agents"]').click();
    await expect.poll(() => posted(page)).toEqual([{ place: `board:${PI}`, change: { op: "show", pane: { kind: "room", ref: AGENTS, title: "#agents", machine: null, state: null }, intent: "beside", tab: "home", in_place_of: null } }]);
    const pane = page.locator(`[data-pane-kind="room"] [data-room-pane="${AGENTS}"]`);
    await expect(pane.locator(`[data-room-view="${AGENTS}"] [data-msg]`)).toHaveCount(6);
    await expect(pane.getByRole("textbox", { name: "Message #agents" })).toBeAttached();
    // The pane is the room alone: no rooms list.
    await expect(pane.locator(".chat-side")).toHaveCount(0);
  });

  test("g a on a board opens #agents beside it", async ({ page }) => {
    await open(page, `/p/${P}/boards/${PI}`);
    await expect(page.locator(`[data-board-desk="${PI}"]`)).toBeVisible();
    await page.keyboard.press("g");
    await page.keyboard.press("a");
    await expect(page.locator(`[data-room-pane="${AGENTS}"]`)).toBeVisible();
  });

  test("in Chat, o opens a board's channel as a pane on its board; #agents has no board to open on", async ({ page }) => {
    await open(page, `/p/${P}/chat/${AGENTS}`);
    await expect(page.locator(`[data-room-view="${AGENTS}"]`)).toBeVisible();
    await expect(page.locator("[data-room-open-pane]")).toHaveCount(0);
    await page.goto(`/p/${P}/chat/ob_00000003`);
    const asPane = page.locator('[data-room-view="ob_00000003"] [data-room-open-pane]');
    await expect(asPane).toContainText("Open as a pane");
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.keyboard.press("o");
    await expect(page).toHaveURL(new RegExp(`/boards/${PI}`));
    await expect.poll(() => posted(page)).toEqual([{ place: `board:${PI}`, change: expect.objectContaining({ op: "show", pane: expect.objectContaining({ kind: "room", ref: "ob_00000003" }) }) }]);
    await expect(page.locator('[data-room-pane="ob_00000003"]')).toBeVisible();
  });
});

test.describe("phone", () => {
  test.use(pixel7);

  test("the rooms are rows a finger's height, edge to edge", async ({ page }) => {
    await open(page, `/p/${P}/chat`);
    const row = page.locator(`[data-chat-room="${GENERAL}"]`);
    await expect(row).toBeVisible();
    const b = (await row.boundingBox())!;
    expect(b.height).toBeGreaterThanOrEqual(44);
    // Its count stays on the screen.
    const count = (await page.locator('[data-chat-room="ch_00000003"] .k-row-meta').last().boundingBox())!;
    expect(count.x + count.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  });
});
