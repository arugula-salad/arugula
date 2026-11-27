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

    await page.goto(`/p/${P}/chat/${AGENTS}?thread=2`);
    const thread = page.locator('[data-thread-panel="2"]');
    await expect(thread.locator("[data-msg]")).toHaveCount(4);
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
