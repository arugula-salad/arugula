// #507 on an iPhone (WebKit, docs/testing.md "Phones"): a drag that starts in
// a Doc's prototype goes on outside the frame, and Full window draws the same
// frame over the whole screen, unclipped by the panes' overflow, keeping its
// state. Playwright's WebKit has no touch drag (only taps), so the touch-drag
// check is the Pixel 7's in doc-prototype-input.spec.ts.

import { rmSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ChildProcess } from "node:child_process";
import { expect, test } from "@playwright/test";
import { bring, newDoc, type Post } from "./doc-steps";
import { counterKept, dragOut, fillPad, FRAME, padDragged } from "./doc-prototype-input-steps";
import { open, ready } from "./helpers";
import { daemon, iphone } from "./phones";

let dir = "";
let url = "";
let proc: ChildProcess | undefined;

test.describe.configure({ mode: "serial" });
test.use({ ...iphone, baseURL: async ({}, use) => use(url) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "ilg-e2e-proto-ios-"));
  ({ proc, url } = await daemon(join(dir, "state"), ["--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"]));
});

test.afterAll(() => {
  proc?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

const post: Post = async (path, body) => {
  const r = await fetch(url + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const text = await r.text();
  expect(r.ok, `${path}: ${text}`).toBe(true);
  return JSON.parse(text) as { block?: number };
};

// The tests share one daemon, so the last test's Doc and its frame would
// still be in the session: on the phone, `bring` swaps panes, and FRAME could
// find the old frame just before it goes. Each test closes its own Doc.
let last: number | undefined;
test.afterEach(async () => {
  if (last !== undefined) await post(`/api/panes/${last}/close`, {}).catch(() => null);
  last = undefined;
});

async function padDoc(page: import("@playwright/test").Page) {
  await open(page);
  const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, term);
  const doc = await newDoc(post, term);
  last = doc;
  await fillPad(post, doc);
  await bring(page, doc);
  await expect(page.locator(FRAME)).toHaveCount(1);
  // The frame is lazy: it loads once it is near the screen.
  await page.locator(FRAME).scrollIntoViewIfNeeded();
  await expect(page.frameLocator(FRAME).locator("#pad")).toBeVisible({ timeout: 15_000 });
  await page.waitForTimeout(500);
}

test("a drag from inside the frame goes on outside it in WebKit", async ({ page }) => {
  await padDoc(page);
  await page.locator(FRAME).scrollIntoViewIfNeeded();
  await dragOut(page, 40);
  await padDragged(page);
});

test("Full window covers the screen in WebKit and keeps the counter", async ({ page }) => {
  await padDoc(page);
  await counterKept(page, "button");
  await counterKept(page, "esc");
});

test.describe("with reduced motion", () => {
  test.use({ reducedMotion: "reduce" });
  test("reduced motion reaches the frame", async ({ page }) => {
    await padDoc(page);
    await expect(page.frameLocator(FRAME).locator("#rm")).toHaveText("reduced");
  });
});
