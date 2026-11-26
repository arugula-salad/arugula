// Arugula #1451: a Doc's prototypes link to each other by file name; the
// steps are doc-steps.ts's, and WebKit's run them in doc.webkit.spec.ts.

import { expect, test, type Page } from "@playwright/test";
import { protoNavigates, type Post } from "./doc-steps";
import { panes, reset } from "./helpers";

const post = (page: Page): Post => async (path, data) => {
  const r = await page.request.post(path, { data });
  const text = await r.text();
  expect(r.ok(), `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

test("a prototype goes to another of its Doc's by file name, and only to those", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  await protoNavigates(page, post(page), term);
});
