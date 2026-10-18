// M112b (#1027): the activity drawer under the workspace. A command run
// in a pane shows under Commands; Refresh merges by id and doesn't show it
// twice; clicking it brings its pane back; the panes above give the drawer
// their rows, and it is never one of the tab's panes.

import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, menu, paneEl, panes, ready, reset, run, tab } from "./helpers";

test.afterAll(closeContexts);
test.use({ viewport: { width: 1440, height: 900 } });

async function newWorkspace(page: Page) {
  await page.addInitScript(() => {
    try {
      localStorage.removeItem("arugula.workspace.activity");
    } catch {
      // No storage: the drawer starts as it would anyway.
    }
  });
  await reset(page);
  await expect(page.locator(".ws-rail")).toBeVisible();
}

test("a command shows under Commands, Refresh doesn't repeat it, and it brings its pane back", async ({ page }) => {
  await newWorkspace(page);
  const first = await active(page);
  await ready(page, first);
  await run(page, first, "echo activity-$((6*7))", "activity-42");

  // Opening the drawer takes rows from the tab, which keeps its one pane.
  const rowsBefore = (await tab(page)).rows;
  await page.locator("[data-activity-toggle]").click();
  const drawer = page.locator("[data-activity-drawer]");
  await expect(drawer).toHaveClass(/open/);
  await expect.poll(async () => (await tab(page)).rows).toBeLessThan(rowsBefore);
  expect(await panes(page)).toEqual([first]);

  await drawer.locator('[data-activity-filter="commands"]').click();
  const entry = drawer.locator(".ws-activity-row", {
    hasText: "echo activity-$((6*7))",
  });
  await expect(entry).toHaveCount(1);
  await expect(entry).toHaveAttribute("data-activity-kind", "command");
  await expect(entry).toHaveAttribute("data-activity-pane", String(first));
  const id = await entry.getAttribute("data-activity-id");
  const count = await drawer.locator(".ws-activity-row").count();

  await drawer.locator("[data-activity-refresh]").click();
  await drawer.locator("[data-activity-refresh]").click();
  await expect(drawer.locator(`[data-activity-id="${id}"]`)).toHaveCount(1);
  await expect(drawer.locator(".ws-activity-row")).toHaveCount(count);

  // Another pane has the keys; the entry gives them back to its own.
  await menu(page, paneEl(page, first), "Split right");
  await expect.poll(() => panes(page)).toHaveLength(2);
  const second = (await panes(page)).find((p) => p !== first)!;
  await expect.poll(() => active(page)).toBe(second);
  await entry.click();
  await expect.poll(() => active(page)).toBe(first);
});

// #1035: System's events are kept, so a reload still shows a pane that
// opened and closed, once, with the id the live stream gave it.
test("System survives a reload: a closed pane's events show once", async ({ page }) => {
  await newWorkspace(page);
  const first = await active(page);
  await menu(page, paneEl(page, first), "Split right");
  await expect.poll(() => panes(page)).toHaveLength(2);
  const second = (await panes(page)).find((p) => p !== first)!;

  await page.locator("[data-activity-toggle]").click();
  const drawer = page.locator("[data-activity-drawer]");
  await drawer.locator('[data-activity-filter="system"]').click();
  const row = (text: string) => drawer.locator(`.ws-activity-row[data-activity-pane="${second}"]`, { hasText: text });
  await expect(row("Pane opened")).toHaveCount(1);
  const opened = await row("Pane opened").getAttribute("data-activity-id");

  await page.evaluate((pane) => window.__arugula.client.intent({ op: "close_pane", pane }), second);
  await expect.poll(() => panes(page)).toEqual([first]);
  await expect(row("Pane closed")).toHaveCount(1);
  // The stream and the poll's read are one event, not two.
  await drawer.locator("[data-activity-refresh]").click();
  await expect(row("Pane closed")).toHaveCount(1);
  await expect(row("Pane opened")).toHaveCount(1);

  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  await page.locator("[data-activity-toggle]").click();
  await drawer.locator('[data-activity-filter="system"]').click();
  await expect(row("Pane closed")).toHaveCount(1);
  await expect(row("Pane opened")).toHaveCount(1);
  await expect(row("Pane opened")).toHaveAttribute("data-activity-id", opened!);
  await expect(drawer.locator("[data-activity-since]")).toHaveCount(0);
});
