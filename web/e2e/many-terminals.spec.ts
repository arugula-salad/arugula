// #1049: many terminals opened quickly. Tabs made faster than the page
// draws them once swapped the shown tab back and forth for good (a stale
// focus effect), the page too busy to answer; and every terminal shown
// kept its WebGL context after it was parked, so Chrome lost the oldest
// past 16. The page answers, every terminal draws its own text at its
// layout's size, and the contexts stay under the cap.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, reset } from "./helpers";
import type { PaneId } from "../src/proto";

test.afterAll(closeContexts);

/** Must match MAX_WEBGL in terminal-view.ts. */
const MAX_WEBGL = 12;

/** Keeps the page's WebGL contexts, to count those not lost (given back,
 * or taken by the browser). */
async function countContexts(page: Page) {
  await page.addInitScript(() => {
    const all: WebGLRenderingContext[] = [];
    (window as unknown as { __gl: typeof all }).__gl = all;
    const get = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, kind: string, ...rest: unknown[]) {
      const ctx = (get as (this: HTMLCanvasElement, ...a: unknown[]) => RenderingContext | null).call(this, kind, ...rest);
      if (ctx && kind.startsWith("webgl") && !all.includes(ctx as WebGLRenderingContext)) all.push(ctx as WebGLRenderingContext);
      return ctx;
    } as typeof get;
  });
}

const live = (page: Page) =>
  page.evaluate(() => (window as unknown as { __gl: WebGLRenderingContext[] }).__gl.filter((gl) => !gl.isContextLost()).length);

/** The page answers within a second (a page stuck redrawing doesn't). */
async function answers(page: Page) {
  const ok = await Promise.race([
    page.evaluate(() => true),
    new Promise<false>((r) => setTimeout(() => r(false), 1000)),
  ]);
  expect(ok, "the page answers").toBe(true);
}

/** A pane shown has its marker (in its buffer: a tab sized after its
 * shell started can have scrolled it up), is at its layout's size, and
 * draws: on WebGL's canvas, or in the DOM renderer's rows. */
async function draws(page: Page, pane: PaneId, marker: string) {
  await expect.poll(() => page.evaluate((p) => window.__arugula.text(p), pane)).toContain(marker);
  const seen = await page.evaluate((p) => {
    const c = window.__arugula.client;
    const r = c.tabOfPane(p)?.layout.panes.find(([id]) => id === p)?.[1];
    const slot = document.querySelector<HTMLElement>(`.pane[data-pane="${p}"]`);
    const screen = slot?.querySelector<HTMLElement>(".xterm-screen");
    const canvas = slot?.querySelector<HTMLCanvasElement>(".xterm-screen > canvas:not([class])");
    return {
      size: window.__arugula.size(p),
      layout: r ? [r.cols, r.rows] : null,
      shown: !!screen && screen.offsetWidth > 0 && screen.offsetHeight > 0,
      webgl: window.__arugula.webgl(p),
      canvas: !!canvas && canvas.width > 0 && canvas.height > 0,
      rows: slot?.querySelector(".xterm-rows")?.textContent ?? "",
    };
  }, pane);
  expect(seen.size, `pane ${pane}'s size`).toEqual(seen.layout);
  expect(seen.shown, `pane ${pane} is on screen`).toBe(true);
  if (seen.webgl) expect(seen.canvas, `pane ${pane}'s WebGL canvas`).toBe(true);
  else expect(seen.rows, `pane ${pane}'s DOM rows`).toContain("$");
}

/** Types `echo $((pane+1000))` once the pane's shell is up: what it
 * prints (short, to fit a small pane) isn't in what it types. */
async function mark(page: Page, pane: PaneId): Promise<string> {
  await expect.poll(async () => (await (await page.request.get(`/api/panes/${pane}/capture`)).text()).includes("$")).toBe(true);
  await page.request.post(`/api/panes/${pane}/send`, { data: { text: `echo $((${pane}+1000))`, enter: true } });
  return `${pane + 1000}`;
}

test("thirty tabs opened at once: the page answers, each draws, contexts stay capped", async ({ page }) => {
  test.setTimeout(120_000);
  const warnings: string[] = [];
  page.on("console", (m) => {
    if (/Too many active WebGL contexts/.test(m.text())) warnings.push(m.text());
  });
  await countContexts(page);
  await reset(page);
  // As fast as the daemon makes them: the page shows each as it comes.
  const panes = await Promise.all(
    Array.from({ length: 30 }, async () => (await (await page.request.post("/api/run", { data: {} })).json()).pane as PaneId),
  );
  const marks = await Promise.all(panes.map((p) => mark(page, p)));
  await answers(page);
  // Through every tab quickly, twice.
  for (let round = 0; round < 2; round++) {
    for (const p of panes) {
      await page.evaluate((p) => window.__arugula.client.setActive(p), p);
      await page.waitForTimeout(10);
    }
    await answers(page);
  }
  // Each one shown draws its own text.
  for (const [i, p] of panes.entries()) {
    await page.evaluate((p) => window.__arugula.client.setActive(p), p);
    await draws(page, p, marks[i]);
  }
  // One tab shown: one context (the measuring terminal's is given back).
  await expect.poll(() => live(page)).toBeLessThanOrEqual(1);
  expect(warnings).toEqual([]);
});

test("a grid with more panes than contexts: the panes used last draw with WebGL, the rest with the DOM, all of them drawn", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1900, height: 1200 });
  await countContexts(page);
  await reset(page);
  const first = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  const panes: PaneId[] = [first];
  // Sixteen in one tab, split as fast as the daemon makes them.
  for (let i = 1; i < 16; i++) {
    // The biggest pane, for a grid of panes the same size.
    const big = await page.evaluate(() => {
      const t = window.__arugula.client.tabView()!;
      return [...t.layout.panes].sort(([, a], [, b]) => b.cols * b.rows - a.cols * a.rows)[0][0];
    });
    const r = await page.request.post("/api/run", { data: { split: big } });
    expect(r.ok(), await r.text()).toBe(true);
    panes.push((await r.json()).pane as PaneId);
  }
  const marks = await Promise.all(panes.map((p) => mark(page, p)));
  await answers(page);
  await expect.poll(() => page.evaluate(() => document.querySelectorAll(".pane").length)).toBe(16);
  for (const [i, p] of panes.entries()) await draws(page, p, marks[i]);
  const webgl = await page.evaluate((ps) => ps.filter((p) => window.__arugula.webgl(p)), panes);
  expect(webgl.length).toBe(MAX_WEBGL);
  expect(await live(page)).toBeLessThanOrEqual(MAX_WEBGL);
  // A pane on the DOM renderer gets WebGL back when it's used.
  const dom = panes.find((p) => !webgl.includes(p))!;
  await page.locator(`.pane[data-pane="${dom}"] .xterm-screen`).click();
  await expect.poll(() => page.evaluate((p) => window.__arugula.webgl(p), dom)).toBe(true);
  await draws(page, dom, `${dom + 1000}`);
  expect(await live(page)).toBeLessThanOrEqual(MAX_WEBGL);
});
