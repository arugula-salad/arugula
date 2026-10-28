// M107 (#1022): Ctrl+Shift+1–9 goes to the Nth pane of the tab in reading
// order (ui/keys.ts), matched by code; a number with no pane goes on to the
// terminal, as every key the table doesn't take does.

import { expect, test } from "@playwright/test";
import { active, paneEl, panes, ready, reset, run, text } from "./helpers";

test("Ctrl+Shift+N selects pane N in reading order; an N past the last pane reaches the terminal", async ({ page }) => {
  await reset(page);
  const first = await active(page);
  await page.evaluate((pane) => window.__arugula.client.intent({ op: "split", pane, edge: "right" }), first);
  await expect.poll(() => panes(page)).toHaveLength(2);
  const right = (await panes(page)).find((p) => p !== first)!;
  await page.evaluate((pane) => window.__arugula.client.intent({ op: "split", pane, edge: "bottom" }), first);
  await expect.poll(() => panes(page)).toHaveLength(3);
  const order = await panes(page);
  for (const p of order) await ready(page, p);
  // Left column top, left column bottom, then the right: reading order.
  expect(order[0]).toBe(first);
  expect(order[2]).toBe(right);

  await paneEl(page, first).click({ position: { x: 40, y: 40 } });
  await expect.poll(() => page.evaluate(() => document.activeElement?.closest<HTMLElement>("[data-pane]")?.dataset.pane)).toBe(String(first));
  for (const n of [3, 2, 1, 3]) {
    await page.keyboard.press(`Control+Shift+Digit${n}`);
    await expect.poll(() => active(page)).toBe(order[n - 1]);
    // The keys go with it.
    await expect.poll(() => page.evaluate(() => document.activeElement?.closest<HTMLElement>("[data-pane]")?.dataset.pane)).toBe(String(order[n - 1]));
  }

  // No ninth pane: the chord isn't taken, and the pane keeps the keys.
  await page.keyboard.press("Control+Shift+Digit9");
  expect(await active(page)).toBe(order[2]);
  await run(page, order[2], "echo still-$((6*7))", "still-42");
  expect(await text(page, order[2])).toContain("still-42");
});

test("Ctrl+Shift+L goes to the next pane that needs you; with none, the terminal gets it", async ({ page }) => {
  await reset(page);
  const first = await active(page);
  await ready(page, first);
  // Nothing needs you yet: the key goes on.
  await paneEl(page, first).click({ position: { x: 40, y: 40 } });
  await page.keyboard.press("Control+Shift+KeyL");
  expect(await active(page)).toBe(first);

  // A command fails in a tab nobody is looking at (attention.spec.ts's way).
  const shown = await page.evaluate(() => window.__arugula.client.tab!);
  const failing = await page.evaluate(async () => (await (await fetch("/api/run", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" })).json()).pane as number);
  await expect.poll(() => page.evaluate((p) => !!window.__arugula.client.info(p)?.running, failing)).toBe(true);
  await page.evaluate((t) => window.__arugula.client.selectTab(t), shown);
  await page.waitForTimeout(500);
  for (const line of ["cargo() { sleep 3.2; echo 'test result: FAILED'; return 101; }", "cargo test"])
    await page.evaluate(
      ({ pane, text }) => fetch(`/api/panes/${pane}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text, enter: true }) }),
      { pane: failing, text: line },
    );
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.reason?.kind ?? null, failing), { timeout: 15_000 }).toBe("failed");

  await paneEl(page, first).click({ position: { x: 40, y: 40 } });
  await page.keyboard.press("Control+Shift+KeyL");
  await expect.poll(() => active(page)).toBe(failing);
  await expect.poll(() => page.evaluate(() => document.activeElement?.closest<HTMLElement>("[data-pane]")?.dataset.pane)).toBe(String(failing));
});
