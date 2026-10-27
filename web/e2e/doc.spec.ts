// #963: a Doc beside a pane fills in on the page as the daemon gets each
// section, on a desktop and on a Pixel 7. Another spec's checks, run for
// WebKit's iPhone, are doc.webkit.spec.ts. Without the `docs` flag a saved
// Doc shows "This block is in Labs": labs-off.spec.ts.
//
// #964: a chart draws as SVG, a drawing shows through <img> and nothing
// else (a script in one runs nowhere), a chart that can't be drawn says so
// with its source, a viewer's page shows all of it, in the light theme and
// the dark. A chart fence in a forge's body stays code: forge-github.spec.ts.
//
// #965: a picture the agent saved shows from a data: URL (control's CSP has
// no blob:), a name written again shows the new one with no reload, any other
// image target stays text and is never fetched, a viewer's page shows it, and
// closing the block empties the page's cache.
//
// #254: GitHub's five alerts draw as callouts, each with its label, icon
// and tone, a plain quote stays a quote, and a stats fence draws tiles,
// light and dark, two columns on a phone. DOC_SHOTS=<dir> saves pictures.

import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ChildProcess } from "node:child_process";
import { expect, test, type Page } from "@playwright/test";
import { bring, brokenEmbeds, fill, fillEmbeds, fillsInPlace, imagesInPlace, newDoc, placedEmbeds, png, putAsset, seeEmbeds, type Post, widths } from "./doc-steps";
import { closeContexts, open, panes, ready, reset } from "./helpers";
import { daemon, pixel7 } from "./phones";
import { example } from "./doc-examples";

const SHOTS = process.env.DOC_SHOTS;
const ALERTS = ["note", "tip", "important", "warning", "caution"];
const CALLOUTS = [...ALERTS.map((a) => `> [!${a.toUpperCase()}]\n> The ${a} callout, with **bold** in it.`), "> A plain quote stays a quote."].join("\n\n");

/** Callouts in s1 and the grammar's tiles in s2, as the agent sends them. */
async function fillCallouts(post: Post, doc: number) {
  await fill(post, doc, "s1", CALLOUTS);
  await fill(post, doc, "s2", example("stats.md"));
}

/** What `fillCallouts` shows, and each callout's tone and each delta's. */
async function seeCallouts(page: Page, doc: number) {
  const block = page.locator(`[data-doc-block="${doc}"]`);
  const callouts = block.locator("[data-section=s1] [data-callout]");
  await expect(callouts).toHaveCount(5, { timeout: 15_000 });
  for (const [i, a] of ALERTS.entries()) {
    const c = callouts.nth(i);
    await expect(c).toHaveAttribute("data-callout", a);
    await expect(c.locator(".md-callout-title")).toHaveText(a[0].toUpperCase() + a.slice(1));
    await expect(c.locator("svg.md-callout-icon")).toHaveCount(1);
    await expect(c.locator("strong")).toHaveText("bold");
    await expect(c).not.toContainText("[!");
  }
  await expect(block.locator("[data-section=s1] blockquote")).toHaveText("A plain quote stays a quote.");
  const tiles = block.locator("[data-section=s2] .doc-stat");
  await expect(tiles).toHaveCount(3);
  await expect(tiles.nth(0)).toContainText("Sessions1,284↑ +12%");
  await expect(tiles.nth(1).locator("[data-tone]")).toHaveAttribute("data-tone", "down");
  await expect(tiles.nth(1).locator("[role=img]")).toHaveAttribute("aria-label", "down");
  await expect(tiles.nth(2).locator("[data-tone]")).toHaveCount(0);
  await expect(block.locator("[data-section=s2] [data-chart-note]")).toHaveCount(0);
  return page.evaluate((d) => {
    const q = (s: string) => document.querySelector(`[data-doc-block="${d}"] ${s}`)!;
    const style = (s: string) => getComputedStyle(q(s));
    return {
      tones: [...document.querySelectorAll(`[data-doc-block="${d}"] [data-callout]`)].map((c) => getComputedStyle(c).borderLeftColor),
      fill: style("[data-callout=note]").backgroundColor,
      paper: getComputedStyle(q(".doc-page").closest(".doc")!).backgroundColor,
      up: style(".doc-stat-delta.up [role=img]").color,
      down: style(".doc-stat-delta.down [role=img]").color,
      valueFont: style(".doc-stat-value").fontFamily,
      labelCase: style(".doc-stat dt").textTransform,
      top: [...document.querySelectorAll(`[data-doc-block="${d}"] .doc-stat`)].map((t) => Math.round(t.getBoundingClientRect().top)),
    };
  }, doc);
}

/** The Doc's page, or one section of it (a phone's pane scrolls), once a
 * fresh fill's highlight has faded. */
