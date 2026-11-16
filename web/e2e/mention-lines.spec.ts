// A post that names two sessions of one machine gets a line for each, as
// control keeps a status per (machine, session) (#875): on a fixture page
// with no daemon (mention-lines-page.tsx).
import { expect, test } from "@playwright/test";
import { fixturePage } from "./fixture-page";

test("a line per session a post names", async ({ page }) => {
  const body = await fixturePage("./mention-lines-page.tsx");
  await page.route("**/mention-lines.html*", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto("/mention-lines.html");
  const lines = page.locator(".mention-line:not([data-issue-comment] *)");
  await expect(lines).toHaveText(["geek/chat: working…", "Sending to geek/review…", "mini/build answered", "air is offline or didn't answer"]);
  await expect(page.locator('.mention-line[data-machine="mch_geek"]')).toHaveCount(2);
  await expect(page.locator('.mention-line[data-machine="mch_geek"][data-session="review"]')).toHaveAttribute("data-mention-status", "sending");
});

// Arugula #1071: an issue's comment shows its replies like a channel
// message's, and an answer's chip opens the thread at the reply it names.
test("an issue's comment opens its thread, and an answer its reply", async ({ page }) => {
  const body = await fixturePage("./mention-lines-page.tsx");
  await page.route("**/mention-lines.html*", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto("/mention-lines.html");
  const comment = page.locator("[data-issue-comment]");
  await expect(comment.locator("[data-thread-footer]")).toContainText("2 replies");
  await comment.locator("[data-thread-footer]").click();
  await comment.locator(".mention-link").click();
  await expect.poll(() => page.evaluate(() => (window as unknown as { opened: unknown[][] }).opened)).toEqual([
    ["ob_aaaa0002", null, 31],
    ["ob_aaaa0002", 32, 31],
  ]);
});
