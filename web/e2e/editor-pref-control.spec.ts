// #1731: signed in on control's page, the Editor preference is the
// account's (control's /api/me/prefs, as Tips are): chosen on one device,
// it's what Settings shows on another.

import { expect, test, type Browser, type Page } from "@playwright/test";
import { closeContexts, menuItem } from "./helpers";
import { TeamControl, home, show } from "./team-fixture";

const t = new TeamControl("editor-pref-control");

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  await t.start();
});

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

/** A browser signed in to control as alice, at the approval screen or in. */
async function signIn(browser: Browser): Promise<Page> {
  const ctx = await browser.newContext({ baseURL: t.base });
  await ctx.addCookies([{ name: "as", value: "alice", url: t.github }]);
  const page = await ctx.newPage();
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
  return page;
}

/** Settings › Editor's row for `label`, with the menu open. */
async function editorRow(page: Page, label: string) {
  await page.locator("[data-account-menu]").click();
  return menuItem(page, label, { exact: true });
}

const accountPref = (page: Page) => page.evaluate(() => fetch("/api/me/prefs").then((r) => r.json()).then((p) => p.editor ?? null));

test("Settings › Editor follows the account to another device", async ({ browser }) => {
  test.setTimeout(180_000);
  // The first device: the account's recovery codes, kept for the second.
  const laptop = await signIn(browser);
  await expect(laptop.locator("[data-recovery-code]")).toHaveCount(2);
  const codes = await laptop.locator("[data-recovery-code]").allTextContents();
  await laptop.locator("[data-stored-codes]").check();
  await laptop.locator("[data-saved-codes]").click();
  await laptop.waitForFunction(() => window.__arugula?.control?.phase === "ready");
  await t.addMachine(laptop, "editorbox");
  await home(laptop);
  await show(laptop, "editorbox");
  await expect(await editorRow(laptop, "VS Code")).toHaveAttribute("aria-checked", "true");
  await (await menuItem(laptop, "nvim", { exact: true })).click();
  await expect.poll(() => accountPref(laptop)).toBe("nvim");
  // The account's, not this browser's.
  expect(await laptop.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}").editor ?? null)).toBeNull();

  // Another device, let in with a recovery code.
  const phone = await signIn(browser);
  await phone.locator("[data-use-recovery]").click();
  await phone.getByLabel("Recovery code").fill(codes[0]);
  await phone.getByRole("button", { name: "Use it" }).click();
  await phone.waitForFunction(() => window.__arugula?.control?.phase === "ready", null, { timeout: 30_000 });
  await show(phone, "editorbox");
  await expect(await editorRow(phone, "nvim")).toHaveAttribute("aria-checked", "true");
  await (await menuItem(phone, "Ask each time", { exact: true })).click();
  await expect.poll(() => accountPref(phone)).toBe("ask");
});
