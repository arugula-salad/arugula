// Arugula #1071: an issue's comment shows its replies like a channel
// message's, and an answer's chip opens the thread at the reply it names:
// on a fixture page with no daemon (issue-thread-links-page.tsx).
import { expect, test } from "@playwright/test";
import { fixturePage } from "./fixture-page";

test("an issue's comment opens its thread, and an answer its reply", async ({ page }) => {
  const body = await fixturePage("./issue-thread-links-page.tsx");
  await page.route("**/issue-thread-links.html*", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto("/issue-thread-links.html");
  await expect(page.locator("[data-thread-footer]")).toContainText("2 replies");
  await page.locator("[data-thread-footer]").click();
  await page.locator(".receipt-icon").click();
  await page.locator(".mention-link").click();
  await expect.poll(() => page.evaluate(() => (window as unknown as { opened: unknown[][] }).opened)).toEqual([
    ["ob_aaaa0002", null, 31],
    ["ob_aaaa0002", 32, 31],
  ]);
});
