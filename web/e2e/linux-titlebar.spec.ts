// The desktop app on Linux (M46): no decorations, so the top bar is the
// titlebar and carries the window's buttons. They end at the window's right
// edge, in its top strip, at any width: desktop-xvfb's titlebar claim
// (packaging/desktop/xvfb/app.sh, laid_out) finds Minimize 120 from the
// right, which the kit's top bar (Lift 3, #1479) put 12 further in.

import { expect, test } from "@playwright/test";
import { reset } from "./helpers";

test("on Linux the window's buttons end at its right edge, in the top strip", async ({ page }) => {
  await page.addInitScript(() => {
    const w = { minimize: async () => {}, toggleMaximize: async () => {}, close: async () => {} };
    Object.assign(window, { __arugulaApp: { name: "Arugula app on test-linux", platform: "linux" }, __TAURI__: { window: { getCurrentWindow: () => w } } });
  });
  for (const [width, height] of [
    [1000, 700],
    [1280, 900],
  ] as const) {
    await page.setViewportSize({ width, height });
    await reset(page);
    const min = await page.getByRole("button", { name: "Minimize", exact: false }).first().boundingBox();
    expect(min, `${width}x${height}`).not.toBeNull();
    expect(Math.abs(min!.x - (width - 120)), `${width}x${height}: Minimize at ${min!.x},${min!.y}`).toBeLessThanOrEqual(2);
    expect(min!.y, `${width}x${height}`).toBeLessThanOrEqual(20);
    const close = (await page.locator('[data-window="close"]').boundingBox())!;
    expect(Math.round(close.x + close.width)).toBe(width);
    // One line, and the crumb shortens its steps rather than run into the views.
    const top = page.locator("[data-top]").first();
    expect((await top.boundingBox())!.height).toBeLessThanOrEqual(47);
    const spill = await top.locator("[data-app-path]").evaluate((c) => {
      const r = c.getBoundingClientRect().right;
      return Math.max(0, ...[...c.querySelectorAll("*")].map((e) => e.getBoundingClientRect().right - r));
    });
    expect(spill, `${width}x${height}: the crumb runs ${spill}px past itself`).toBeLessThanOrEqual(1);
    await top.screenshot({ path: test.info().outputPath(`top-${width}.png`) });
  }
});
