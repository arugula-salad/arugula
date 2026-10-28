// M26: the swarm's frame rate at 500 panes, as S16 measured the prototype:
// at least about 60 fps on the laptop, and 30 on a phone-sized viewport with
// the CPU slowed 4x (a stand-in until a real phone). The panes are the
// synthetic fleet (src/swarm/fake.ts), drawn with physics awake throughout.
//
// #278: what's held is what the scene does in a frame, not how many frames
// the browser asks for. The rate is the machine's: on a loaded geek it fell
// to a third with nothing changed (14 of 150 failed on main), and on CI's
// shared pods likewise, so it's logged. A Canvas 2D scene's work, its step
// and draw on the main thread (the median frame), must fit in the frame
// the rate allows: 60 fps holds while it's under 16.7 ms. Load moves that
// too, about 2x, which still leaves it 2-5x under. The city's is its draw
// calls and triangles.

import { devices, expect, test, type Page } from "@playwright/test";
import { open, closeContexts } from "./helpers";
import type { Measured } from "../src/shell/swarm/field";

test.afterAll(closeContexts);

/** What each theme's scene has that the others don't, to know it's drawn. */
const MARK = { city: "lotOf", hive: "cellOf", timeline: "runsOf" } as const;

async function measure(page: Page, theme: "blocks" | "city" | "hive" | "timeline" = "blocks"): Promise<Measured> {
  await page.addInitScript((t) => localStorage.setItem("arugula.swarm.theme", t), theme);
  await page.goto("/#swarm");
  await open(page).catch(() => {});
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  await page.evaluate(() => void window.__arugula.swarmFake(500));
  await page.waitForTimeout(3000);
  if (theme !== "blocks") {
    await expect.poll(() => page.evaluate((m) => !!(window.__arugula.swarm as Record<string, unknown> | null)?.[m], MARK[theme]), { timeout: 10_000 }).toBe(true);
    await page.waitForTimeout(2000);
  }
  return page.evaluate(() => (window.__arugula.swarm as { measure(ms: number): Promise<Measured> }).measure(5000));
}

function log(what: string, m: Measured) {
  const drew = m.calls ? `, at most ${m.calls} draw calls and ${m.triangles} triangles a frame` : "";
  console.log(`${what}: ${m.fps.toFixed(1)} fps, work p50 ${m.workP50.toFixed(2)} ms, p95 ${m.workP95.toFixed(2)} ms over ${m.frames} frames${drew}`);
}

/** A frame's work fits in the frame `fps` allows. Enough frames for a
 * median (5 s at the slowest a loaded geek drew, 34 fps, is 170): a loop
 * that stopped drawing has no work to hold. */
function fits(what: string, m: Measured, fps: number) {
  log(what, m);
  expect(m.frames, "frames in 5 s").toBeGreaterThanOrEqual(20);
  expect(m.workP50, `a frame's work, at ${fps} fps`).toBeLessThanOrEqual(1000 / fps);
}

test("500 panes at 60 fps on the laptop", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1400, height: 860 } });
  fits("laptop, 500 panes", await measure(page), 60);
  await page.close();
});

test("500 panes at 30 fps on a phone with the CPU slowed 4x", async ({ browser }) => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  const ctx = await browser.newContext({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 4 });
  fits("phone (4x), 500 panes", await measure(page), 30);
  await ctx.close();
});

// M41: the city at 500 panes. WebGL in headless Chromium is software
// (SwiftShader) in another process, so what costs is what the city hands
// it: one instanced mesh a kind of building, not a mesh a building. Its
// main-thread work isn't held, as it waits on SwiftShader when that's
// starved (77 ms a frame, 7 frames in 5 s, once on a loaded geek).
const CITY_CALLS = 200;
const CITY_TRIANGLES = 24_000;
test("500 panes in the city", async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1400, height: 860 } });
  const m = await measure(page, "city");
  log("city, laptop, 500 panes", m);
  expect(m.calls, "draw calls a frame").toBeGreaterThan(0);
  expect(m.calls, "draw calls a frame").toBeLessThanOrEqual(CITY_CALLS);
  expect(m.triangles, "triangles a frame").toBeLessThanOrEqual(CITY_TRIANGLES);
  await page.close();
});

// M42: the hive and the timeline at 500 panes, both Canvas 2D like blocks;
// 50 fps, the floor they had.
for (const theme of ["hive", "timeline"] as const) {
  test(`500 panes in the ${theme}`, async ({ browser }) => {
    const page = await browser.newPage({ viewport: { width: 1400, height: 860 } });
    fits(`${theme}, laptop, 500 panes`, await measure(page, theme), 50);
    await page.close();
  });
}
