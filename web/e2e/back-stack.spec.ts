// Lift 3 (#1479): one back stack across projects. Escape goes back a place
// when nothing else takes it, across a switch of project, and a reload
// keeps the stack. Never from a field.

import { expect, test } from "@playwright/test";
import { away, mark, openApp, P, rail, SITE } from "./stack-steps";

test.use({ permissions: ["local-network-access"], viewport: { width: 1280, height: 800 } });

test("Escape goes back a place, across projects, and after a reload", async ({ page }) => {
  await openApp(page, `/p/${P}/boards`);
  await rail(page, "docs").click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
  await mark(page, SITE).click();
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*$`));

  // Back across the switch, to the other project's Docs.
  await away(page);
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));

  // A reload keeps it: Escape goes on back to Boards.
  await page.reload();
  await expect(page.locator("[data-rail]")).toBeVisible();
  await away(page);
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
});

test("Escape in a field is the field's", async ({ page }) => {
  await openApp(page, `/p/${P}/docs`);
  await rail(page, "boards").click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
  const field = page.getByRole("searchbox").or(page.locator("input[type=search], input[type=text]")).first();
  await field.focus();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
});

test("Escape that closes a menu closes only the menu", async ({ page }) => {
  await openApp(page, `/p/${P}/boards`);
  await rail(page, "docs").click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
  // The account menu, at the rail's foot: Escape closes it, and the keys go
  // back to the rail, where Escape would otherwise go back a place.
  await page.locator("[data-rail] [data-account-menu]").click();
  await expect(page.getByRole("menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu")).toHaveCount(0);
  await page.waitForTimeout(300);
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
});
