// Empty states that do the next step (Discovery 5), in the shell: the
// attention drawer, empty (from Go to…), says what lands there and offers Set up while Claude
// Code's settings lack Arugula's hooks (the daemon's answer is stubbed both
// ways, so nothing touches this box's ~/.claude). The platform's empty
// states are in discovery-empty-platform.spec.ts.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, openAttentionDrawer, reset } from "./helpers";

test.afterAll(closeContexts);
test.use({ viewport: { width: 1440, height: 900 } });

/** The daemon says Claude Code is here without Arugula's hooks, and Set up
 * works: what `/api/setup?part=agents` and its POST would say. */
async function hooksMissing(page: Page): Promise<string[]> {
  const posted: string[] = [];
  await page.route(/\/api\/setup\?part=agents$/, (r) =>
    r.fulfill({ json: { claude: { installed: true, tools: true, hooks: false }, adapters: [] } }),
  );
  await page.route(/\/api\/setup\/agents\/claude$/, (r) => {
    posted.push(r.request().method());
    return r.fulfill({ json: { ok: true, done: ["Installed Arugula's hooks in Claude Code's settings."] } });
  });
  return posted;
}

test("the attention drawer, with nothing waiting, says what lands there and sets up Claude Code's hooks", async ({ page }) => {
  const posted = await hooksMissing(page);
  await reset(page);
  await openAttentionDrawer(page);
  const empty = page.locator("[data-attention-drawer] [data-attention-empty]");
  await expect(empty).toContainText("Nothing needs you on any machine.");
  await expect(empty).toContainText("Claude in a terminal? Its questions land here");
  await empty.locator("[data-attention-setup]").click();
  await expect(empty.locator('[data-attention-setup-said="done"]')).toContainText("Installed Arugula's hooks");
  await expect(empty.locator("[data-attention-setup]")).toHaveCount(0);
  expect(posted).toEqual(["POST"]);
});

test("the attention drawer offers no Set up where the hooks are in", async ({ page }) => {
  await page.route(/\/api\/setup\?part=agents$/, (r) => r.fulfill({ json: { claude: { installed: true, tools: true, hooks: true }, adapters: [] } }));
  await reset(page);
  await openAttentionDrawer(page);
  await expect(page.locator("[data-attention-empty]")).toBeVisible();
  // Long enough for the daemon's answer to have come.
  await page.waitForTimeout(500);
  await expect(page.locator("[data-attention-setup]")).toHaveCount(0);
});
