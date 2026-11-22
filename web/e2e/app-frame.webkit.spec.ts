// The app's rail and bars (Arugula #1345) in Safari's engine: app-frame.spec.ts's
// checks of the places (the rail's five, in order, the one lit on each page),
// the place's views in the bar, and on an iPhone the bar along the bottom,
// the Inbox's bell and nothing wider than the phone. On the same fixture
// page (app-frame-page.tsx), on the suite's daemon's origin.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { inProject, panes, reset } from "./helpers";
import { iphone } from "./phones";

const P = "prj_aaaa0002";

/** A fresh pane on the daemon's own page, then the app at `path(pane)`. */
async function openApp(page: Page, path: (pane: number) => string): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  await inProject(page, P, "Arugula");
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path(pane));
  return pane;
}

/** The keys of the `data-<prefix>-…` hooks of `sel`'s matches, in order. */
const hooks = (page: Page, sel: string, prefix: string) =>
  page.locator(sel).evaluateAll((els, pre) => els.map((e) => [...e.attributes].find((a) => a.name.startsWith(pre))?.name.slice(pre.length) ?? "").filter(Boolean), `data-${prefix}-`);

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the rail is a button per place, one lit on each page; the bar has the place's views", async ({ page }) => {
    await openApp(page, () => `/p/${P}/boards`);
    expect(await hooks(page, ".ws-rail > *", "open")).toEqual(["overview", "boards", "docs", "machines", "chat", "inbox", "settings"]);
    const lit = page.locator(".ws-rail [aria-current=page]");
    // By the rail's buttons: the app's own moves, no page load. Not the
    // Overview or Machines on a computer: WebKit stopped on those pages on
    // main too, apart from the rail (seen before #1346 rebuilt Machines);
    // app-frame.spec.ts has them, and the iPhone's below.
    for (const [key, url] of [
      ["docs", "/docs$"],
      ["boards", "/boards$"],
      ["chat", "/chat"],
      ["inbox", "/inbox$"],
      ["settings", "/settings"],
    ] as const) {
      await page.locator(`.ws-rail [data-open-${key}]`).click();
      await expect(page).toHaveURL(new RegExp(url));
      await expect(page.locator(`.ws-rail [data-open-${key}]`)).toHaveAttribute("aria-current", "page");
      await expect(lit, key).toHaveCount(1);
      await expect(page.locator("[data-place-views]"), key).toHaveCount(0);
    }
    await expect(page.locator(".ws-rail [data-open-overview]")).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*$`));
    await expect(page.locator(".ws-rail [data-open-machines]")).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*/machines$`));
    // Runs are Machines': lit, with its views.
    await page.goto(`/p/${P}/runs`);
    await expect(page.locator(".ws-rail [data-open-machines]")).toHaveAttribute("aria-current", "page");
    await expect(lit).toHaveCount(1);
    await expect.poll(() => hooks(page, "[data-place-views] > *", "view")).toEqual(["machines", "runs"]);
    await expect(page.locator("[data-view-runs]")).toHaveAttribute("aria-current", "page");
    // The Swarm is the Overview's: its route lights the Overview.
    await page.goto(`/p/${P}/machines/swarm`);
    await expect(page.locator(".ws-rail [data-open-overview]")).toHaveAttribute("aria-current", "page");
    await expect(lit).toHaveCount(1);
  });
});

test.describe("on an iPhone", () => {
  test.use({ ...iphone, viewport: { width: 390, height: 844 } });

  test("the five places are a bar along the bottom, the Inbox a bell in the top bar, and no page scrolls sideways", async ({ page }) => {
    await openApp(page, () => `/p/${P}/boards`);
    await expect(page.locator(".ws-rail")).toHaveCount(0);
    expect(await hooks(page, "[data-phone-tabs] > *", "tab")).toEqual(["overview", "boards", "docs", "machines", "chat"]);
    await expect(page.locator("[data-phone-tabs] [data-tab-boards]")).toHaveAttribute("aria-current", "page");
    const bell = page.locator("[data-platform-bar] [data-phone-inbox]");
    await expect(bell.locator(".ws-rail-count")).toHaveText("4");
    const sideways = () => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    for (const path of ["", "/boards", "/docs", "/chat", "/machines", "/runs"]) {
      await page.goto(`/p/${P}${path}`, { waitUntil: "domcontentloaded" });
      await expect(page.locator("[data-platform-bar]")).toBeVisible();
      // No Swarm on a phone's Overview.
      if (!path) await expect(page.locator("[data-view-swarm]")).toHaveCount(0);
      expect(await sideways(), path || "/").toBeLessThanOrEqual(0);
    }
    await bell.click();
    await expect(page).toHaveURL(/\/inbox/);
    await expect(bell).toHaveAttribute("aria-current", "page");
    expect(await sideways(), "/inbox").toBeLessThanOrEqual(0);
  });
});
