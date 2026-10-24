// #820: on a phone, control's page has the account menu's items in the
// sheet (a desktop has them in the bar's account menu): a machine is
// removed from there, its confirmation fitting the screen, and Sign out.

import { expect, test, type Page } from "@playwright/test";
import { TeamControl, home, show } from "./team-fixture";
import { sheetTab } from "./helpers";

const ctl = new TeamControl("phone-account");
/** A small phone: what fits here fits a bigger one. */
const SCREEN = { width: 375, height: 667 };
let phone: Page;

test.describe.configure({ mode: "serial" });
test.beforeAll(() => ctl.start());
test.afterAll(async () => {
  await phone?.context().close();
  ctl.stop();
});

/** The box is on the screen, whole. */
async function fits(page: Page, selector: string) {
  const box = (await page.locator(selector).boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(SCREEN.width);
  expect(box.y + box.height).toBeLessThanOrEqual(SCREEN.height);
}

const openSheet = async (page: Page) => {
  await page.locator(".sheet-button").click();
  await expect(page.locator(".sheet")).toBeVisible();
  await sheetTab(page, "More");
};

test("the phone's sheet has the account's items; a machine is removed from it", async ({ browser }) => {
  test.setTimeout(90_000);
  phone = await ctl.person(browser, "pat", { viewport: SCREEN, isMobile: true, hasTouch: true });
  await ctl.addMachine(phone, "box");
  await ctl.addMachine(phone, "spare");
  await home(phone);
  await show(phone, "box");
  await expect.poll(() => phone.evaluate(() => window.__arugula.hosts.names), { timeout: 30_000 }).toEqual(["box", "spare"]);

  await openSheet(phone);
  const account = phone.locator("[data-sheet-account]");
  await expect(account.getByRole("heading")).toHaveText("Signed in as pat");
  for (const name of ["Teams…", "Devices and machines…", "Add a phone or browser…", "Sign-in and account…", "Sign out"])
    await expect(account.getByRole("button", { name, exact: true })).toBeVisible();

  await account.getByRole("button", { name: "Devices and machines…", exact: true }).tap();
  await expect(phone.locator(".sheet")).toHaveCount(0);
  await expect(phone.getByRole("heading", { name: "Devices and machines" })).toBeVisible();
  const machines = phone.locator("[data-machines] li");
  await expect(machines).toHaveCount(2);
  const spare = machines.filter({ hasText: /^spare\b/ });
  const id = (await spare.getAttribute("data-device"))!;
  await spare.locator("[data-remove]").tap();

  const ask = phone.locator("[data-confirm-dialog]");
  await expect(ask).toContainText("Remove spare?");
  await expect(ask.locator("[data-remove-explain]")).toContainText("keeps running on it, reachable only locally");
  await fits(phone, "[data-confirm-dialog]");
  await expect(ask.locator("[data-cancel-remove]")).toBeInViewport({ ratio: 1 });
  await expect(ask.locator("[data-confirm-remove]")).toBeInViewport({ ratio: 1 });
  await expect(ask.locator("[data-confirm-remove]")).toBeEnabled();
  await ask.locator("[data-confirm-remove]").tap();

  await expect(ask).toHaveCount(0);
  await expect(machines).toHaveCount(1);
  await expect(machines).toHaveText(/^box\b/);
  expect(await phone.evaluate((d) => window.__arugula.control!.trusted.has(d), id)).toBe(false);
  await expect.poll(() => phone.evaluate(() => window.__arugula.hosts.names), { timeout: 20_000 }).toEqual(["box"]);
  await phone.getByRole("button", { name: "Done", exact: true }).tap();
});

test("Sign out, from the phone's sheet", async () => {
  await openSheet(phone);
  await phone.locator("[data-sheet-account]").getByRole("button", { name: "Sign out", exact: true }).tap();
  await expect(phone.locator("[data-signin=github]")).toBeVisible();
});
