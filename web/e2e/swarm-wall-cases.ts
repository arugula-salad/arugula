// #446 (shared by swarm-wall.spec.ts and its WebKit twin, since the
// desktop app is WebKit): the swarm's wall mode, against one fake machine.
// Its button and `w` hide the chrome (the workspace's rail and the shared
// bar under the swarm, the swarm's own controls) and ask for fullscreen;
// Escape, its Exit button and leaving fullscreen bring the chrome back; a
// reload comes back out of it; with nobody there the cursor and the Exit
// button hide (a short wait here, `window.__arugulaWallIdleMs`), never
// while the pointer is on the button. In every theme.

import { expect, test, type Page } from "@playwright/test";
import { FakeFleet } from "./fake-fleet";
import { closeContexts } from "./helpers";

export function swarmWallTests() {
  let fake: FakeFleet;

  // No service worker: with one, Playwright's WebKit on Linux never
  // finishes a second load of the page (a reload hangs its main thread).
  test.use({ baseURL: async ({}, use) => use(fake?.machines[0]?.url), viewport: { width: 1440, height: 900 }, serviceWorkers: "block" });
  test.describe.configure({ mode: "serial" });
  test.afterAll(closeContexts);

  test.beforeAll(async () => {
    fake = new FakeFleet();
    await fake.machine("workstation");
  });

  test.afterAll(() => fake?.stop());

  /** The swarm beside the workspace's rail, under the shared bar. */
  async function swarm(page: Page, idleMs = 60_000) {
    await page.addInitScript((ms) => ((window as { __arugulaWallIdleMs?: number }).__arugulaWallIdleMs = ms), idleMs);
    await page.goto("/#swarm");
    await expect(page.locator(".swarm-shell > .bar")).toBeVisible({ timeout: 20_000 });
    await expect(page.locator("[data-wall-enter]")).toBeVisible();
  }

  const box = (page: Page) => page.locator(".swarm").boundingBox();

  /** The chrome is hidden: the swarm fills the window and keeps only its
   * counts and its Exit button. */
  async function inWall(page: Page) {
    await expect(page.locator("html")).toHaveAttribute("data-wall", "");
    await expect(page.locator(".swarm")).toHaveAttribute("data-wall", "");
    await expect(page.locator(".swarm-shell > .bar")).toBeHidden();
    await expect.poll(() => box(page)).toEqual({ x: 0, y: 0, width: 1440, height: 900 });
    for (const sel of ["[data-wall-enter]", "[data-fit]", "[data-theme-pick='blocks']", "[data-g='project']"]) await expect(page.locator(sel)).toBeHidden();
    await expect(page.locator("[data-stat='panes']")).toBeVisible();
    await expect(page.locator(".swarm-rail")).toBeVisible();
    await expect(page.locator("[data-wall-exit]")).toBeVisible();
  }

  /** Everything is as it was: the bar, the rail beside, the controls. */
  async function outOfWall(page: Page) {
    await expect(page.locator("html")).not.toHaveAttribute("data-wall", "");
    await expect(page.locator(".swarm")).not.toHaveAttribute("data-wall", "");
    await expect(page.locator(".swarm-shell > .bar")).toBeVisible();
    await expect(page.locator("[data-wall-exit]")).toHaveCount(0);
    await expect(page.locator("[data-wall-enter]")).toBeVisible();
    const b = (await box(page))!;
    expect(b.x).toBeGreaterThan(0);
    expect(b.y).toBeGreaterThan(0);
    // Still the swarm: Escape left wall mode, not the swarm.
    expect(new URL(page.url()).hash).toBe("#swarm");
  }

  const fullscreen = (page: Page) => page.evaluate(() => !!(document.fullscreenElement ?? (document as { webkitFullscreenElement?: Element }).webkitFullscreenElement));

  test("the button enters wall mode, and Escape leaves it", async ({ page }) => {
    await swarm(page);
    await page.locator("[data-wall-enter]").click();
    await inWall(page);
    // Fullscreen is the browser's to give: logged, not required.
    console.log(`fullscreen granted: ${await fullscreen(page)}`);
    await page.keyboard.press("Escape");
    await outOfWall(page);
    expect(await fullscreen(page)).toBe(false);
  });

  test("the Exit button leaves it", async ({ page }) => {
    await swarm(page);
    await page.locator("[data-wall-enter]").click();
    await inWall(page);
    await page.locator("[data-wall-exit]").click();
    await outOfWall(page);
  });

  test("leaving fullscreen leaves it, when fullscreen was given", async ({ page }) => {
    await swarm(page);
    await page.locator("[data-wall-enter]").click();
    await inWall(page);
    test.skip(!(await expect.poll(() => fullscreen(page), { timeout: 3000 }).toBe(true).then(() => true, () => false)), "this browser didn't give fullscreen");
    await page.evaluate(() => document.exitFullscreen());
    await outOfWall(page);
  });

  test("w enters and leaves, but not while typing", async ({ page }) => {
    await swarm(page);
    await page.locator(".swarm-field").click({ position: { x: 300, y: 600 } });
    await page.keyboard.press("w");
    await inWall(page);
    await page.keyboard.press("w");
    await outOfWall(page);
    // From the rail's button that opened the swarm too.
    await page.locator(".ws-rail button").first().focus();
    await page.keyboard.press("w");
    await inWall(page);
    await page.keyboard.press("Escape");
    await outOfWall(page);
    // Typed into a field, it's a letter.
    await page.evaluate(() => {
      const i = document.createElement("input");
      i.id = "typed";
      document.querySelector(".swarm-bar")!.append(i);
    });
    await page.locator("#typed").fill("");
    await page.locator("#typed").press("w");
    await expect(page.locator("#typed")).toHaveValue("w");
    await expect(page.locator(".swarm")).not.toHaveAttribute("data-wall", "");
    // With a modifier it isn't ours either.
    await page.locator(".swarm-field").click({ position: { x: 300, y: 600 } });
    await page.keyboard.press("Shift+W");
    await expect(page.locator(".swarm")).not.toHaveAttribute("data-wall", "");
  });

  test("a reload comes back out of it", async ({ page }) => {
    await swarm(page);
    await page.locator("[data-wall-enter]").click();
    await inWall(page);
    await page.reload();
    await expect(page.locator("[data-wall-enter]")).toBeVisible({ timeout: 20_000 });
    await outOfWall(page);
  });

  test("with nobody there the cursor and the Exit button hide, until someone moves", async ({ page }) => {
    await swarm(page, 1500);
    await page.mouse.move(400, 500);
    await page.locator("[data-wall-enter]").click();
    await inWall(page);
    await page.mouse.move(420, 520);
    await expect(page.locator(".swarm")).toHaveAttribute("data-wall-idle", "", { timeout: 3000 });
    await expect(page.locator(".swarm-wall-ctl")).toHaveCSS("opacity", "0");
    await expect(page.locator(".swarm-field")).toHaveCSS("cursor", "none");
    // Any movement brings them back.
    await page.mouse.move(430, 530);
    await expect(page.locator(".swarm")).not.toHaveAttribute("data-wall-idle", "");
    await expect(page.locator(".swarm-wall-ctl")).toHaveCSS("opacity", "1");
    // A key does too.
    await expect(page.locator(".swarm")).toHaveAttribute("data-wall-idle", "", { timeout: 3000 });
    await page.keyboard.press("Shift");
    await expect(page.locator(".swarm")).not.toHaveAttribute("data-wall-idle", "");
    // Never while the pointer is on the button.
    await page.locator("[data-wall-exit]").hover();
    await page.waitForTimeout(2500);
    await expect(page.locator(".swarm")).not.toHaveAttribute("data-wall-idle", "");
    await page.locator("[data-wall-exit]").click();
    await outOfWall(page);
    await expect(page.locator(".swarm")).not.toHaveAttribute("data-wall-idle", "");
  });

  test("reduced motion has no transitions", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await swarm(page);
    await page.locator("[data-wall-enter]").click();
    // The page's reduced-motion rule: a hundredth of a millisecond.
    const fade = () => page.locator(".swarm-wall-ctl").evaluate((e) => parseFloat(getComputedStyle(e).transitionDuration));
    await expect.poll(fade).toBeLessThan(0.001);
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await expect.poll(fade).toBeGreaterThan(0.1);
  });

  test("every theme has it", async ({ page }) => {
    await swarm(page);
    for (const theme of ["blocks", "city", "hive", "timeline"]) {
      await page.locator(`[data-theme-pick='${theme}']`).click();
      await expect(page.locator(".swarm")).toHaveAttribute("data-theme", theme);
      await page.locator("[data-wall-enter]").click();
      await inWall(page);
      await expect(page.locator(".swarm-field")).toBeVisible();
      await page.keyboard.press("Escape");
      await outOfWall(page);
    }
  });
}
