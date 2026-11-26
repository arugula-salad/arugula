// Lift 3 (#1479): flip back (`g ;`, ⌃Tab) goes to your last place in
// another project, and again comes back; Go to…, empty, lists the places
// you were, across projects, newest first.

import { expect, test } from "@playwright/test";
import { away, mark, openApp, P, rail, SITE } from "./stack-steps";

test.use({ permissions: ["local-network-access"], viewport: { width: 1280, height: 800 } });

test("g ; flips to the last place in another project, and again back", async ({ page }) => {
  await openApp(page, `/p/${P}/boards`);
  await mark(page, SITE).click();
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*$`));
  await rail(page, "docs").click();
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/docs$`));

  await away(page);
  await page.keyboard.press("g");
  await page.keyboard.press(";");
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
  // Again, back. (⌃Tab is the same key where the browser lets it through;
  // `g ;` is the one it can't take.)
  await away(page);
  await page.keyboard.press("g");
  await page.keyboard.press(";");
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/docs$`));
});

test("Go to…, empty, starts with where you were, across projects", async ({ page }) => {
  await openApp(page, `/p/${P}/boards`);
  await mark(page, SITE).click();
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*$`));
  await page.locator("[data-open-palette]").click();
  const first = page.locator("[data-goto] [role=option]").first();
  await expect(first).toHaveAttribute("data-command", new RegExp(`^recent/\\/p\\/${P}`));
  await expect(first).toContainText("Arugula");
  await expect(first).toContainText("Boards");
  await first.click();
  await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
});
