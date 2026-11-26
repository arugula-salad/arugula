// The app on its fixture page (app-frame-page.tsx), for the back stack's
// and flip back's specs (Lift 3, #1479): every project's address is the
// fixture, so a reload anywhere lands on it.

import { expect, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { inProject, reset } from "./helpers";

export const P = "prj_aaaa0002";
export const SITE = "prj_aaaa0003";

export async function openApp(page: Page, path: string) {
  await reset(page);
  await inProject(page, P, "Arugula");
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(/\/p\/prj_/, (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
  await expect(page.locator("[data-rail]")).toBeVisible();
}

export const rail = (page: Page, place: string) => page.locator(`[data-rail] [data-open-${place}]`);
export const mark = (page: Page, project: string) => page.locator(`[data-rail] [data-project-mark="${project}"]`);

/** Keys to the page, not a field or a terminal: once a page that has just
 * drawn has put its focus where it wants (Boards' filter takes it), take it
 * back, until it stays. */
export async function away(page: Page) {
  await expect
    .poll(async () => {
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await page.waitForTimeout(100);
      return page.evaluate(() => document.activeElement === document.body);
    })
    .toBe(true);
}
