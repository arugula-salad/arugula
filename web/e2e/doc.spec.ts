// #963: a Doc beside a pane fills in on the page as the daemon gets each
// section, on a desktop and on a Pixel 7. Another spec's checks, run for
// WebKit's iPhone, are doc.webkit.spec.ts. Without the `docs` flag a saved
// Doc shows "This block is in Labs": labs-off.spec.ts.
//
// #964: a chart draws as SVG, a drawing shows through <img> and nothing
// else (a script in one runs nowhere), a chart that can't be drawn says so
// with its source, a viewer's page shows all of it, in the light theme and
// the dark. A chart fence in a forge's body stays code: forge-github.spec.ts.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ChildProcess } from "node:child_process";
import { expect, test, type Page } from "@playwright/test";
import { bring, brokenEmbeds, fillEmbeds, fillsInPlace, newDoc, placedEmbeds, seeEmbeds, type Post } from "./doc-steps";
import { closeContexts, open, panes, ready, reset } from "./helpers";
import { daemon, pixel7 } from "./phones";

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

  test("a chart and a drawing fit the phone", async ({ page }) => {
    await open(page);
    const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
    await ready(page, term);
    const doc = await newDoc(post(page), term);
    await bring(page, doc);
    await fillEmbeds(post(page), doc);
    await seeEmbeds(page, doc);
    // Nothing is wider than the page: the chart and the pictures shrink to it.
    const width = await page.evaluate(() => document.documentElement.clientWidth);
    for (const el of await page.locator(`[data-doc-block="${doc}"] svg.chart, [data-doc-block="${doc}"] img.doc-drawing`).all()) {
      expect((await el.boundingBox())!.width).toBeLessThanOrEqual(width);
    }
  });
});

test("a chart draws, drawings show as pictures, and a script in one runs nothing", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fillEmbeds(post(page), doc);
  await seeEmbeds(page, doc);
  // The Doc's text has a line for a chart and for a drawing.
  await expect
    .poll(() => page.evaluate((d) => window.__arugula.text(d), doc))
    .toContain("[chart: Sales by month (bar, 3 rows)]");
  expect(await page.evaluate((d) => window.__arugula.text(d), doc)).toContain("[drawing]");
});

test("a drawing that can't be shown says why, and fences sit where the daemon looks", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fillEmbeds(post(page), doc);
  await seeEmbeds(page, doc);
  await placedEmbeds(page, post(page), doc);
  await brokenEmbeds(page, post(page), doc);
});

// What a person reads: colours the page's palette gives, in both.
test("charts read in the light theme and the dark", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fillEmbeds(post(page), doc);
  const look = () =>
    page.evaluate((d) => {
      const probe = (c: string) => {
        const i = document.createElement("i");
        i.style.color = c;
        document.body.append(i);
        const v = getComputedStyle(i).color;
        i.remove();
        return v;
      };
      const svg = document.querySelector(`[data-doc-block="${d}"] svg.chart`)!;
      const lum = (rgb: string) => {
        const [r, g, b] = rgb.match(/[\d.]+/g)!.map(Number);
        return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
      };
      const text = getComputedStyle(svg.querySelector("text")!).fill;
      return {
        scheme: document.documentElement.dataset.scheme,
        text,
        textLum: lum(text),
        bar: getComputedStyle(svg.querySelector("rect[data-bar]")!).fill,
        series: probe("var(--info)"),
        grid: getComputedStyle(svg.querySelector("line")!).stroke,
        color: getComputedStyle(svg).color,
      };
    }, doc);
  await seeEmbeds(page, doc);
  const dark = await look();
  expect(dark.scheme).toBe("dark");
  expect(dark.textLum, "light text on the dark page").toBeGreaterThan(0.5);
  expect(dark.text).toBe(dark.color);
  expect(dark.grid).toBe(dark.color);
  expect(dark.bar).toBe(dark.series);

  await page.evaluate(() => localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "latte", finish: "flat", corners: "square" })));
  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.state !== null)).toBe(true);
  await seeEmbeds(page, doc);
  const light = await look();
  expect(light.scheme).toBe("light");
  expect(light.textLum, "dark text on the light page").toBeLessThan(0.5);
  expect(light.text).toBe(light.color);
  expect(light.bar).toBe(light.series);
  expect(light.bar, "the series has its own colour in each palette").not.toBe(dark.bar);
});

test.describe("a viewer", () => {
  const OWNER = "me@example.com";
  const FRIEND = "friend@example.com";
  let dir = "";
  let url = "";
  let proc: ChildProcess | undefined;
  test.use({ baseURL: async ({}, use) => use(url) });

  test.beforeAll(async () => {
    dir = mkdtempSync(join(tmpdir(), "ilg-e2e-doc-view-"));
    ({ proc, url } = await daemon(join(dir, "state"), ["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"]));
  });

  test.afterAll(async ({ browser }) => {
    await closeContexts({ browser });
    proc?.kill("SIGKILL");
    if (dir) rmSync(dir, { recursive: true, force: true });
  });

  test("sees the chart and the drawings, and can't change the Doc", async ({ browser, page }) => {
    await open(page);
    const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
    await ready(page, term);
    const doc = await newDoc(post(page), term);
    const session = await page.evaluate((d) => {
      const c = window.__arugula.client;
      return c.sessionOfTab(c.tabOfPane(d)!.id);
    }, doc);
    expect((await page.request.post("/api/acl", { data: { session, principal: `tailnet:${FRIEND}`, role: "viewer" } })).ok()).toBe(true);
    await fillEmbeds(post(page), doc);

    const friend = await (await browser.newContext({ baseURL: url, extraHTTPHeaders: { "tailscale-user-login": FRIEND } })).newPage();
    await friend.goto("/");
    await expect.poll(() => friend.evaluate(() => window.__arugula?.client.role())).toBe("viewer");
    await bring(page, doc);
    await bring(friend, doc);
    await seeEmbeds(friend, doc);
    // They can't write: the owner's tool is refused, and the Doc is the same.
    const r = await friend.request.post(`/api/blocks/${doc}/call/doc_fill`, { data: { section: "s1", markdown: "Rewritten." } });
    expect(r.status()).toBe(403);
    await seeEmbeds(friend, doc);
    await friend.close();
  });
});
