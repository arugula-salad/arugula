// #965: a room opens where it was left, and its view doesn't jump when the
// keyboard comes and goes, a message is sent or things above it load. On a
// fixture page with no daemon (chat-scroll-page.tsx), at a phone's width, in
// Chrome (Pixel 7) and in WebKit (iPhone): chat-scroll.spec.ts and
// chat-scroll.webkit.spec.ts.
//
// The check is the anchor message's place on every frame across the change,
// not where it ends: a bounce that settles would pass the end alone.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";

/** How far a message may move on any frame. */
const SLACK = 4;
const A = "ch_aaaa0001";
const B = "ch_aaaa0002";
const UNREAD = "ch_aaaa0003";

/** A 1x1 PNG, stretched by the page to a picture that takes room once loaded. */
const PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64");

async function open(page: Page, room: string = A, query = "") {
  const body = await fixturePage("./chat-scroll-page.tsx");
  await page.route("**/chat-scroll.html*", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.route("**/fx/pic.png", async (r) => {
    await new Promise((done) => setTimeout(done, 150));
    await r.fulfill({ contentType: "image/png", body: PNG });
  });
  await page.goto(`/chat-scroll.html${query}`);
  await pick(page, room);
}

async function pick(page: Page, room: string) {
  await page.locator(`[data-pick="${room}"]`).click();
  await expect(page.locator(`[data-room="${room}"]`)).toBeVisible();
  await expect(page.locator("[data-msg]").first()).toBeVisible();
  // Settled: placed, and nothing more moving.
  await settle(page);
}

/** Two frames with the list where it was, so what loads after is the test's. */
async function settle(page: Page) {
  await page.evaluate(
    () =>
      new Promise<void>((done) => {
        const list = document.querySelector<HTMLElement>(".thread-list")!;
        let same = 0;
        let at = -1;
        const tick = () => {
          same = list.scrollTop === at ? same + 1 : 0;
          at = list.scrollTop;
          if (same >= 5) done();
          else requestAnimationFrame(tick);
        };
        tick();
      }),
  );
}

const list = (page: Page) => page.locator(".thread-list");
const scrollTop = (page: Page) => list(page).evaluate((el) => el.scrollTop);
const atBottom = (page: Page) => list(page).evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight);
const top = (page: Page, seq: number) => page.evaluate((n) => window.__fx.top(n), seq);

/** The message under the top edge: what the list anchors to (a person's eye is at the top of what they read). */
async function mid(page: Page): Promise<number> {
  return list(page).evaluate((el) => {
    const edge = el.getBoundingClientRect().top;
    const all = [...el.querySelectorAll<HTMLElement>("[data-msg]")];
    return Number(all.find((m) => m.getBoundingClientRect().bottom > edge)!.dataset.msg);
  });
}

/** Scroll as the person does: the list's scroll event is what tells it (mobile WebKit has no wheel). */
async function scrollBy(page: Page, dy: number) {
  await list(page).evaluate((el, dy) => el.scrollBy({ top: dy, behavior: "instant" }), dy);
  await settle(page);
}

/** What `fn` did to message `seq`'s place, frame by frame: the most it moved from where it was. */
async function drift(page: Page, seq: number, fn: () => Promise<void>): Promise<number> {
  const before = (await top(page, seq))!;
  await page.evaluate((n) => {
    (window as any).__w = window.__fx.watch(n);
  }, seq);
  await fn();
  await settle(page);
  const seen = await page.evaluate(() => (window as any).__w.stop() as number[]);
  expect(seen.length).toBeGreaterThan(0);
  return Math.max(...seen.map((t) => Math.abs(t - before)));
}

/** Shrink the page by a keyboard, both ways: the layout viewport (Android's
 * Chrome resizes it) and only the visual viewport (an iPhone's Safari). */
const keyboards = {
  "the viewport resizing": {
    on: (page: Page) => page.setViewportSize({ ...page.viewportSize()!, height: page.viewportSize()!.height - 300 }),
    off: (page: Page) => page.setViewportSize({ ...page.viewportSize()!, height: page.viewportSize()!.height + 300 }),
  },
  "only the visual viewport shrinking": {
    on: (page: Page) => page.evaluate(() => window.__fx.keyboard(300)),
    off: (page: Page) => page.evaluate(() => window.__fx.keyboard(0)),
  },
};

