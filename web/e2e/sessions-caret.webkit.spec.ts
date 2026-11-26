// #378, #1443: a caret at the left end of the tab strip, at the sidebar's
// edge, shows and hides the sessions sidebar; the rail has no Sessions button
// and the header has none (it shows the place stack). The caret stays when
// the sidebar is closed (it's the way back), and it's the panes' only: the
// swarm has none, and Panes brings the sidebar back as it was.

import { expect, test } from "@playwright/test";
import { reset } from "./helpers";

test.use({ viewport: { width: 1440, height: 900 }, serviceWorkers: "block" });

test("the caret shows and hides the sessions sidebar, and stays when it's closed", async ({ page }) => {
  await reset(page);
  const rail = page.locator(".ws-rail");
  const caret = page.locator(".ws-tabs [data-open-sessions]");
  const sidebar = page.locator("[data-session-sidebar]");
  await expect(rail).toBeVisible();

  // The rail has no Sessions.
  await expect(rail.locator("[data-open-sessions]")).toHaveCount(0);
  await expect(rail.getByRole("button", { name: /sessions sidebar/i })).toHaveCount(0);

  // In the tab strip, before the first tab, centred on the tabs; not in the header.
  await expect(sidebar).toBeVisible();
  await expect(caret).toBeVisible();
  await expect(page.locator(".ws-head [data-open-sessions]")).toHaveCount(0);
  await expect(caret).toHaveAttribute("aria-controls", "ws-session-sidebar");
  await expect(caret).toHaveAttribute("aria-expanded", "true");
  await expect(caret).toHaveAttribute("aria-label", "Hide sessions sidebar");
  await expect(caret.locator("svg")).toHaveCount(1);
  const tab = page.locator(".ws-tabs .tab").first();
  const [c, t, sb] = await Promise.all([caret.boundingBox(), tab.boundingBox(), sidebar.boundingBox()]);
  expect(c!.x + c!.width).toBeLessThanOrEqual(t!.x);
  expect(Math.abs(c!.y + c!.height / 2 - (t!.y + t!.height / 2))).toBeLessThan(8);
  expect(c!.x).toBeGreaterThanOrEqual(sb!.x + sb!.width);

  // The header's left is the place stack: machine, then the session.
  const stack = page.locator(".ws-head .ws-stack");
  await expect(stack.locator(".host-button, .ws-host-name").first()).toBeVisible();
  await expect(stack.locator(".app-path-sep")).not.toHaveCount(0);
  await expect(stack.locator(".session-button")).toBeVisible();

  // Hidden: the caret stays, pointing the other way.
  await caret.click();
  await expect(sidebar).toHaveCount(0);
  await expect(caret).toBeVisible();
  await expect(caret).toHaveAttribute("aria-expanded", "false");
  await expect(caret).toHaveAttribute("aria-label", "Show sessions sidebar");
  await expect(caret.locator("svg")).toHaveCount(1);
  const [c2, t2] = await Promise.all([caret.boundingBox(), tab.boundingBox()]);
  expect(c2!.x + c2!.width).toBeLessThanOrEqual(t2!.x);

  // And back, from the keyboard.
  await caret.focus();
  await page.keyboard.press("Enter");
  await expect(sidebar).toBeVisible();
  await expect(caret).toHaveAttribute("aria-expanded", "true");
});

test("the caret is the panes' only, and Machines brings the sidebar back", async ({ page }) => {
  await reset(page);
  const caret = page.locator(".ws-tabs [data-open-sessions]");
  const sidebar = page.locator("[data-session-sidebar]");
  await expect(sidebar).toBeVisible();

  await page.locator("[data-machine-places] [data-machine-swarm]").click();
  await expect.poll(() => page.evaluate(() => location.hash)).toBe("#swarm");
  await expect(page.locator("[data-open-sessions]")).toHaveCount(0);
  await page.locator(".ws-rail [data-open-machines]").click();
  await expect(caret).toBeVisible();
  await expect(sidebar).toBeVisible();

  // Closed before the swarm, closed after it, with the caret to open it.
  await caret.click();
  await expect(sidebar).toHaveCount(0);
  await page.locator("[data-machine-places] [data-machine-swarm]").click();
  await expect(page.locator("[data-open-sessions]")).toHaveCount(0);
  await page.locator(".ws-rail [data-open-machines]").click();
  await expect(sidebar).toHaveCount(0);
  await expect(caret).toHaveAttribute("aria-expanded", "false");
  await expect(caret.locator("svg")).toHaveCount(1);
  await caret.click();
  await expect(sidebar).toBeVisible();
});
