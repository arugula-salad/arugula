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
  await expect(lines).toHaveText([
    "geek/chat: working…", "Sending to geek/review…", "mini/build answered", "studio/launch lead answered", "air is offline or didn't answer",
    "Not delivered to box: its owner turned agent mentions off",
    "Aarons-Mac-mini.local: no session named v1-engineering-done-board",
  ]);
  await expect(page.locator('.mention-line[data-machine="mch_geek"]')).toHaveCount(2);
  await expect(page.locator('.mention-line[data-machine="mch_geek"][data-session="review"]')).toHaveAttribute("data-mention-status", "sending");
});

test("a no-such-session chip is one line, and its names put @machine/session in the composer (#1167)", async ({ page }) => {
  const body = await fixturePage("./mention-lines-page.tsx");
  await page.route("**/mention-lines.html*", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto("/mention-lines.html");
  const chip = page.locator('.mention-line[data-mention-status="no_such_session"] .chip');
  await expect(chip).toHaveCSS("white-space", "nowrap");
  await expect(chip).not.toContainText("Keymaster");
  // Nobody typed @oldbox or @offbox: no line for them.
  await expect(page.locator('.mention-line[data-machine="mch_old"], .mention-line[data-machine="mch_off"]')).toHaveCount(0);
  await chip.click();
  const names = page.locator("[data-mention-names]");
  await expect(names.locator("[data-mention-pick]")).toHaveText(["Keymaster", "deploy approvals workflow", "launch"]);
  await names.locator('[data-mention-pick="deploy approvals workflow"]').click();
  await expect(page.locator("[data-composer]")).toHaveText("@aarons-mac-mini.local/deploy-approvals-workflow ");
  await expect(names).toHaveCount(0);
});
