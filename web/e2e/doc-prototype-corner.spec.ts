// Arugula #620: the chips over a prototype ("⤢ Full window", "Prototype ·
// sandboxed") sat on the frame's top-right corner and took the clicks of
// whatever the prototype pinned there. They are a strip above the frame now.
// The check is on effects: a button the prototype pins at `position: fixed;
// top: 4px; right: 4px` is what `elementFromPoint` finds at its centre (the
// frame, not a chip), and a click there reaches it. Inline and in Full
// window, on a desktop and a Pixel 7. The Full window and close buttons
// still work, and the strip never overlaps the frame.

import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { bring, fill, newDoc, type Post } from "./doc-steps";
import { panes, reset } from "./helpers";
import { pixel7 } from "./phones";

const SHOTS = process.env.DOC_PROTOTYPE_SHOTS;

const post = (page: Page): Post => async (path, data) => {
  const r = await page.request.post(path, { data });
  const text = await r.text();
  expect(r.ok(), `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

const CORNER = `\`\`\`prototype height=300 Corner
<!doctype html>
<html>
<body style="margin:0">
<button id="corner" style="position:fixed;top:4px;right:4px;padding:6px 10px">Copy</button>
<span id="n">0</span>
<script>
var n = 0;
document.getElementById("corner").onclick = function () { document.getElementById("n").textContent = ++n; };
</script>
</body>
</html>
\`\`\`
`;

const FRAME = "[data-prototype-frame]";
const BAR = ".doc-prototype-bar";

async function cornerDoc(page: Page) {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fill(post(page), doc, "s1", `Before.\n\n${CORNER}\nAfter.\n`);
  await bring(page, doc);
  await expect(page.locator(FRAME)).toHaveCount(1);
  await expect(page.frameLocator(FRAME).locator("#corner")).toBeVisible();
  // The frame's hit-test data follows its first paint: a press before it goes to the page above.
  await page.waitForTimeout(500);
}

/** What the page hits at the corner button's centre is the frame; a click there reaches the button. */
async function cornerReaches(page: Page, clicks: number) {
  const inside = page.frameLocator(FRAME);
  const at = (await inside.locator("#corner").boundingBox())!;
  const x = at.x + at.width / 2;
  const y = at.y + at.height / 2;
  const hit = await page.evaluate(([px, py]) => document.elementFromPoint(px, py)?.matches("[data-prototype-frame]") ?? document.elementFromPoint(px, py)?.outerHTML.slice(0, 80), [x, y]);
  expect(hit, "the frame is under the corner button, not a chip").toBe(true);
  await page.mouse.click(x, y);
  await expect(inside.locator("#n"), "the click reached the prototype's button").toHaveText(String(clicks));
}

/** The strip is above the frame: no pixel of the frame is under it. */
async function stripAbove(page: Page) {
  const bar = (await page.locator(BAR).boundingBox())!;
  const frame = (await page.locator(FRAME).boundingBox())!;
  expect(bar.y + bar.height, "the strip ends where the frame begins").toBeLessThanOrEqual(frame.y + 0.5);
}

async function run(page: Page, name: string) {
  await cornerDoc(page);
  await page.locator(FRAME).scrollIntoViewIfNeeded();
  await page.waitForTimeout(300);
  await stripAbove(page);
  await cornerReaches(page, 1);
  if (SHOTS) {
    mkdirSync(SHOTS, { recursive: true });
    await page.screenshot({ path: join(SHOTS, `prototype-inline-${name}.png`) });
  }

  // Full window: the page below a strip of its own, the corner button still the frame's.
  await page.locator("[data-prototype-full]").click();
  const box = page.locator("[data-prototype-box]");
  await expect.poll(() => box.evaluate((el) => el.matches(":popover-open"))).toBe(true);
  const win = page.viewportSize()!;
  await expect.poll(async () => (await page.locator(FRAME).boundingBox())!.width).toBe(win.width);
  await page.waitForTimeout(300);
  await stripAbove(page);
  const frame = (await page.locator(FRAME).boundingBox())!;
  expect(frame.y + frame.height, "the page reaches the window's bottom").toBe(win.height);
  await cornerReaches(page, 2);
  if (SHOTS) await page.screenshot({ path: join(SHOTS, `prototype-full-${name}.png`) });

  // The close button works, and the inline frame is back at its height.
  await page.locator("[data-prototype-full-close]").click();
  await expect.poll(() => box.evaluate((el) => el.matches(":popover-open"))).toBe(false);
  expect((await page.locator(FRAME).boundingBox())!.height).toBe(300);
  await page.waitForTimeout(300);
  await cornerReaches(page, 3);
}

test("a button pinned at a prototype's top-right corner takes the click, inline and in Full window", async ({ page }) => {
  await run(page, "desktop");
});

test.describe("on a Pixel 7", () => {
  test.use(pixel7);
  test("the same at phone width", async ({ page }) => {
    await run(page, "pixel7");
  });
});
