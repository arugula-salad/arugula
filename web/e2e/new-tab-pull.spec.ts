// #1521: a new tab or pane opened by another client within 3 s shouldn't
// pull this window's focus onto it. Only this client's own new tabs and panes
// should jump the view to them.

import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, open, ready, reset, tab } from "./helpers";

test.afterAll(closeContexts);

const myTab = (p: Page) => p.evaluate(() => window.__arugula.client.tab);

test("another client's new tab doesn't pull the focus", async ({ browser }) => {
  const a = await (await browser.newContext()).newPage();
  await reset(a);
  const pane = await active(a);
  const tab1 = await tab(a);

  const b = await (await browser.newContext()).newPage();
  await open(b);
  await ready(b, pane);

  // Record which tab A is viewing now.
  const aViewingBefore = await myTab(a);
  expect(aViewingBefore).toBe(tab1.id);
  // reset() made A's session: let that 3 s pass, so only the rename is recent.
  await a.waitForTimeout(3100);

  // A renames the current tab (doesn't create anything).
  await a.evaluate((tabId) => {
    const c = window.__arugula.client;
    c.intent({ op: "rename_tab", tab: tabId, name: "renamed" });
  }, tab1.id);

  // Within 3 seconds, B creates a new tab.
  const tabs = () => a.evaluate(() => window.__arugula.client.state!.tabs.length);
  const before = await tabs();
  await a.waitForTimeout(500);
  await b.evaluate(() => {
    const c = window.__arugula.client;
    c.intent({ op: "new_tab", session: c.session!, from_pane: null });
  });

  // A has seen B's tab arrive, still inside the 3 s.
  await expect.poll(tabs, { timeout: 2000 }).toBe(before + 1);

  // A should still be viewing the original tab, not B's new one.
  const aViewingAfter = await myTab(a);
  expect(aViewingAfter).toBe(aViewingBefore);
});

test("this client's own new tab still jumps the focus", async ({ browser }) => {
  const a = await (await browser.newContext()).newPage();
  await reset(a);
  const pane = await active(a);
  const tab1 = await tab(a);

  // Record the starting tab.
  const aViewingBefore = await myTab(a);
  expect(aViewingBefore).toBe(tab1.id);

  // A creates a new tab (should jump).
  await a.evaluate(() => {
    const c = window.__arugula.client;
    c.intent({ op: "new_tab", session: c.session, from_pane: null });
  });

  // Wait for the new tab to be reflected in A's client.tab.
  await expect.poll(() => myTab(a)).not.toBe(aViewingBefore);

  // A should now be viewing a different tab (the new one).
  const aViewingAfter = await myTab(a);
  expect(aViewingAfter).not.toBe(aViewingBefore);
});
