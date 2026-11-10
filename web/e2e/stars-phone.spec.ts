// Starred tabs and panes on a phone (#854): the Here sheet lists them
// below Needs you and above the quick actions, and its Sessions list
// marks them with a ★. A row shows the tab or pane. The desktop's half is
// in stars.spec.ts.

import { devices, expect, test } from "@playwright/test";
import { active, reset, sheetTab } from "./helpers";

const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

test("the Here sheet lists starred tabs and panes below Needs you, and a row shows one", async ({ page }) => {
  await reset(page);
  // Two panes in the first tab, and a second tab, shown.
  const [a, first] = await page.evaluate(() => {
    const c = window.__arugula.client;
    return [c.tab!, c.active()!];
  });
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), first);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.panes.length)).toBe(2);
  const second = await page.evaluate((p) => window.__arugula.client.state!.panes.map((x) => x.id).find((id) => id !== p)!, first);
  await page.evaluate(() => {
    const c = window.__arugula.client;
    c.intent({ op: "new_tab", session: c.session!, from_pane: null });
  });
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.tabs.length)).toBe(2);
  const b = await page.evaluate((a) => window.__arugula.client.state!.tabs.find((t) => t.id !== a)!.id, a);
  await page.evaluate((t) => window.__arugula.client.selectTab(t), b);

  // Starred on another device: the same option on the daemon.
  const star = (kind: "tab" | "pane", id: number, at: number) =>
    page.evaluate(([kind, id, at]) => window.__arugula.client.intent({ op: "set_option", scope: { kind, id }, name: "star", value: String(at) } as never), [kind, id, at] as const);
  await star("pane", second, 2);
  await star("tab", a, 1);

  await page.locator(".sheet-button").click();
  await sheetTab(page, "Here");
  const group = page.locator("[data-sheet-starred]");
  await expect(group).toContainText("Starred");
  await expect.poll(() => group.locator("[data-starred]").evaluateAll((els) => els.map((e) => e.getAttribute("data-starred")))).toEqual([`tab:${a}`, `pane:${second}`]);
  // Below Needs you, above the quick actions.
  const order = await page.locator(".sheet-panel").evaluate((panel) =>
    [...panel.children].map((e) => (e.matches("[data-sheet-starred]") ? "starred" : e.matches(".needs-you, [data-needs-none]") ? "needs" : e.matches(".sheet-quick") ? "quick" : "")).filter(Boolean),
  );
  expect(order).toEqual(["needs", "starred", "quick"]);

  // The Sessions list (the shown session's, unfolded) marks them.
  await expect(page.locator(".sheet-tab .star-mark[aria-label='Starred tab']")).toHaveCount(1);
  await expect(page.locator(".sheet-tab .star-mark[aria-label='Starred pane']")).toHaveCount(1);

  // A row shows its pane, and closes the sheet.
  await group.locator(`[data-starred="pane:${second}"]`).click();
  await expect(page.locator(".sheet")).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.tab)).toBe(a);
  await expect.poll(() => active(page)).toBe(second);
});