export function chatScrollTests(device: Record<string, unknown>) {
  test.use(device);

  test("a room opens at the bottom, newest last", async ({ page }) => {
    await open(page);
    expect(await atBottom(page)).toBeLessThan(2);
    await expect(page.locator('[data-msg="120"]')).toBeInViewport();
  });

  test("the list anchors itself: the browser's scroll anchoring is off", async ({ page, browserName }) => {
    test.skip(browserName !== "chromium", "Safari has no overflow-anchor");
    await open(page);
    expect(await list(page).evaluate((el) => getComputedStyle(el).overflowAnchor)).toBe("none");
  });

  test("leave a room scrolled up and come back: the same message is at the same place", async ({ page }) => {
    await open(page);
    await scrollBy(page, -900);
    const seq = await mid(page);
    const was = (await top(page, seq))!;
    expect(await atBottom(page)).toBeGreaterThan(400);
    // To another room, and back (the room's own messages stay loaded).
    await pick(page, B);
    expect(await atBottom(page)).toBeLessThan(2);
    await pick(page, A);
    expect(Math.abs((await top(page, seq))! - was)).toBeLessThanOrEqual(SLACK);
    expect(await atBottom(page)).toBeGreaterThan(400);
  });

  test("after a reload: where it was left, with older messages loaded to reach it", async ({ page }) => {
    await open(page);
    // Up through the 40 loaded, and a page of older ones.
    for (let i = 0; i < 12 && (await top(page, 60)) === null; i++) {
      await list(page).evaluate((el) => (el.scrollTop = 0));
      await page.locator("[data-room-older]").click({ timeout: 1500 }).catch(() => {});
      await settle(page);
    }
    await list(page).evaluate((el) => (el.scrollTop = 0));
    await scrollBy(page, 200);
    const seq = await mid(page);
    expect(seq).toBeLessThan(80);
    const was = (await top(page, seq))!;
    // Written when it's left: a second's wait covers the throttle.
    await page.evaluate(() => dispatchEvent(new Event("pagehide")));
    await page.reload();
    await pick(page, A);
    await expect.poll(async () => await top(page, seq)).not.toBeNull();
    await settle(page);
    expect(Math.abs((await top(page, seq))! - was)).toBeLessThanOrEqual(SLACK);
  });

  test("a room left at the bottom opens at the bottom; storage that's broken is no matter", async ({ page }) => {
    await open(page);
    await pick(page, B);
    await page.evaluate(() => localStorage.setItem("chat.scroll.ch_aaaa0001", "{not json"));
    await pick(page, A);
    expect(await atBottom(page)).toBeLessThan(2);
    await page.evaluate(() => localStorage.setItem("chat.scroll.ch_aaaa0002", JSON.stringify({ seq: 9999, offset: 0 })));
    await pick(page, B);
    expect(await atBottom(page)).toBeLessThan(2);
  });

  test("a room with unread messages opens at the bottom, where its New line is", async ({ page }) => {
    await page.addInitScript(() => localStorage.setItem("chat.scroll.ch_aaaa0003", JSON.stringify({ seq: 100, offset: 10 })));
    await open(page, UNREAD);
    expect(await atBottom(page)).toBeLessThan(2);
    await expect(page.locator("[data-new-line]")).toBeAttached();
  });

  test("a linked message opens in the middle, further back than what's loaded, and stays there", async ({ page }) => {
    await open(page, A, "?msg=70");
    await expect.poll(() => top(page, 70)).not.toBeNull();
    await settle(page);
    const mid = await list(page).evaluate((el) => el.clientHeight / 2);
    const at = (await top(page, 70))!;
    // Centred on the message: its top is in the middle band of the list.
    expect(at).toBeGreaterThan(mid - 250);
    expect(at).toBeLessThan(mid + 50);
    const moved = await drift(page, 70, async () => {
      await page.evaluate(() => window.__fx.keyboard(300));
      await page.waitForTimeout(200);
    });
    expect(moved).toBeLessThanOrEqual(SLACK);
  });

  for (const [name, kb] of Object.entries(keyboards)) {
    test(`${name} at the bottom: it stays at the bottom, with no movement`, async ({ page }) => {
      await open(page);
      const newest = 120;
      const moved = await drift(page, newest, async () => {
        await kb.on(page);
        await page.waitForTimeout(200);
      });
      // At the bottom the newest is where it was from the bottom, not from the top:
      // so what's checked is that it's at the bottom again, and never left it by a row.
      expect(await atBottom(page)).toBeLessThan(2);
      expect(moved).toBeLessThanOrEqual(300 + SLACK);
      await kb.off(page);
      await settle(page);
      expect(await atBottom(page)).toBeLessThan(2);
    });

    test(`${name} mid-history: the message under the eye doesn't move`, async ({ page }) => {
      await open(page);
      await scrollBy(page, -900);
      const seq = await mid(page);
      const shown = await drift(page, seq, async () => {
        await kb.on(page);
        await page.waitForTimeout(200);
      });
      expect(shown).toBeLessThanOrEqual(SLACK);
      const closed = await drift(page, seq, async () => {
        await kb.off(page);
        await page.waitForTimeout(200);
      });
      expect(closed).toBeLessThanOrEqual(SLACK);
    });
  }

  test("sending: my message goes to the bottom once and the view doesn't move after", async ({ page }) => {
    await open(page);
    await scrollBy(page, -500);
    expect(await atBottom(page)).toBeGreaterThan(300);
    const box = page.locator("textarea, [contenteditable=true]").first();
    await box.click();
    await box.fill("a line that I wrote");
    await page.evaluate(() => {
      (window as any).__w = window.__fx.watch("gap");
    });
    await settle(page);
    await page.evaluate(() => (window as any).__w.clear());
    await box.press("Enter");
    await expect(page.getByText("a line that I wrote")).toBeVisible();
    await settle(page);
    const gaps = await page.evaluate(() => (window as any).__w.stop() as number[]);
    // Once: every change of size after it's sent leaves it at the bottom, and at the end too.
    expect(gaps.length).toBeGreaterThan(0);
    expect(Math.max(...gaps)).toBeLessThan(2);
    expect(await atBottom(page)).toBeLessThan(2);
  });

  test("a picture loading above the anchor doesn't move it", async ({ page }) => {
    await open(page);
    await scrollBy(page, -900);
    const seq = await mid(page);
    const above = seq - 4;
    expect(await top(page, above)).not.toBeNull();
    const moved = await drift(page, seq, async () => {
      await page.evaluate((n) => {
        const img = document.createElement("img");
        img.style.cssText = "display:block;width:100%;height:220px";
        img.src = "/fx/pic.png";
        document.querySelector(`[data-msg="${n}"]`)!.appendChild(img);
      }, above);
      await page.waitForTimeout(400);
    });
    expect(moved).toBeLessThanOrEqual(SLACK);
  });

  test("a row above or below growing doesn't move the anchor; at the bottom it keeps the bottom", async ({ page }) => {
    await open(page);
    await scrollBy(page, -900);
    const seq = await mid(page);
    const grow = (n: number, px: number) =>
      page.evaluate(([n, px]) => ((document.querySelector(`[data-msg="${n}"]`) as HTMLElement).style.paddingBottom = `${px}px`), [n, px]);
    expect(await drift(page, seq, () => grow(seq - 3, 90))).toBeLessThanOrEqual(SLACK);
    expect(await drift(page, seq, () => grow(seq + 3, 90))).toBeLessThanOrEqual(SLACK);
    await pick(page, B);
    await grow(120, 120);
    await page.waitForTimeout(100);
    expect(await atBottom(page)).toBeLessThan(2);
  });

  test("older messages loading in above don't move the view", async ({ page }) => {
    await open(page);
    await scrollBy(page, -3000);
    await list(page).evaluate((el) => (el.scrollTop = 0));
    await settle(page);
    // The first of what's loaded is on its way to the top edge, with older ones to come.
    const first = Number(await page.locator("[data-msg]").first().getAttribute("data-msg"));
    const was = (await top(page, first))!;
    const moved = await drift(page, first, async () => {
      await page.locator("[data-room-older]").click();
      await expect.poll(() => top(page, first - 1)).not.toBeNull();
    });
    expect(Math.abs(was)).toBeLessThan(80);
    expect(moved).toBeLessThanOrEqual(SLACK);
  });
}
