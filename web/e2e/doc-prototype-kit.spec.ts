// #699: a prototype starts from the kit in the runner page (`runner.html`):
// `--ar-` colors that follow the Doc's scheme, and `ar-` classes drawn in the
// house look. A prototype that uses none of them is untouched. Colors are
// compared with appearance.ts's Slate and Latte by `scripts/prototype-kit.test.mjs`;
// here the frame's own computed styles say they reach it, in both schemes.

import { expect, test, type Page } from "@playwright/test";
import { fill, kitPrototype, newDoc, plainPrototype, type Post } from "./doc-steps";
import { panes, reset } from "./helpers";

const post = (page: Page): Post => async (path, data) => {
  const r = await page.request.post(path, { data });
  const text = await r.text();
  expect(r.ok(), `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

const rgb = (hex: string) => {
  const n = parseInt(hex.slice(1), 16);
  return `rgb(${n >> 16}, ${(n >> 8) & 255}, ${n & 255})`;
};

test("a prototype gets the kit's colors and classes in the Doc's scheme, and one that uses none is as before", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fill(post(page), doc, "s1", `${kitPrototype()}\n${plainPrototype()}`);
  const kit = page.frameLocator("[data-prototype-frame]").first();
  const plain = page.frameLocator("[data-prototype-frame]").last();
  await expect(kit.locator(".ar-dialog")).toBeVisible({ timeout: 15_000 });
  await expect(plain.locator("#p")).toHaveText("Plain");

  // Slate, the dark default: bg and accent from appearance.ts.
  const bg = () => kit.locator("body").evaluate((el) => getComputedStyle(el).backgroundColor);
  await expect.poll(bg).toBe(rgb("#0c1118"));
  const primary = kit.locator(".ar-btn-primary").first();
  expect(await primary.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(rgb("#56c8dc"));

  // The scheme moving while it is open moves the kit with it, no reload.
  await page.evaluate(() => (document.documentElement.dataset.scheme = "light"));
  await expect.poll(bg).toBe(rgb("#eff1f5"));
  expect(await primary.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(rgb("#702fc6"));

  // Opt-in: a page with no ar- class has no kit rule on it.
  const p = await plain.locator("#b").evaluate((el) => {
    const s = getComputedStyle(el);
    return [s.minHeight, s.borderRadius, getComputedStyle(document.body).margin];
  });
  expect(p).toEqual(["0px", "0px", "8px"]);
});
