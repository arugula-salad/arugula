// Native #245: the Share dialog lists the session's live read-only links,
// and Revoke ends one: its viewer is cut off and the link refuses. The
// access log opens beside them.

import { expect, test } from "@playwright/test";
import { active, ready, reset, run } from "./helpers";

test.use({ viewport: { width: 1440, height: 900 } });

test("a read-only link is listed, and Revoke ends it", async ({ page, context }) => {
  await reset(page);
  const pane = await active(page);
  await ready(page, pane);
  await run(page, pane, "echo linked-$((5*5))", "linked-25");
  const link = await page.evaluate((p) => window.__arugula.client.share(p), pane);
  expect(link).toMatch(/\/share\//);
  // The link is on the machine's tailnet name; this daemon is the one here.
  const url = new URL(link!).pathname;

  const viewer = await context.newPage();
  await viewer.goto(url);
  const shown = () => viewer.evaluate(() => (window as unknown as { __share: { text(): string } }).__share.text());
  const ended = () => viewer.evaluate(() => (window as unknown as { __share: { ended: string | null } }).__share.ended);
  await expect.poll(shown).toContain("linked-25");

  await page.locator("[data-crumb-menu]").click();
  await page.getByRole("menuitem", { name: "Share session…" }).click();
  const dialog = page.locator(".share-dialog");
  const row = dialog.locator(`[data-share-live^="share:"][data-share-live-pane="${pane}"]`);
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("Read-only link");

  await row.locator("[data-revoke]").click();
  await expect(row).toHaveCount(0);
  await expect.poll(ended).toMatch(/expired or was revoked/);
  expect((await page.request.get(url)).status()).toBe(404);

  // The access log: this session's grants and revokes (none here yet).
  await dialog.locator("[data-access-log-open]").click();
  await expect(dialog.locator("[data-access-log], [data-access-log-empty]")).toBeVisible();
  await viewer.close();
});
