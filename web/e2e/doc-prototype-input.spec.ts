// #507: input to a Doc's prototype, and Full window. A drag that starts in the
// frame goes on when the pointer leaves it; a touch drag in it moves the
// prototype's own element, not the Doc; Full window draws the same frame over
// the whole window and keeps its state. The WebKit twin (phone width) is
// doc-prototype-input.webkit.spec.ts.

import { expect, test, type Page } from "@playwright/test";
import { newDoc, type Post } from "./doc-steps";
import { counterKept, touchDragOut, fillPad, FRAME, padDragged, scrolled } from "./doc-prototype-input-steps";
import { DOMAIN, rig, share, visitor, type Rig } from "./doc-share-rig";
import { bring } from "./doc-steps";
import { panes, reset } from "./helpers";
import { pixel7 } from "./phones";
import "./local-token";

const post = (page: Page): Post => async (path, data) => {
  const r = await page.request.post(path, { data });
  const text = await r.text();
  expect(r.ok(), `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

async function padDoc(page: Page) {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fillPad(post(page), doc);
  await bring(page, doc);
  await expect(page.locator(FRAME)).toHaveCount(1);
  await expect(page.frameLocator(FRAME).locator("#pad")).toBeVisible();
  // The frame's hit-test data follows its first paint: a press before it goes to the page above.
  await page.waitForTimeout(500);
  return doc;
}

// A finger, not the mouse: with CDP's synthetic mouse, Chrome stops sending a
// sandboxed (opaque-origin, out-of-process) frame the pointer at its edge, and
// sends the release to the page above, though a finger and WebKit's mouse
// (doc-prototype-input.webkit.spec.ts) go on. Not seen with a real mouse.
test.describe("with a touch screen", () => {
  test.use({ hasTouch: true });
  test("a drag that starts in the frame goes on outside it and completes", async ({ page }) => {
    await padDoc(page);
    await page.locator(FRAME).scrollIntoViewIfNeeded();
    await page.waitForTimeout(500);
    await touchDragOut(page, 60);
    await padDragged(page);
  });
});

test("Full window keeps the counter, and closes on its ✕, on Esc, and on Esc inside the frame", async ({ page }) => {
  await padDoc(page);
  await counterKept(page, "button");
  await counterKept(page, "esc");
  await counterKept(page, "esc-in-frame");
});

// Reduced motion is the WebKit spec's: Chrome's DevTools emulation of it isn't
// handed to a sandboxed, out-of-process frame, so it can't show the frame's
// answer here (a person's own setting is the browser's, for every frame).

test.describe("on a Pixel 7", () => {
  test.use(pixel7);

  test("a touch drag in the frame moves the prototype's pad and not the Doc; two fingers scroll the Doc", async ({ page }) => {
    await padDoc(page);
    await page.locator(FRAME).scrollIntoViewIfNeeded();
    await page.waitForTimeout(500);
    const before = await scrolled(page);
    const box = (await page.locator(FRAME).boundingBox())!;
    const cdp = await page.context().newCDPSession(page);
    const touch = (type: string, points: { x: number; y: number; id: number }[]) =>
      cdp.send("Input.dispatchTouchEvent", { type, touchPoints: points.map((p) => ({ x: p.x, y: p.y, id: p.id })) });
    // One finger on the pad, down the page.
    const x = box.x + 60;
    const y = box.y + 60;
    await touch("touchStart", [{ x, y, id: 1 }]);
    for (let i = 1; i <= 10; i++) await touch("touchMove", [{ x, y: y + i * 8, id: 1 }]);
    await touch("touchEnd", []);
    const pad = page.frameLocator(FRAME).locator("#pad");
    await expect(pad).toHaveAttribute("data-state", "up");
    expect(await pad.evaluate((el) => (el as HTMLElement).style.transform), "the pad followed the finger").toMatch(/translate\(0px, ?[1-9]/);
    expect(await scrolled(page), "the Doc didn't move").toBe(before);
    // Two fingers off the pad, in the frame: the Doc scrolls.
    const ax = box.x + 250;
    const bx = box.x + 290;
    const top = box.y + 200;
    await touch("touchStart", [{ x: ax, y: top, id: 1 }, { x: bx, y: top, id: 2 }]);
    for (let i = 1; i <= 10; i++) await touch("touchMove", [{ x: ax, y: top - i * 12, id: 1 }, { x: bx, y: top - i * 12, id: 2 }]);
    await touch("touchEnd", []);
    await expect.poll(() => scrolled(page), "two fingers scroll the Doc").toBeGreaterThan(before);
  });
});

// On a shared Doc's page: click to run, then Full window works the same.
test.describe("on a shared page", () => {
  test.describe.configure({ mode: "serial" });
  let r: Rig;
  test.beforeAll(async () => {
    test.setTimeout(120_000);
    r = await rig(DOMAIN);
  });
  test.afterAll(async () => {
    await r?.dispose();
  });

  test("Full window works after Run prototype, and keeps the counter", async ({ browser }) => {
    const [{ id: term }] = (await (await fetch(`${r.mini}/api/panes`)).json()) as { id: number }[];
    const d = await newDoc(r.post, term, "Pad doc");
    await fillPad(r.post, d);
    const url = await share(r, d);
    const v = await visitor(browser);
    try {
      await v.page.goto(url);
      await v.page.locator("[data-prototype-run-button]").click();
      await expect(v.page.frameLocator(FRAME).locator("#pad")).toBeVisible({ timeout: 15_000 });
      await counterKept(v.page, "button");
      await counterKept(v.page, "esc");
    } finally {
      await v.ctx.close();
    }
  });
});
