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
