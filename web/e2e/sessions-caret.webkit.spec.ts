// #378: a caret in the panes page's header, left of the machine menu, shows
// and hides the sessions sidebar; the rail has no Sessions button. The caret
// stays when the sidebar is closed (it's the way back), and it's the panes'
// only: the swarm has none, and Panes brings the sidebar back as it was.

import { expect, test } from "@playwright/test";
import { reset } from "./helpers";

test.use({ viewport: { width: 1440, height: 900 }, serviceWorkers: "block" });

test("the caret shows and hides the sessions sidebar, and stays when it's closed", async ({ page }) => {
  await reset(page);
  const rail = page.locator(".ws-rail");
  const caret = page.locator(".ws-head [data-open-sessions]");
  const sidebar = page.locator("[data-session-sidebar]");
  await expect(rail).toBeVisible();

  // The rail has no Sessions.
  await expect(rail.locator("[data-open-sessions]")).toHaveCount(0);
  await expect(rail.getByRole("button", { name: /sessions sidebar/i })).toHaveCount(0);

  // Just left of the machine menu, open.
  await expect(sidebar).toBeVisible();
  await expect(caret).toBeVisible();
  await expect(caret).toHaveAttribute("aria-controls", "ws-session-sidebar");
  await expect(caret).toHaveAttribute("aria-expanded", "true");
  await expect(caret).toHaveAttribute("aria-label", "Hide sessions sidebar");
  await expect(caret).toHaveText("‹");
  const menu = page.locator(".ws-head .host-button, .ws-head .ws-host-name").first();
  const [c, m] = await Promise.all([caret.boundingBox(), menu.boundingBox()]);
  expect(c!.x + c!.width).toBeLessThanOrEqual(m!.x);
  expect(Math.abs(c!.y + c!.height / 2 - (m!.y + m!.height / 2))).toBeLessThan(4);

  // Hidden: the caret stays, as ›.
  await caret.click();
  await expect(sidebar).toHaveCount(0);
  await expect(caret).toBeVisible();
  await expect(caret).toHaveAttribute("aria-expanded", "false");
  await expect(caret).toHaveAttribute("aria-label", "Show sessions sidebar");
  await expect(caret).toHaveText("›");
  const [c2, m2] = await Promise.all([caret.boundingBox(), menu.boundingBox()]);
  expect(c2!.x + c2!.width).toBeLessThanOrEqual(m2!.x);

  // And back.
  await caret.click();
  await expect(sidebar).toBeVisible();
  await expect(caret).toHaveText("‹");
});

test("the caret is the panes' only, and Panes brings the sidebar back", async ({ page }) => {
  await reset(page);
  const caret = page.locator(".ws-head [data-open-sessions]");
  const sidebar = page.locator("[data-session-sidebar]");
  await expect(sidebar).toBeVisible();

  await page.locator(".ws-rail [data-open-swarm]").click();
  await expect.poll(() => page.evaluate(() => location.hash)).toBe("#swarm");
  await expect(page.locator("[data-open-sessions]")).toHaveCount(0);
  await page.locator(".ws-rail [data-open-panes]").click();
  await expect(caret).toBeVisible();
  await expect(sidebar).toBeVisible();

  // Closed before the swarm, closed after it, with the caret to open it.
  await caret.click();
  await expect(sidebar).toHaveCount(0);
  await page.locator(".ws-rail [data-open-swarm]").click();
  await expect(page.locator("[data-open-sessions]")).toHaveCount(0);
  await page.locator(".ws-rail [data-open-panes]").click();
  await expect(sidebar).toHaveCount(0);
  await expect(caret).toHaveText("›");
  await caret.click();
  await expect(sidebar).toBeVisible();
});
