// #963: a Doc beside a pane fills in on the page as the daemon gets each
// section, on a desktop and on a Pixel 7. Another spec's checks, run for
// WebKit's iPhone, are doc.webkit.spec.ts. Without the `docs` flag a saved
// Doc shows "This block is in Labs": labs-off.spec.ts.

import { expect, test, type Page } from "@playwright/test";
import { fillsInPlace, type Post } from "./doc-steps";
import { open, panes, ready, reset } from "./helpers";
import { pixel7 } from "./phones";

const post = (page: Page): Post => async (path, data) => {
  const r = await page.request.post(path, { data });
  const text = await r.text();
  expect(r.ok(), `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

test("the outline shows every intent, and each fill appears with no reload", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  await fillsInPlace(page, post(page), term);
});

test("a Doc is a block beside the pane, with its title for a name", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const made = await post(page)("/api/blocks", { type: "doc", config: { title: "Notes" }, split: term, local: true });
  await expect.poll(() => panes(page)).toHaveLength(2);
  const doc = made.block!;
  await expect(page.locator(`[data-doc-block="${doc}"]`)).toBeVisible();
  await expect(page.locator(`[data-doc-block="${doc}"]`)).toContainText("Waiting for the outline");
  await expect
    .poll(() => page.evaluate((d) => window.__arugula.client.state!.panes.find((p) => p.id === d)?.type, doc))
    .toBe("doc");
});

test.describe("on a Pixel 7", () => {
  test.use(pixel7);
  test("the same, with the Doc up on the phone", async ({ page }) => {
    await open(page);
    const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
    await ready(page, term);
    await fillsInPlace(page, post(page), term);
  });
});
