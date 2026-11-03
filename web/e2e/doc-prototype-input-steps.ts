// What #507's specs share (doc-prototype-input.spec.ts and its WebKit twin):
// a prototype with a draggable pad and a counter, and the checks on it.

import { expect, type Page } from "@playwright/test";
import { fill, type Post } from "./doc-steps";

/** A pad that takes a pointer by `setPointerCapture` and moves with it (its
 * own `touch-action: none`), a counter, and the media query the frame sees. */
export const PAD = `\`\`\`prototype height=300 Pad
<!doctype html>
<html>
<body style="margin:0">
<div id="pad" style="touch-action:none;position:absolute;left:10px;top:10px;width:100px;height:100px;background:#c44"></div>
<button id="inc" style="position:absolute;left:150px;top:10px;padding:10px">+</button>
<span id="n" style="position:absolute;left:220px;top:18px">0</span>
<span id="rm" style="position:absolute;left:150px;top:80px"></span>
<script>
var pad = document.getElementById("pad"), n = 0, x0 = 0, y0 = 0;
pad.addEventListener("pointerdown", function (e) { pad.setPointerCapture(e.pointerId); x0 = e.clientX; y0 = e.clientY; pad.dataset.state = "down"; });
pad.addEventListener("pointermove", function (e) {
  if (!pad.hasPointerCapture(e.pointerId)) return;
  pad.style.transform = "translate(" + (e.clientX - x0) + "px," + (e.clientY - y0) + "px)";
  pad.dataset.maxy = Math.max(+pad.dataset.maxy || 0, e.clientY);
});
pad.addEventListener("pointerup", function () { pad.dataset.state = "up"; });
pad.addEventListener("pointercancel", function () { pad.dataset.state = "cancel"; });
document.getElementById("inc").onpointermove = function (e) { e.target.dataset.seen = "1"; };
document.getElementById("inc").onclick = function () { document.getElementById("n").textContent = ++n; };
document.getElementById("rm").textContent = matchMedia("(prefers-reduced-motion: reduce)").matches ? "reduced" : "full";
</script>
</body>
</html>
\`\`\`
`;

/** A Doc with room to scroll above and below the pad. */
export const padDoc = `${Array.from({ length: 12 }, (_, i) => `Line ${i} before.\n`).join("\n")}\n${PAD}\n${Array.from({ length: 30 }, (_, i) => `Line ${i} after.\n`).join("\n")}`;

export const FRAME = "[data-prototype-frame]";

/** Wait until the page's hit test at the centre of `inner` (an element in the
 * frame, which sets `data-seen` on a pointermove) is the frame itself and the
 * frame has seen the pointer there: a press before then goes to the page above. */
export async function frameAnswers(page: Page, inner: string) {
  await expect
    .poll(async () => {
      // As a click would: bring it into view (a toolbar can cover the frame's top).
      await page.frameLocator(FRAME).locator(inner).scrollIntoViewIfNeeded();
      const at = await page.frameLocator(FRAME).locator(inner).boundingBox();
      if (!at) return "no box";
      return page.evaluate(
        ([x, y, sel]) => {
          const hit = document.elementFromPoint(x as number, y as number);
          return hit?.matches(sel as string) ? "frame" : (hit?.outerHTML.slice(0, 100) ?? "nothing");
        },
        [at.x + at.width / 2, at.y + at.height / 2, FRAME],
      );
    }, { message: `the frame is under ${inner}` })
    .toBe("frame");
  const el = page.frameLocator(FRAME).locator(inner);
  await el.evaluate((e) => delete (e as HTMLElement).dataset.seen);
  const at = (await el.boundingBox())!;
  let nudge = 0;
  await expect
    .poll(async () => {
      await page.mouse.move(at.x + at.width / 2 + (nudge++ % 2), at.y + at.height / 2);
      return el.getAttribute("data-seen");
    }, { message: `the frame saw the pointer on ${inner}` })
    .toBe("1");
}

export const fillPad = (post: Post, doc: number) => fill(post, doc, "s1", padDoc);

/** How far the Doc (anything that scrolls above the frame) is scrolled. */
export const scrolled = (page: Page) =>
  page.evaluate((sel) => {
    let sum = 0;
    for (let el = document.querySelector(sel)?.parentElement ?? null; el; el = el.parentElement) sum += el.scrollTop;
    return sum + window.scrollY;
  }, FRAME);

