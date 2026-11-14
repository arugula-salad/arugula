// Control's pages move to the app (Arugula #989): the shell's page, told by
// /control.json that its pages live in the app (`pages: "app"`), sends its
// own addresses there, and keeps signing in, secrets and chat. Control
// isn't here: /control.json is made up on the suite's daemon, and the app
// is a stand-in page at its origin.
import { expect, test, type Page } from "@playwright/test";

const APP = "http://app.test";
const D = "da5b786733ac68a9";

async function asControl(page: Page, pages: string | null) {
  await page.route(/\/control\.json$/, (r) =>
    r.fulfill({ contentType: "application/json", body: JSON.stringify({ control: true, url: new URL(r.request().url()).origin, platform: APP, ...(pages ? { pages } : {}) }) }),
  );
  await page.route(`${APP}/**`, (r) => r.fulfill({ contentType: "text/html", body: "<!doctype html><title>the app</title><p data-app-stand-in>the app</p>" }));
}

test("with the cloud's word, a pane's address opens at its place in the app", async ({ page }) => {
  await asControl(page, "app");
  await page.goto(`/#pane=${D}.3&share`);
  await expect(page).toHaveURL(`${APP}/machines/${D}?pane=3&share`);
  await expect(page.locator("[data-app-stand-in]")).toBeVisible();
});

test("and Swarm's, and a machine's", async ({ page }) => {
  await asControl(page, "app");
  await page.goto(`/#swarm=board:brd_e2e`);
  await expect(page).toHaveURL(`${APP}/machines/swarm?board=brd_e2e`);
  await page.goto(`/#machine=${D}&agent`);
  await expect(page).toHaveURL(`${APP}/machines/${D}?agent`);
});

test("signing in, secrets and chat stay on control's page", async ({ page }) => {
  await asControl(page, "app");
  for (const hash of ["#platform-signin=cmV0dXJu", "#join=abc", "#chat-block"]) {
    await page.goto(`/${hash}`);
    // Long enough for a redirect to have happened.
    await page.waitForTimeout(500);
    expect(new URL(page.url()).origin, hash).not.toBe(APP);
  }
});

test("without the cloud's word, nothing moves", async ({ page }) => {
  await asControl(page, null);
  await page.goto(`/#pane=${D}.3`);
  await page.waitForTimeout(500);
  expect(new URL(page.url()).origin).not.toBe(APP);
});
