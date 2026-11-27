// A thin link (control's relay, or one the lag probe finds slow; Arugula
// #443): panes in hidden tabs stop streaming, so a busy one can't queue ahead
// of the tab being shown. Shown again, they come back with what they missed,
// and while detached they still raise attention, which the daemon reads.
import { expect, test, type Page } from "@playwright/test";
import { panes, ready, reset, text } from "./helpers";

// Every probe counts as slow, and one goes out every 200 ms.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    (window as { __arugulaLag?: unknown }).__arugulaLag = { probeMs: 200, lagMs: 30_000, slowMs: -1 };
  });
});

const say = (page: Page, pane: number, s: string) =>
  page.evaluate(([p, s]) => window.__arugula.client.input(p as number, new TextEncoder().encode(s as string)), [pane, s] as const);
const offset = (page: Page, pane: number) => page.evaluate((p) => window.__arugula.offset(p), pane);
const attention = (page: Page, pane: number) => page.evaluate((p) => window.__arugula.client.info(p)?.attention, pane);
const tabEl = (page: Page, tab: number) => page.locator(`[data-tab-id="${tab}"]`);

test("a hidden tab's panes stop streaming on a thin link, come back when shown, and still raise attention", async ({ page }) => {
  await reset(page);
  await expect.poll(() => page.evaluate(() => (window.__arugula.client as unknown as { thin: boolean }).thin)).toBe(true);
  const first = await page.evaluate(() => window.__arugula.client.tab!);

  // A second tab, with two panes.
  await page.evaluate(() => {
    const c = window.__arugula.client;
    c.intent({ op: "new_tab", session: c.session!, from_pane: null });
  });
  await expect.poll(() => page.evaluate(() => window.__arugula.client.tab)).not.toBe(first);
  const second = await page.evaluate(() => window.__arugula.client.tab!);
  const [a] = await panes(page);
  await ready(page, a);
  await page.evaluate((pane) => window.__arugula.client.intent({ op: "split", pane, edge: "right" }), a);
  await expect.poll(() => panes(page)).toHaveLength(2);
  const [, b] = await panes(page);
  await ready(page, b);

  // Hidden: what it prints doesn't come.
  await tabEl(page, first).click();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.tab)).toBe(first);
  // Output already on its way when it was hidden lands first.
  await page.waitForTimeout(500);
  const before = await offset(page, a);
  await say(page, a, "echo while-hidden-$((6*7))\n");
  // The bell, and a program that's blocked, then done.
  await say(page, a, "printf '\\a'\n");
  await expect.poll(() => attention(page, a)).toBe("needs_input");
  const report = (body: string) => `printf '\\033]7501;${body}\\033\\\\'`;
  await say(page, b, `${report("state=blocked:app=agent")}; read x; ${report("state=done:app=agent")}\n`);
  await expect.poll(() => attention(page, b)).toBe("needs_input");
  await say(page, b, "y\n");
  await expect.poll(() => attention(page, b)).toBe("done");
  expect(await offset(page, a)).toBe(before);
  expect(await text(page, a)).not.toContain("while-hidden-42");

  // Shown again: what it missed.
  await tabEl(page, second).click();
  await expect.poll(() => text(page, a)).toContain("while-hidden-42");
  await expect.poll(() => offset(page, a)).toBeGreaterThan(before!);
  // And it streams while shown.
  await say(page, a, "echo shown-$((7*7))\n");
  await expect.poll(() => text(page, a)).toContain("shown-49");
});
