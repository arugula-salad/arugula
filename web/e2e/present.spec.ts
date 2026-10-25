// #1472: while a window presents a tab, its size is the tab's. Another
// editor switching to the tab, typing, or asking for the size doesn't
// take it; "Take over" does, and "Stop" gives the tab back to claims.

import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, open, ready, reset, run, tab, text, type } from "./helpers";

test.afterAll(closeContexts);

const me = (p: Page) => p.evaluate(() => window.__arugula.client.clientId);

test("a presenter keeps the tab's size until someone takes over", async ({ browser }) => {
  const a = await (await browser.newContext({ viewport: { width: 1100, height: 650 } })).newPage();
  await reset(a);
  const pane = await active(a);
  const first = await tab(a);
  // A second tab for B to switch to and back from.
  const session = await a.evaluate(() => window.__arugula.client.session!);
  await a.evaluate((s) => window.__arugula.client.intent({ op: "new_tab", session: s, from_pane: null }), session);
  await expect.poll(() => a.evaluate(() => window.__arugula.client.tab)).not.toBe(first.id);
  const other = await a.evaluate(() => window.__arugula.client.tab!);
  await a.evaluate((id) => window.__arugula.client.selectTab(id), first.id);
  const b = await (await browser.newContext({ viewport: { width: 760, height: 480 } })).newPage();
  await open(b);
  await b.evaluate((id) => window.__arugula.client.selectTab(id), first.id);
  await ready(b, pane);
  // A presents from the tab's menu.
  await expect.poll(async () => (await tab(a)).owner).toBe(await me(b));
  await a.locator(`[data-tab-id="${first.id}"]`).click({ button: "right" });
  await a.getByRole("menuitemradio", { name: "Present" }).click();
  await expect.poll(async () => (await tab(a)).presenter).toBe(await me(a));
  await expect.poll(async () => (await tab(a)).owner).toBe(await me(a));
  const sized = await tab(a);
  await expect(a.locator("[data-presenter-bar]")).toContainText("Presenting");
  await expect(a.locator("[data-presenter-bar]")).toHaveAttribute("data-presenting", "me");
  // B sees who presents, in place of "Use this window's size".
  await expect(b.locator("[data-presenter-bar]")).toContainText("is presenting");
  await expect(b.locator("[data-sized-elsewhere]")).toHaveCount(0);

  // Every size the tab has from now on (each one resizes the PTY).
  await a.evaluate((id) => {
    const c = window.__arugula.client;
    const w = window as unknown as { sizes: string[] };
    w.sizes = [];
    let last = "";
    c.subscribe(() => {
      const t = c.state?.tabs.find((t) => t.id === id);
      const now = t ? `${t.cols}x${t.rows}` : "";
      if (now && now !== last) w.sizes.push(now);
      last = now;
    });
  }, sized.id);
  const sizes = () => a.evaluate(() => (window as unknown as { sizes: string[] }).sizes);

  // B switches away and back, and types. It clicks nothing in the bar.
  await b.evaluate((id) => window.__arugula.client.selectTab(id), other);
  await expect.poll(() => b.evaluate(() => window.__arugula.client.tab)).toBe(other);
  await b.evaluate((id) => window.__arugula.client.selectTab(id), first.id);
  await expect.poll(() => b.evaluate(() => window.__arugula.client.tab)).toBe(first.id);
  await b.waitForTimeout(500);
  await type(b, pane, "echo b-types-$((3+4))\n");
  await expect.poll(() => text(a, pane)).toContain("b-types-7");
  await b.waitForTimeout(3500);
  await type(b, pane, "echo b-again-$((4+4))\n");
  await expect.poll(() => text(a, pane)).toContain("b-again-8");
  expect(await sizes(), "the tab was resized under the presenter").toEqual([`${sized.cols}x${sized.rows}`]);
  const held = await tab(a);
  expect([held.presenter, held.owner]).toEqual([await me(a), await me(a)]);

  // B takes over: the size is B's.
  await b.locator("[data-presenter-bar]").getByRole("button", { name: "Take over" }).click();
  await expect.poll(async () => (await tab(a)).presenter).toBe(await me(b));
  await expect.poll(async () => (await tab(a)).owner).toBe(await me(b));
  await expect(b.locator("[data-presenter-bar]")).toHaveAttribute("data-presenting", "me");
  await expect(a.locator("[data-presenter-bar]")).toHaveAttribute("data-presenting", "other");

  // B stops: both bars go, and nobody's pinned.
  await b.locator("[data-presenter-bar]").getByRole("button", { name: "Stop" }).click();
  await expect.poll(async () => (await tab(a)).presenter).toBeUndefined();
  await expect(a.locator("[data-presenter-bar]")).toHaveCount(0);
  await expect(b.locator("[data-presenter-bar]")).toHaveCount(0);
  await run(a, pane, "echo done-$((5+5))", "done-10");
});
