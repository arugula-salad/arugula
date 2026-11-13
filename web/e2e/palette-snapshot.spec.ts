// A snapshot sets all 256 colors. The theme follows only those that differ
// from what it has, and only once for all of them: setting it per color held
// up a page with many terminals for a second or more.

import { expect, test, type Page } from "@playwright/test";
import type { PaneId } from "../src/shared/proto";
import { active, closeContexts, ready, reset, run } from "./helpers";

test.afterAll(closeContexts);

type Counted = { applyTheme(): void; themeSets: number; term: { options: { theme: { red?: string; green?: string } } } };

const count = (page: Page, pane: PaneId) =>
  page.evaluate((p) => {
    const v = window.__arugula.client.panes.get(p)!.view as unknown as Counted;
    const set = v.applyTheme;
    v.themeSets = 0;
    v.applyTheme = () => {
      v.themeSets++;
      set.call(v);
    };
  }, pane);
const sets = (page: Page, pane: PaneId) =>
  page.evaluate((p) => (window.__arugula.client.panes.get(p)!.view as unknown as Counted).themeSets, pane);
const red = (page: Page, pane: PaneId) =>
  page.evaluate((p) => (window.__arugula.client.panes.get(p)!.view as unknown as Counted).term.options.theme.red, pane);

/** Its snapshot again, written to the end. */
const resnapshot = (page: Page, pane: PaneId) =>
  page.evaluate(
    (p) =>
      new Promise<void>((done) => {
        const c = window.__arugula.client as unknown as { attach(a: { pane: PaneId; offset: null }[]): void; panes: Map<PaneId, { view: { write(d: Uint8Array, cb?: () => void): void } }> };
        const v = c.panes.get(p)!.view;
        const write = v.write;
        v.write = (d, cb) => {
          v.write = write;
          write.call(v, d, () => {
            cb?.();
            // After the parse's own microtasks.
            setTimeout(done, 0);
          });
        };
        c.attach([{ pane: p, offset: null }]);
      }),
    pane,
  );

test("a snapshot's colors set the theme only when they change, and once", async ({ page }) => {
  await reset(page);
  const pane = await active(page);
  await ready(page, pane);
  const start = await red(page, pane);
  await count(page, pane);

  await resnapshot(page, pane);
  expect(await sets(page, pane)).toBe(0);
  expect(await red(page, pane)).toBe(start);

  // Two colors a program sets, in one write.
  await run(page, pane, String.raw`printf '\033]4;1;rgb:12/34/56\a\033]4;2;rgb:65/43/21\a'; echo set-$((40+2))`, "set-42");
  await expect.poll(() => red(page, pane)).toBe("#123456");
  expect(await sets(page, pane)).toBe(1);

  // The snapshot now brings them back: nothing changes, and they stay.
  await resnapshot(page, pane);
  expect(await sets(page, pane)).toBe(1);
  expect(await red(page, pane)).toBe("#123456");

  await run(page, pane, String.raw`printf '\033]104\a'; echo reset-$((40+3))`, "reset-43");
  await expect.poll(() => red(page, pane)).toBe(start);
  expect(await sets(page, pane)).toBe(2);
});
