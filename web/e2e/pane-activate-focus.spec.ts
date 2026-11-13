// #472: activating a terminal pane with a click leaves the keys with it. The
// click on its header (or body) makes it the active pane; what is typed next
// reaches that terminal, not the one before it or nothing.

import { expect, test } from "@playwright/test";
import { active, paneEl, panes, ready, reset, text } from "./helpers";

test("typing after a click on an inactive terminal's header or body reaches that terminal", async ({ page }) => {
  await reset(page);
  const first = await active(page);
  await page.evaluate((pane) => window.__arugula.client.intent({ op: "split", pane, edge: "right" }), first);
  await expect.poll(() => panes(page)).toHaveLength(2);
  const [left, right] = await panes(page);
  for (const p of [left, right]) await ready(page, p);

  for (const [from, to, where, word] of [
    [left, right, "header", "viaheader"],
    [right, left, "body", "viabody"],
  ] as const) {
    await paneEl(page, from).click({ position: { x: 40, y: 60 } });
    await expect.poll(() => active(page)).toBe(from);
    const target = where === "header" ? page.locator(`[data-pane-header="${to}"] .pane-header-title`) : paneEl(page, to);
    await target.click(where === "body" ? { position: { x: 40, y: 60 } } : {});
    await expect.poll(() => active(page)).toBe(to);
    await page.keyboard.type(`echo ${word}-$((6*7))`);
    await page.keyboard.press("Enter");
    await expect.poll(() => text(page, to), { timeout: 10_000 }).toContain(`${word}-42`);
    expect(await text(page, from)).not.toContain(`${word}-42`);
  }
});