async function shoot(page: Page, doc: number, name: string, part = ".doc-page") {
  if (!SHOTS) return;
  mkdirSync(SHOTS, { recursive: true });
  await page.waitForFunction(() => !document.getAnimations().some((a) => (a as CSSAnimation).animationName === "doc-fresh" && a.playState === "running"));
  await page.locator(`[data-doc-block="${doc}"] ${part}`).screenshot({ path: join(SHOTS, `${name}.png`) });
}

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

  test("a picture fits the phone", async ({ page }) => {
    await open(page);
    const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
    await ready(page, term);
    const doc = await newDoc(post(page), term);
    await bring(page, doc);
    await putAsset(post(page), doc, "wide.png", png(900, 40));
    await fill(post(page), doc, "s1", "![A wide one](assets/wide.png)");
    await expect.poll(() => widths(page, doc, "s1")).toEqual([900]);
    const width = await page.evaluate(() => document.documentElement.clientWidth);
    const box = await page.locator(`[data-doc-block="${doc}"] img.doc-image`).boundingBox();
    expect(box!.width).toBeLessThanOrEqual(width);
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

test("a picture shows, a replaced one shows with no reload, and other images stay text", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await imagesInPlace(page, post(page), doc);
});

// What a person reads: the Doc's own colours (#253), in both schemes.
test("charts read in the light theme and the dark", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fillEmbeds(post(page), doc);
  const look = () =>
    page.evaluate((d) => {
      const block = document.querySelector(`[data-doc-block="${d}"]`)!;
      const probe = (c: string) => {
        const i = document.createElement("i");
        i.style.color = c;
        block.append(i);
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
        series: probe("var(--doc-leaf)"),
        app: probe("var(--info)"),
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
  expect(dark.bar, "the Doc's colours, not the app's").not.toBe(dark.app);

  await page.evaluate(() => localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "latte", finish: "flat", corners: "square" })));
  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.state !== null)).toBe(true);
  await seeEmbeds(page, doc);
  const light = await look();
  expect(light.scheme).toBe("light");
  expect(light.textLum, "dark text on the light page").toBeLessThan(0.5);
  expect(light.text).toBe(light.color);
  expect(light.bar).toBe(light.series);
  expect(light.bar, "the series has its own colour in each scheme").not.toBe(dark.bar);
});

test("callouts and stat tiles draw in the Doc's tones, light and dark", async ({ page }) => {
  // Wide enough that the Doc beside the pane has room for a row of tiles, tall enough to see it all.
  await page.setViewportSize({ width: 1600, height: 1800 });
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fillCallouts(post(page), doc);
  await bring(page, doc);
  const dark = await seeCallouts(page, doc);
  expect(new Set(dark.tones).size, "five tones").toBe(5);
  expect(dark.fill, "a tinted fill, not the paper").not.toBe(dark.paper);
  expect(dark.up).not.toBe(dark.down);
  expect(dark.valueFont).toContain("Big Shoulders Display");
  expect(dark.labelCase).toBe("uppercase");
  expect(new Set(dark.top).size, "one row on a desktop").toBe(1);
  await shoot(page, doc, "block-desktop-dark");

  await page.evaluate(() => localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "latte", finish: "flat", corners: "square" })));
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.state)).toBe(true);
  await bring(page, doc);
  const light = await seeCallouts(page, doc);
  expect(new Set(light.tones).size).toBe(5);
  expect(light.tones, "each scheme has its own tones").not.toEqual(dark.tones);
  expect(light.fill).not.toBe(light.paper);
  await shoot(page, doc, "block-desktop-light");
});

test.describe("callouts and tiles on a Pixel 7", () => {
  test.use(pixel7);
  test("tiles wrap to two columns", async ({ page }) => {
    await open(page);
    const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
    await ready(page, term);
    const doc = await newDoc(post(page), term);
    await bring(page, doc);
    await fillCallouts(post(page), doc);
    const seen = await seeCallouts(page, doc);
    // Three tiles: two in the first row, one in the second.
    expect(seen.top[0]).toBe(seen.top[1]);
    expect(seen.top[2]).toBeGreaterThan(seen.top[0]);
    await shoot(page, doc, "block-pixel7-callouts-dark", "[data-section=s1]");
    await shoot(page, doc, "block-pixel7-stats-dark", "[data-section=s2]");
  });
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

  test("sees a picture, which reading takes a viewer's call for, and can't put one", async ({ browser, page }) => {
    await open(page);
    const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
    await ready(page, term);
    const doc = await newDoc(post(page), term);
    const session = await page.evaluate((d) => {
      const c = window.__arugula.client;
      return c.sessionOfTab(c.tabOfPane(d)!.id);
    }, doc);
    expect((await page.request.post("/api/acl", { data: { session, principal: `tailnet:${FRIEND}`, role: "viewer" } })).ok()).toBe(true);
    await putAsset(post(page), doc, "flow.png", png(3, 2));
    await fill(post(page), doc, "s1", "The flow: ![The flow](assets/flow.png)");

    const friend = await (await browser.newContext({ baseURL: url, extraHTTPHeaders: { "tailscale-user-login": FRIEND } })).newPage();
    await friend.goto("/");
    await expect.poll(() => friend.evaluate(() => window.__arugula?.client.role())).toBe("viewer");
    await bring(friend, doc);
    await expect.poll(() => widths(friend, doc, "s1"), { timeout: 15_000 }).toEqual([3]);
    // They can't put one, or rewrite the section to show it differently.
    const put = await friend.request.post(`/api/blocks/${doc}/call/doc_asset_put`, { data: { name: "x.png", data: png(1, 1).toString("base64") } });
    expect(put.status()).toBe(403);
    const again = await friend.request.post(`/api/blocks/${doc}/call/doc_fill`, { data: { section: "s1", markdown: "Rewritten." } });
    expect(again.status()).toBe(403);
    await expect.poll(() => widths(friend, doc, "s1")).toEqual([3]);
    await friend.close();
  });
});
