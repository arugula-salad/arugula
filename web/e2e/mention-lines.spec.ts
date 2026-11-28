// A post that names two sessions of one machine gets a row for each in its
// icon's card (#1560), as control keeps a status per (machine, session)
// (#875): on a fixture page with no daemon (mention-lines-page.tsx).
import { expect, test } from "@playwright/test";
import { fixturePage } from "./fixture-page";

test("a line per session a post names", async ({ page }) => {
  const body = await fixturePage("./mention-lines-page.tsx");
  await page.route("**/mention-lines.html*", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto("/mention-lines.html");
  // One icon (#1560): a count, the worst state's tone, and a card on a click.
  const icon = page.locator(".receipt-icon");
  await expect(icon).toHaveAttribute("data-receipt", "7");
  await expect(icon).toHaveAttribute("data-tone", "warn");
  await icon.click();
  const lines = page.locator("[data-receipt-card] .mention-line");
  await expect(lines).toHaveText([
    // By session, never `machine/…` (Arugula #1582): the machine is the name's hover.
    "chat working…", "review sending…", "build answered ↗", "launch lead answered ↗", "air offline or didn't answer",
    "box not delivered: its owner turned agent mentions off",
    /^Aarons-Mac-mini\.local no session named v1-engineering-done-board/,
  ]);
  await expect(page.locator('.mention-line[data-machine="mch_geek"]')).toHaveCount(2);
  await expect(page.locator('.mention-line[data-machine="mch_geek"] b').first()).toHaveAttribute("title", "on geek");
  await expect(page.locator('.mention-line[data-machine="mch_geek"][data-session="review"]')).toHaveAttribute("data-mention-status", "sending");
});

test("a no-such-session row lists its names, and a pick puts @machine/session in the composer (#1167)", async ({ page }) => {
  const body = await fixturePage("./mention-lines-page.tsx");
  await page.route("**/mention-lines.html*", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto("/mention-lines.html");
  // A tap opens the card on a phone.
  await page.locator(".receipt-icon").click();
  // Nobody typed @oldbox or @offbox: no line for them.
  await expect(page.locator(".mention-line")).toHaveCount(7);
  await expect(page.locator('.mention-line[data-machine="mch_old"], .mention-line[data-machine="mch_off"]')).toHaveCount(0);
  const names = page.locator("[data-mention-names]");
  await expect(names.locator("[data-mention-pick]")).toHaveText(["Keymaster", "deploy approvals workflow", "launch"]);
  await names.locator('[data-mention-pick="deploy approvals workflow"]').click();
  await expect(page.locator("[data-composer]")).toHaveText("@aarons-mac-mini.local/deploy-approvals-workflow ");
  await expect(names).toHaveCount(0);
});
