// The machines inside the app's frame (Arugula #989): a real pane of the
// suite's daemon drawn at the app's `/p/<project>/machines…` places, beside
// the app's rail and not the shell's, the machines' list, and Swarm under
// Machines; on a computer and on a phone. On a fixture page
// (app-frame-page.tsx) served on the daemon's origin; the hub's leg (tokens
// through the relay on the app's host) is the cloud's spec.
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

const P = "prj_aaaa0002";

// A page a route fulfils has no address of its own, so Chrome counts it as
// public and won't let it reach the daemon on loopback without this.
test.use({ permissions: ["local-network-access"] });

/** A fresh pane on the daemon's own page, then the app at `path(pane)`. */
async function openApp(page: Page, path: (pane: number) => string): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path(pane));
  return pane;
}

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("a pane opens on its machine's page inside the app, beside the app's rail", async ({ page }, info) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    const frame = page.locator("[data-platform-machines]");
    // One rail, the app's: its Projects, and Panes lit.
    await expect(frame.locator(".ws-rail")).toHaveCount(1);
    await expect(frame.locator(".ws-rail [data-open-projects]")).toBeVisible();
    await expect(frame.locator(".ws-rail [data-open-panes]")).toHaveAttribute("aria-current", /.+/);
    // The machine's page, as it ships: its sessions, tabs and the pane.
    await expect(frame.locator(".app.ws")).toBeVisible();
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    // Typed in the app, run on the machine, drawn back in the app.
    await page.locator(`[data-pane="${pane}"]`).click();
    await page.keyboard.type("echo app-frame-$((6*7))\n");
    await expect.poll(() => page.evaluate((p) => (window as unknown as { __app: { text(p: number): string } }).__app.text(p), pane)).toContain("app-frame-42");
    await page.screenshot({ path: info.outputPath("app-frame-machine.png") });
  });

  test("Machines lists the machines; Swarm is under it", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/machines`);
    const list = page.locator("[data-machines-list]");
    await expect(list.locator("[data-machine-link]")).toHaveCount(1);
    await page.screenshot({ path: info.outputPath("app-frame-machines.png") });
    await page.locator(".ws-rail [data-open-swarm]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines/swarm$`));
    await expect(page.locator("[data-app-swarm] .swarm")).toBeVisible();
    // Beside the app's rail, not over it.
    await expect(page.locator(".ws-rail [data-open-projects]")).toBeVisible();
    expect((await page.locator("[data-app-swarm] .swarm").boundingBox())!.x).toBeGreaterThan(0);
    await expect(page.locator(".ws-rail [data-open-swarm]")).toHaveAttribute("aria-current", /.+/);
    await page.screenshot({ path: info.outputPath("app-frame-swarm.png") });
    // The machine from the list, by its link.
    await page.locator(".ws-rail [data-open-panes]").click();
    await list.locator("[data-machine-link]").click();
    await expect(page.locator("[data-platform-machines] .app.ws")).toBeVisible();
  });

  test("a project's other places are as they were, and Panes is a place of the app", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    await expect(page.locator(".platform-main")).toBeVisible();
    const panesLink = page.locator(".ws-rail [data-open-panes]");
    await expect(panesLink).toHaveAttribute("href", new RegExp(`^/p/${P}[^/]*/machines$`));
    await page.screenshot({ path: info.outputPath("app-frame-boards.png") });
  });
});

test.describe("the shell's keys", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("are the machine page's only: on Boards, Commands no longer opens", async ({ page }) => {
    await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator("[data-platform-machines] .app.ws")).toBeVisible();
    const palette = page.locator(".palette-search");
    // On the machine's page, Commands opens as it does on the shell's page.
    await page.keyboard.press("Control+Shift+KeyP");
    await expect(palette).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(palette).toHaveCount(0);
    // On to Boards, by the app's own links: the same page, no reload.
    await page.locator(".ws-rail [data-open-projects]").click();
    await page.locator("[data-platform] a", { hasText: /^Boards$/ }).first().click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
    await expect(page.locator(".platform-main")).toBeVisible();
    // The chord reaches the page as any key would: nothing takes it.
    await page.evaluate(() => addEventListener("keydown", (e) => ((window as unknown as { prevented: boolean }).prevented = e.defaultPrevented)));
    await page.keyboard.press("Control+Shift+KeyP");
    await expect(palette).toHaveCount(0);
    expect(await page.evaluate(() => (window as unknown as { prevented: boolean }).prevented)).toBe(false);
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("a pane opens full screen with the machine's own header, and no rail", async ({ page }, info) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator(".ws-rail")).toHaveCount(0);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    await page.screenshot({ path: info.outputPath("app-frame-phone.png") });
  });
});