/** Drag the pad with the mouse from inside the frame to `dy` below the frame
 * and let go there, outside the frame's box. (The pause is the first hover:
 * a press before the frame has seen the pointer goes to the page above.) */
export async function dragOut(page: Page, dy: number) {
  const box = (await page.locator(FRAME).boundingBox())!;
  const x = box.x + 60;
  const y = box.y + 60;
  await page.mouse.move(x, y);
  await page.waitForTimeout(300);
  await page.mouse.down();
  await page.mouse.move(x, y + 40, { steps: 4 });
  await page.mouse.move(x + 20, box.y + box.height + dy, { steps: 8 });
  await page.mouse.up();
  return box;
}

/** The same drag with a finger (DevTools touch events, which Chrome has and
 * Playwright's WebKit doesn't). */
export async function touchDragOut(page: Page, dy: number) {
  const box = (await page.locator(FRAME).boundingBox())!;
  const cdp = await page.context().newCDPSession(page);
  const x = box.x + 60;
  const touch = (type: string, y?: number) =>
    cdp.send("Input.dispatchTouchEvent", { type, touchPoints: y === undefined ? [] : [{ x, y, id: 1 }] });
  await touch("touchStart", box.y + 60);
  for (let y = box.y + 70; y <= box.y + box.height + dy; y += 10) await touch("touchMove", y);
  await touch("touchEnd");
}

/** The pad saw the drag through: it went down inside, followed the pointer
 * past the frame's bottom edge, and got the pointerup out there. */
export async function padDragged(page: Page) {
  const pad = page.frameLocator(FRAME).locator("#pad");
  await expect(pad).toHaveAttribute("data-state", "up");
  expect(+(await pad.getAttribute("data-maxy"))!, "the drag went on past the frame's edge").toBeGreaterThan(300);
}

/** Counter 0 -> 1 -> 2 across Full window's open and close: the same frame, never reloaded. */
export async function counterKept(page: Page, close: "button" | "esc" | "esc-in-frame") {
  const inside = page.frameLocator(FRAME);
  const box = page.locator("[data-prototype-box]");
  await page.locator(FRAME).scrollIntoViewIfNeeded();
  const n0 = +((await inside.locator("#n").textContent()) ?? "0");
  await frameAnswers(page, "#inc");
  await inside.locator("#inc").click();
  await expect(inside.locator("#n")).toHaveText(String(n0 + 1));
  const small = (await page.locator(FRAME).boundingBox())!;
  await page.waitForTimeout(300);
  await page.locator("[data-prototype-full]").click();
  await expect.poll(() => box.evaluate((el) => el.matches(":popover-open"))).toBe(true);
  const win = page.viewportSize()!;
  await expect.poll(async () => (await page.locator(FRAME).boundingBox())!.width).toBe(win.width);
  const big = (await page.locator(FRAME).boundingBox())!;
  // Arugula #620: the close button has a strip of its own; the page takes the rest.
  const strip = (await page.locator("[data-prototype-box] .doc-prototype-bar").boundingBox())!;
  expect(big.y, "the page begins under the strip").toBeCloseTo(strip.y + strip.height, 0);
  expect(big.y + big.height, "it reaches the window's bottom").toBe(win.height);
  expect([big.x, strip.y]).toEqual([0, 0]);
  expect(big.height).toBeGreaterThan(small.height);
  await expect(inside.locator("#n"), "state kept going in").toHaveText(String(n0 + 1));
  await frameAnswers(page, "#inc");
  await inside.locator("#inc").click();
  await expect(inside.locator("#n")).toHaveText(String(n0 + 2));
  if (close === "button") await page.locator("[data-prototype-full-close]").click();
  else if (close === "esc") await page.keyboard.press("Escape");
  else {
    await inside.locator("#inc").focus();
    await page.keyboard.press("Escape");
  }
  await expect.poll(() => box.evaluate((el) => el.matches(":popover-open"))).toBe(false);
  expect((await page.locator(FRAME).boundingBox())!.height, "back to its height").toBe(300);
  // A press right after the layout moved can reach the page above, not the frame.
  await frameAnswers(page, "#inc");
  await expect(inside.locator("#n"), "no reload: the counter is where it was").toHaveText(String(n0 + 2));
  await inside.locator("#inc").click();
  await expect(inside.locator("#n"), "and the frame still answers").toHaveText(String(n0 + 3));
}
