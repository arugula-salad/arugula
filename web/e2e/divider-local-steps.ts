// #442: a split divider moves with the pointer at once, and the terminals are
// resized when the pointer settles or is let go, not at every step.
import { expect, test, type Page } from "@playwright/test";
import { active, menu, paneEl, panes, reset, tab } from "./helpers";

/** The daemon's replies reach the page `delay.ms` late; what the page sends is counted. */
async function instrument(page: Page) {
  const delay = { ms: 0 };
  await page.routeWebSocket(/\/ws(\?|$)/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((m) => server.send(m));
    server.onMessage((m) => (delay.ms ? setTimeout(() => ws.send(m), delay.ms) : ws.send(m)));
    ws.onClose((c, r) => server.close({ code: c, reason: r }));
    server.onClose((c, r) => ws.close({ code: c, reason: r }));
  });
  await page.addInitScript(() => {
    const w = window as unknown as { __resize: number; __ws: number };
    w.__resize = 0;
    w.__ws = 0;
    const Orig = window.WebSocket;
    window.WebSocket = class extends Orig {
      send(d: string | ArrayBufferLike | Blob | ArrayBufferView) {
        w.__ws++;
        if (typeof d === "string" && d.includes("resize_split")) w.__resize++;
        super.send(d);
      }
    };
  });
  return delay;
}

const sent = (page: Page) => page.evaluate(() => (window as unknown as { __resize: number }).__resize);

async function split(page: Page) {
  await reset(page);
  const first = await active(page);
  await menu(page, paneEl(page, first), "Split right");
  await expect.poll(() => panes(page)).toHaveLength(2);
  const divider = page.locator(".divider.live").first();
  await expect(divider).toBeVisible();
  // The layout settles before we measure.
  await page.waitForTimeout(500);
  return divider;
}

export function dividerLocalSpecs() {
  test("the divider is at the pointer before the daemon replies", async ({ page }) => {
    const delay = await instrument(page);
    const divider = await split(page);
    const before = (await divider.boundingBox())!;
    const layout = (await tab(page)).layout.panes.map(([, r]) => r.cols);
    delay.ms = 3000;
    const y = before.y + before.height / 2;
    const x0 = before.x + before.width / 2;
    await page.mouse.move(x0, y);
    await page.mouse.down();
    await page.mouse.move(x0 + 5, y, { steps: 2 });
    await page.mouse.move(x0 + 120, y, { steps: 12 });
    // Still down, and the pointer is still: the preview is at the pointer, within a cell.
    const mid = (await divider.boundingBox())!;
    expect(Math.abs(mid.x - before.x - 120)).toBeLessThanOrEqual(12);
    await page.mouse.up();
    // Released with no reply yet (the replies are 3 s late): it holds there.
    const held = (await divider.boundingBox())!;
    expect(Math.abs(held.x - before.x - 120)).toBeLessThanOrEqual(12);
    expect((await tab(page)).layout.panes.map(([, r]) => r.cols)).toEqual(layout);
    expect(await sent(page)).toBe(1);
    // The daemon's layout arrives: the panes moved, and the divider is where the layout has it.
    await expect.poll(async () => (await tab(page)).layout.panes[0][1].cols, { timeout: 10_000 }).toBeGreaterThan(layout[0]);
    await expect(async () => {
      const b = (await divider.boundingBox())!;
      expect(await divider.evaluate((e) => (e as HTMLElement).style.transform)).toBe("");
      expect(Math.abs(b.x - held.x)).toBeLessThanOrEqual(12);
    }).toPass({ timeout: 10_000 });
  });

  test("a 2 s drag sends at most 3 resizes, and ends in the daemon's layout", async ({ page }) => {
    await instrument(page);
    const divider = await split(page);
    const before = (await divider.boundingBox())!;
    const y = before.y + before.height / 2;
    const x0 = before.x + before.width / 2;
    const steps = 120;
    await page.mouse.move(x0, y);
    await page.mouse.down();
    const t0 = Date.now();
    for (let i = 1; i <= steps; i++) {
      await page.mouse.move(x0 + (200 * i) / steps, y);
      const wait = t0 + (i * 2000) / steps - Date.now();
      if (wait > 0) await page.waitForTimeout(wait);
    }
    const mid = (await divider.boundingBox())!;
    expect(Math.abs(mid.x - before.x - 200)).toBeLessThanOrEqual(12);
    await page.mouse.up();
    expect(await sent(page)).toBeLessThanOrEqual(3);
    expect(await sent(page)).toBeGreaterThanOrEqual(1);
    await expect.poll(async () => (await tab(page)).layout.panes[0][1].cols).toBeGreaterThan(0);
    await expect(async () => {
      const b = (await divider.boundingBox())!;
      expect(await divider.evaluate((e) => (e as HTMLElement).style.transform)).toBe("");
      expect(Math.abs(b.x - before.x - 200)).toBeLessThanOrEqual(12);
    }).toPass({ timeout: 10_000 });
  });
}
