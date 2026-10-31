// #439: a focus belongs to the client holding the tab. A desktop focuses a
// pane, a phone then opens another one, and the desktop's focus never traps
// it. (Cases for focus-per-screen.spec.ts and its WebKit twin.)

import { expect, test, type Page } from "@playwright/test";
import type { PaneId } from "../src/shared/proto";
import { closeContexts, open, panes, reset, tab } from "./helpers";

type Phone = { viewport: { width: number; height: number }; userAgent: string; deviceScaleFactor: number; isMobile: boolean; hasTouch: boolean };

const shownIds = (page: Page) => page.evaluate(() => window.__arugula.client.tabView()!.layout.panes.map(([id]) => id));
const focus = async (page: Page) => (await tab(page)).focus;
/** What a person's tap on a pane does on a phone. */
const tap = (page: Page, pane: PaneId) => page.evaluate((p) => window.__arugula.client.setActive(p), pane);

export function focusPerScreenTests(profile: Phone) {
  test.afterAll(closeContexts);
  test.use({ viewport: { width: 1440, height: 900 } });

  test("a focus on the desktop never hides a pane from the phone", async ({ page: desktop, browser }) => {
    await reset(desktop);
    for (const edge of ["right", "bottom"] as const) {
      const from = (await panes(desktop)).at(-1)!;
      await desktop.evaluate(([pane, edge]) => window.__arugula.client.intent({ op: "split", pane, edge }), [from, edge] as const);
    }
    await expect.poll(() => panes(desktop)).toHaveLength(3);
    const [one, two, three] = await panes(desktop);

    // The desktop focuses the first pane.
    const toolbar = desktop.locator("[data-pane-toolbar]");
    await desktop.evaluate((p) => window.__arugula.client.setActive(p), one);
    await toolbar.locator("[data-pane-focus]").click();
    await expect.poll(() => focus(desktop)).toBe(one);
    await expect.poll(() => shownIds(desktop)).toEqual([one]);

    // The phone opens the tab, then another pane, and sees it.
    const phone = await (await browser.newContext(profile)).newPage();
    await open(phone);
    await tap(phone, two);
    await expect.poll(() => shownIds(phone)).toEqual([two]);
    expect(await focus(phone)).toBeNull();
    await tap(phone, three);
    await expect.poll(() => shownIds(phone)).toEqual([three]);
    await tap(phone, one);
    await expect.poll(() => shownIds(phone)).toEqual([one]);

    // The desktop focuses again without being refused: it takes the tab
    // back. The phone is then the one it traps no longer: its tap wins.
    await desktop.evaluate((p) => window.__arugula.client.setActive(p), two);
    await toolbar.locator("[data-pane-focus]").click();
    await expect.poll(() => focus(desktop)).toBe(two);
    await expect.poll(() => shownIds(desktop)).toEqual([two]);
    await tap(phone, three);
    await expect.poll(() => shownIds(phone)).toEqual([three]);
    await expect.poll(() => focus(desktop)).toBeNull();

    // Restoring on the desktop (which took the tab to focus) hides nothing
    // from the phone: it sees every pane, and a tap still opens one.
    await toolbar.locator("[data-pane-focus]").click();
    await expect.poll(() => focus(desktop)).toBe(two);
    await toolbar.locator("[data-pane-focus]").click();
    await expect.poll(() => focus(desktop)).toBeNull();
    await expect.poll(() => shownIds(phone)).toHaveLength(3);
    await tap(phone, two);
    await expect.poll(() => shownIds(phone)).toEqual([two]);
    expect(await focus(desktop)).toBeNull();

    // With the phone gone, the desktop focuses and restores as ever.
    await phone.context().close();
    await toolbar.locator("[data-pane-focus]").click();
    await expect.poll(() => focus(desktop)).not.toBeNull();
    await expect.poll(() => shownIds(desktop)).toHaveLength(1);
    await toolbar.locator("[data-pane-focus]").click();
    await expect.poll(() => focus(desktop)).toBeNull();
    await expect.poll(() => shownIds(desktop)).toHaveLength(3);
  });
}
