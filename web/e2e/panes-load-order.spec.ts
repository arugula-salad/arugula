// A page comes up with the tab on screen first: its history is asked for at
// once, and the other terminals load behind it one at a time, or as soon as
// they're shown. Every terminal's history parses on the page's thread, so
// asked for together they held up the page for seconds.

import { expect, test, type Page, type WebSocket } from "@playwright/test";
import type { AttachPane, PaneId } from "../src/shared/proto";
import { active, closeContexts, ready, reset, run, text } from "./helpers";

test.afterAll(closeContexts);

/** A new tab's terminal, and the tab. */
async function newTab(page: Page): Promise<{ tab: number; pane: PaneId }> {
  const before = await page.evaluate(() => window.__arugula.client.state!.tabs.map((t) => t.id));
  await page.evaluate((session) => window.__arugula.client.intent({ op: "new_tab", session, from_pane: null }), await page.evaluate(() => window.__arugula.client.session!));
  await expect.poll(() => page.evaluate((b) => window.__arugula.client.state!.tabs.some((t) => !b.includes(t.id)), before)).toBe(true);
  const tab = await page.evaluate((b) => window.__arugula.client.state!.tabs.find((t) => !b.includes(t.id))!.id, before);
  await page.evaluate((t) => window.__arugula.client.selectTab(t), tab);
  const pane = await active(page);
  await ready(page, pane);
  return { tab, pane };
}

/** Each attach this page sends, by the panes it names. */
function attaches(page: Page): PaneId[][] {
  const sent: PaneId[][] = [];
  page.on("websocket", (ws: WebSocket) =>
    ws.on("framesent", ({ payload }) => {
      if (typeof payload !== "string" || !payload.includes('"attach"')) return;
      const msg = JSON.parse(payload) as { type: string; panes?: AttachPane[] };
      if (msg.type === "attach") sent.push(msg.panes!.map((p) => p.pane));
    }),
  );
  return sent;
}

const reload = async (page: Page) => {
  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
};
const offset = (page: Page, pane: PaneId) => page.evaluate((p) => window.__arugula.offset(p), pane);

test("the tab on screen comes first and the others load behind it, whole", async ({ page }) => {
  await reset(page);
  const shown = await active(page);
  const quiet = await newTab(page);
  // Longer than a piece, in characters of several bytes, so pieces split
  // them: every line must come back as it went.
  const long = await newTab(page);
  await run(page, long.pane, `for i in $(seq 1 3000); do echo "ligne-$i é漢字🥗 xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; done; echo "long-$((6*7))-done"`, "long-42-done");
  await page.evaluate((p) => window.__arugula.client.setActive(p), shown);

  const sent = attaches(page);
  await reload(page);
  await expect.poll(() => sent.length).toBeGreaterThanOrEqual(3);
  expect(sent[0]).toEqual([shown]);
  expect(sent.slice(1).flat().sort()).toEqual([quiet.pane, long.pane].sort());
  for (const later of sent.slice(1)) expect(later).toHaveLength(1);

  await expect.poll(() => text(page, long.pane), { timeout: 15_000 }).toContain("long-42-done");
  const lines = (await text(page, long.pane)).split("\n").filter((l) => l.startsWith("ligne-"));
  expect(lines).toHaveLength(3000);
  lines.forEach((l, i) => expect(l.trimEnd()).toBe(`ligne-${i + 1} é漢字🥗 xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx`));
});

test("a tab shown before its turn loads at once", async ({ page }) => {
  await reset(page);
  const shown = await active(page);
  const opened = await newTab(page);
  const waiting = await newTab(page);
  await page.evaluate((p) => window.__arugula.client.setActive(p), shown);

  // The page is never idle: nothing behind the shown tab gets its turn.
  await page.addInitScript(() => {
    window.requestIdleCallback = () => 0;
  });
  await reload(page);
  await ready(page, shown);
  expect(await offset(page, opened.pane)).toBeNull();

  await page.evaluate((t) => window.__arugula.client.selectTab(t), opened.tab);
  await ready(page, opened.pane);
  expect(await offset(page, waiting.pane)).toBeNull();
});
