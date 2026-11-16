// A post that names two sessions of one machine gets a line for each, as
// control keeps a status per (machine, session) (#875): on a fixture page
// with no daemon (mention-lines-page.tsx).
import { expect, test } from "@playwright/test";
import { fixturePage } from "./fixture-page";

test("a line per session a post names", async ({ page }) => {
  const body = await fixturePage("./mention-lines-page.tsx");
  await page.route("**/mention-lines.html*", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto("/mention-lines.html");
  const lines = page.locator(".mention-line");
  await expect(lines).toHaveText(["geek/chat: working…", "Sending to geek/review…", "mini/build answered", "air is offline or didn't answer"]);
  await expect(page.locator('.mention-line[data-machine="mch_geek"]')).toHaveCount(2);
  await expect(page.locator('.mention-line[data-machine="mch_geek"][data-session="review"]')).toHaveAttribute("data-mention-status", "sending");
});
