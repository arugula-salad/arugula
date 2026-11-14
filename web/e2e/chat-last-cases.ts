// #965: chat opens in the room you left, not the newest one. On a fixture
// page with no daemon (chat-last-page.tsx): the real chat page and the
// platform page's own router and history, over a fake control. At a phone's
// width and a computer's, in Chrome and in WebKit: chat-last.spec.ts and
// chat-last.webkit.spec.ts.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ALPHA = "ch_aaaa0001"; // the newest room
const BETA = "ch_aaaa0002";
const GAMMA = "ch_aaaa0003";
const CHAT = "/p/prj_aaaa0002/chat";

async function open(page: Page, path = CHAT) {
  const body = await fixturePage("./chat-last-page.tsx");
  await page.route(/\/p\/prj_aaaa0002/, (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
}

const view = (page: Page, room: string) => page.locator(`[data-room-view="${room}"]`);
const row = (page: Page, room: string) => page.locator(`[data-chat-room="${room}"]`);
const noRoom = (page: Page) => expect(page.locator("[data-room-view]")).toHaveCount(0);
/** The phone's list: every room's row, and no room. */
async function onList(page: Page) {
  await expect(row(page, ALPHA)).toBeVisible();
  await noRoom(page);
}
/** Frames pass, so a redirect that was going to happen has. */
const frames = (page: Page) => page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(() => done())))));
/** Another page of the project, then the rail's (or the app's) Chat. */
async function awayAndBack(page: Page) {
  await page.evaluate(() => window.__fx.away());
  await expect(page.locator("[data-away]")).toBeVisible();
  await page.locator("[data-go-chat]").click();
}

export function chatLastTests(device: Record<string, unknown>, phone: boolean) {
  test.use(phone ? device : { ...device, viewport: { width: 1280, height: 800 } });

  // A phone starts at the list; a computer shows a room already.
  const pick = async (page: Page, room: string) => {
    await row(page, room).click();
    await expect(view(page, room)).toBeVisible();
  };

  test("chat comes back to the room you were in, though another has unread", async ({ page }) => {
    await open(page);
    await pick(page, BETA);
    await page.evaluate(() => window.__fx.unread("ch_aaaa0001"));
    await awayAndBack(page);
    await expect(view(page, BETA)).toBeVisible();
    await frames(page);
    await expect(view(page, BETA)).toBeVisible();
    await expect(view(page, ALPHA)).toHaveCount(0);
  });

  test("a reload comes back to it too", async ({ page }) => {
    await open(page);
    await pick(page, BETA);
    await page.reload();
    await expect(view(page, BETA)).toBeVisible();
    // A computer's list address shows beta; a phone's page load is the list.
    if (phone) return;
    await open(page);
    await expect(view(page, BETA)).toBeVisible();
    await frames(page);
    await expect(view(page, BETA)).toBeVisible();
  });

  test("a link to a room wins, and becomes the one it comes back to", async ({ page }) => {
    await open(page);
    await pick(page, BETA);
    await open(page, `${CHAT}/${GAMMA}`);
    await expect(view(page, GAMMA)).toBeVisible();
    await awayAndBack(page);
    await expect(view(page, GAMMA)).toBeVisible();
  });

  test("a room that is gone opens as it always did", async ({ page }) => {
    await open(page);
    await page.evaluate(() => localStorage.setItem("chat.last.acct_a.tm_1", JSON.stringify({ room: "ch_aaaa00ff" })));
    await open(page);
    if (phone) await onList(page);
    else await expect(view(page, ALPHA)).toBeVisible();
  });

  test("a thread that was open opens again", async ({ page }) => {
    await open(page, `${CHAT}/${BETA}?thread=2`);
    await expect(page.locator("[data-thread-panel='2']")).toBeVisible();
    await awayAndBack(page);
    await expect(page.locator("[data-thread-panel='2']")).toBeVisible();
    // The room opened without it is the room kept, not the thread.
    await open(page, `${CHAT}/${BETA}`);
    await expect(view(page, BETA)).toBeVisible();
    await awayAndBack(page);
    await expect(view(page, BETA)).toBeVisible();
    await expect(page.locator("[data-thread-panel]")).toHaveCount(0);
  });

  test("each account has its own last room", async ({ page }) => {
    await open(page, `${CHAT}?me=acct_a`);
    await pick(page, BETA);
    await open(page, `${CHAT}?me=acct_b`);
    // B has been nowhere: a phone's list, a computer's newest.
    if (phone) await onList(page);
    else await expect(view(page, ALPHA)).toBeVisible();
    await pick(page, GAMMA);
    // Opened from the app, as a phone's page load is the list.
    const again = async (me: string) => {
      await open(page, `${CHAT}?me=${me}`);
      if (phone) {
        await onList(page);
        await awayAndBack(page);
      }
    };
    await again("acct_a");
    await expect(view(page, BETA)).toBeVisible();
    await again("acct_b");
    await expect(view(page, GAMMA)).toBeVisible();
  });

  if (phone) {
    test("a reload of the list stays the list", async ({ page }) => {
      await open(page);
      await pick(page, BETA);
      // The list's own address, with beta the room kept.
      await open(page);
      await onList(page);
      await page.reload();
      await onList(page);
      await frames(page);
      await onList(page);
    });

    test("a page load of /chat is the list, and the rail's Chat (openChat) then opens the kept room", async ({ page }) => {
      await open(page);
      await pick(page, BETA);
      // A fresh load of the list address, beta being the one kept.
      await open(page);
      await onList(page);
      await frames(page);
      await onList(page);
      await awayAndBack(page);
      await expect(view(page, BETA)).toBeVisible();
      // With the list beneath it, so Back climbs.
      await page.goBack();
      await onList(page);
    });

    test("Back from the room it opened in is the list, and stays the list", async ({ page }) => {
      await open(page);
      await pick(page, BETA);
      await awayAndBack(page);
      await expect(view(page, BETA)).toBeVisible();
      await page.goBack();
      await onList(page);
      await frames(page);
      await onList(page);
    });

    test("left at the list, chat comes back to the list", async ({ page }) => {
      await open(page);
      await pick(page, BETA);
      await page.locator(".chat-back").click();
      await onList(page);
      await awayAndBack(page);
      await onList(page);
      await frames(page);
      await onList(page);
      // And a room picked from it is the one after.
      await pick(page, GAMMA);
      await awayAndBack(page);
      await expect(view(page, GAMMA)).toBeVisible();
    });
  } else {
    test("with nothing kept, the newest unread room still opens", async ({ page }) => {
      await open(page, `${CHAT}?unread=ch_aaaa0003`);
      await expect(view(page, GAMMA)).toBeVisible();
    });

    test("unread arriving elsewhere doesn't move you", async ({ page }) => {
      await open(page);
      await pick(page, BETA);
      await page.evaluate(() => window.__fx.unread("ch_aaaa0003"));
      await expect(row(page, GAMMA)).toHaveClass(/unread/);
      await expect(view(page, BETA)).toBeVisible();
      await awayAndBack(page);
      await expect(view(page, BETA)).toBeVisible();
    });
  }
}
