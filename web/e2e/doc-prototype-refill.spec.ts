// #509: filling a ```prototype section again updates its frame in place for
// everyone who has the Doc open: the Doc block beside the agent, a second
// client on the session (a phone too), and a shared link's page once the owner
// taps Update. A refill never republishes the link by itself.

import { expect, test, type Page } from "@playwright/test";
import { bring, fill, newDoc, type Post } from "./doc-steps";
import { DOMAIN, rig, share, visitor } from "./doc-share-rig";
import { closeContexts, open, panes, reset } from "./helpers";
import { pixel7 } from "./phones";

const body = (v: string) => `Before.\n\n\`\`\`prototype height=120 Probe\n<p id="v">${v}</p>\n\`\`\`\n\nAfter.`;

const post = (page: Page): Post => async (path, data) => {
  const r = await page.request.post(path, { data });
  const text = await r.text();
  expect(r.ok(), `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

test.afterAll(closeContexts);

test("a refill reloads the frame in the Doc block and on a phone on the same session, within a second", async ({ page, browser }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fill(post(page), doc, "s1", body("one"));
  const phone = await browser.newContext({ ...pixel7 });
  const second = await phone.newPage();
  await open(second);
  await bring(second, doc);

  const frame = (p: Page) => p.locator(`[data-doc-block="${doc}"] [data-prototype-frame]`);
  const shows = (p: Page, v: string, timeout = 15_000) => expect(p.frameLocator(`[data-doc-block="${doc}"] [data-prototype-frame]`).locator("#v")).toHaveText(v, { timeout });
  await shows(page, "one");
  await shows(second, "one");
  await fill(post(page), doc, "s1", body("two"));
  await shows(page, "two", 1_000);
  await shows(second, "two", 1_000);
  // One frame each: the new one replaced the old.
  await expect(frame(page)).toHaveCount(1);
  await expect(frame(second)).toHaveCount(1);
});

test.describe("a shared Doc", () => {
  test.describe.configure({ mode: "serial" });
  let r: Awaited<ReturnType<typeof rig>>;
  test.beforeAll(async () => {
    test.setTimeout(120_000);
    r = await rig(DOMAIN);
  });
  test.afterAll(async () => {
    await r?.dispose();
  });

  test("says its copy is older after a refill, and an Update reaches an open viewer with its prototype still running", async ({ page, browser }) => {
    const [{ id: term }] = (await (await fetch(`${r.mini}/api/panes`)).json()) as { id: number }[];
    const d = await newDoc(r.post, term, "Refill doc");
    await fill(r.post, d, "s1", body("one"));
    const url = await share(r, d);
    const meta = async () => (await (await fetch(`${url.split("#")[0]}/data/meta`)).json()) as { ver: number };

    await page.goto(r.mini);
    const block = page.locator(`[data-doc-block="${d}"]`);
    await expect(block.locator(".doc-title")).toBeVisible({ timeout: 20_000 });
    // The bar is the block's header, beside the body.
    const older = page.locator("[data-doc-bar-older]");
    await expect(page.locator("[data-doc-bar-shared]")).toBeVisible();
    await expect(older).toHaveCount(0);

    // A viewer on the link, with a clock of its own to drive its checks.
    const v = await visitor(browser);
    await v.page.clock.install();
    await v.page.goto(url);
    await expect(v.page.locator(".doc-title")).toHaveText("Refill doc", { timeout: 15_000 });
    await v.page.locator("[data-prototype-run-button]").click();
    const inside = v.page.frameLocator("[data-prototype-frame]");
    await expect(inside.locator("#v")).toHaveText("one", { timeout: 15_000 });
    await v.page.evaluate(() => ((window as unknown as { __same: boolean }).__same = true));

    // A refill: the block shows the new version and the older-copy prompt,
    // and the link is as it was.
    await fill(r.post, d, "s1", body("two"));
    await expect(older).toHaveText("Shared copy is older · Update");
    await expect(page.frameLocator(`[data-doc-block="${d}"] [data-prototype-frame]`).locator("#v")).toHaveText("two");
    expect((await meta()).ver).toBe(1);
    await v.page.clock.runFor(31_000);
    await expect(inside.locator("#v")).toHaveText("one");

    // One tap on it: the same link is version 2, the prompt goes.
    await older.click();
    await expect.poll(async () => (await meta()).ver).toBe(2);
    await expect(older).toHaveCount(0);

    // The open viewer picks it up at its next check: swapped in place (the
    // page is the one it was), still running without a click, one frame.
    await v.page.clock.runFor(31_000);
    await expect(inside.locator("#v")).toHaveText("two", { timeout: 15_000 });
    await expect(v.page.locator("[data-prototype-run]")).toHaveCount(0);
    await expect(v.page.locator("[data-prototype-frame]")).toHaveCount(1);
    expect(await v.page.evaluate(() => (window as unknown as { __same?: boolean }).__same)).toBe(true);
    await v.ctx.close();
  });
});
