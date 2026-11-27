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
  // Playwright's WebKit has a PushManager but no push service: once the
  // daemon's page has registered its worker, getSubscription never answers
  // and the page stops (timers, evaluate, screenshots). On a computer the
  // Overview's and Machines' code asks it (ui/notify.tsx's pushState; an
  // iPhone in a tab is "install" first). Safari answers: no subscription.
  await page.addInitScript(() => {
    PushManager.prototype.getSubscription = async () => null;
  });
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
    expect(await hooks(page, "[data-rail] > *", "open")).toEqual(["overview", "boards", "docs", "machines", "chat", "inbox", "settings"]);
    const lit = page.locator("[data-rail] [aria-current=page]");
    // By the rail's buttons: the app's own moves, no page load. Overview
    // and Machines show their place's views.
    for (const [key, url, views] of [
      ["overview", `/p/${P}[^/]*$`, []],
      ["docs", "/docs$", []],
      ["machines", "/machines$", ["machines"]],
      ["boards", "/boards$", []],
      ["chat", "/chat", []],
      ["inbox", "/inbox$", []],
      ["settings", "/settings", []],
    ] as const) {
      await page.locator(`[data-rail] [data-open-${key}]`).click();
      await expect(page).toHaveURL(new RegExp(url));
      await expect(page.locator(`[data-rail] [data-open-${key}]`)).toHaveAttribute("aria-current", "page");
      await expect(lit, key).toHaveCount(1);
      await expect.poll(() => hooks(page, "[data-place-views] > *", "view"), key).toEqual(views);
    }
    await expect(page.locator("[data-rail] [data-open-overview]")).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*$`));
    await expect(page.locator("[data-rail] [data-open-machines]")).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*/machines$`));
    // The Runs list's address is Running, over Machines (Lift 6a, #1483); closing it leaves Machines.
    await page.goto(`/p/${P}/runs`);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines\\?running$`));
    await expect(page.locator("[data-rail] [data-open-machines]")).toHaveAttribute("aria-current", "page");
    await expect(lit).toHaveCount(1);
    await expect(page.getByRole("dialog", { name: "Running" }).locator("[data-kit=sheet]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    // Swarm's old address lands on Machines (Lift 6a).
    await page.goto(`/p/${P}/machines/swarm`);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await expect(page.locator("[data-rail] [data-open-machines]")).toHaveAttribute("aria-current", "page");
    await expect(lit).toHaveCount(1);
  });
});

test.describe("on an iPhone", () => {
  test.use({ ...iphone, viewport: { width: 390, height: 844 } });

  test("the five places are a bar along the bottom, the Inbox a bell in the top bar, and no page scrolls sideways", async ({ page }) => {
    await openApp(page, () => `/p/${P}/boards`);
    await expect(page.locator("[data-rail]")).toHaveCount(0);
    expect(await hooks(page, "[data-phone-tabs] > *", "tab")).toEqual(["overview", "boards", "docs", "machines", "chat"]);
    await expect(page.locator("[data-phone-tabs] [data-tab-boards]")).toHaveAttribute("aria-current", "page");
    const bell = page.locator("[data-platform-bar] [data-phone-inbox]");
    await expect(bell.locator("[data-count]")).toHaveText("4");
    const sideways = () => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    for (const path of ["", "/boards", "/docs", "/chat", "/machines", "/runs"]) {
      await page.goto(`/p/${P}${path}`, { waitUntil: "domcontentloaded" });
      await expect(page.locator("[data-platform-bar]")).toBeVisible();
      expect(await sideways(), path || "/").toBeLessThanOrEqual(0);
    }
    await bell.click();
    await expect(page).toHaveURL(/\/inbox/);
    await expect(bell).toHaveAttribute("aria-current", "page");
    expect(await sideways(), "/inbox").toBeLessThanOrEqual(0);
  });
});
