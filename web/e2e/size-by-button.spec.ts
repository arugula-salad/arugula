// #473: while another screen that holds a tab's size is connected, a screen
// takes it only through "Use this window's size": not input (keys, mouse
// and focus reports, a phone's tap), not showing the tab again. With no
// holder connected, the first screen to show the tab takes it.
// A is a desktop window, B a Pixel 7 on the same tab.

import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, paneEl, reset, ready, open, tab, type } from "./helpers";
import { pixel7 } from "./phones";

test.afterAll(closeContexts);

const me = (p: Page) => p.evaluate(() => window.__arugula.client.clientId);
const ownerOf = (p: Page, id: number) => p.evaluate((id) => window.__arugula.client.tabView(id)?.owner ?? null, id);
const size = (p: Page, id: number) =>
  p.evaluate((id) => {
    const t = window.__arugula.client.tabView(id)!;
    return `${t.cols}x${t.rows}`;
  }, id);

test("only the button takes the size from a connected screen", async ({ browser }) => {
  const a = await (await browser.newContext({ viewport: { width: 1100, height: 650 } })).newPage();
  await reset(a);
  const pane = await active(a);
  const tabId = (await tab(a)).id;
  const heldBy = await me(a);
  await expect.poll(async () => (await tab(a)).owner).toBe(heldBy);
  const desktop = await size(a, tabId);

  const b = await (await browser.newContext(pixel7)).newPage();
  await open(b);
  await ready(b, pane);
  // B shows the tab and doesn't take it; it says another window has it.
  await expect(b.locator("[data-sized-elsewhere]").getByRole("button", { name: "Use this window's size" })).toBeVisible();
  expect((await tab(b)).owner).toBe(heldBy);

  // 1. Input from B: keys, a mouse report, a focus report and a tap.
  await type(b, pane, "echo from-phone\n");
  await b.evaluate((p) => {
    const c = window.__arugula.client;
    for (const s of ["\x1b[<0;5;5M", "\x1b[<0;5;5m", "\x1b[I", "\x1b[O"]) c.input(p, new TextEncoder().encode(s));
  }, pane);
  await paneEl(b, pane).tap({ position: { x: 30, y: 30 } });
  await b.waitForTimeout(700);
  expect(await ownerOf(a, tabId), "B's input took the size").toBe(heldBy);
  expect(await size(a, tabId)).toBe(desktop);

  // 2. B switches away and back.
  const other = await b.evaluate(async (id) => {
    const c = window.__arugula.client;
    const session = c.sessionOfTab(id)!;
    c.intent({ op: "new_tab", session, from_pane: null });
    return session;
  }, tabId);
  await expect.poll(() => b.evaluate((s) => window.__arugula.client.state!.sessions.find((x) => x.id === s)!.tabs.length, other)).toBe(2);
  await b.evaluate((id) => {
    const c = window.__arugula.client;
    c.selectTab(c.state!.sessions.find((s) => s.tabs.includes(id))!.tabs.find((t) => t !== id)!);
  }, tabId);
  await b.waitForTimeout(500);
  await b.evaluate((id) => window.__arugula.client.selectTab(id), tabId);
  await b.waitForTimeout(700);
  expect(await ownerOf(a, tabId), "B switching back took the size").toBe(heldBy);
  expect(await size(a, tabId)).toBe(desktop);

  // 3. The button is B's.
  await b.locator("[data-sized-elsewhere]").getByRole("button", { name: "Use this window's size" }).click();
  await expect.poll(() => ownerOf(a, tabId)).toBe(await me(b));
  await expect.poll(async () => (await size(a, tabId)) === (await size(b, tabId))).toBe(true);
  expect(await size(a, tabId)).not.toBe(desktop);

  // 4. A asks for it back and then goes: nobody holds it, and B showing the
  // tab takes it, no button.
  await a.evaluate((id) => window.__arugula.client.selectTab(id), tabId);
  await a.locator("[data-sized-elsewhere]").getByRole("button", { name: "Use this window's size" }).click();
  await expect.poll(() => ownerOf(b, tabId)).toBe(heldBy);
  await b.evaluate((id) => {
    const c = window.__arugula.client;
    const o = c.state!.sessions.find((s) => s.tabs.includes(id))!.tabs.find((t) => t !== id)!;
    c.selectTab(o);
  }, tabId);
  await a.close();
  await expect.poll(() => ownerOf(b, tabId)).toBeNull();
  await b.evaluate((id) => window.__arugula.client.selectTab(id), tabId);
  await expect.poll(() => ownerOf(b, tabId)).toBe(await me(b));
});
