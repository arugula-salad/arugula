// Empty states that do the next step (Discovery 5), in the shell: the
// bell's empty drawer says what lands there and offers Set up while Claude
// Code's settings lack Arugula's hooks (the daemon's answer is stubbed both
// ways, so nothing touches this box's ~/.claude); the Docs page with no
// Docs offers Start an agent, which opens its dialog; and the activity
// drawer says what it keeps the first time it's open, and not again. The
// platform's empty states are in discovery-empty-platform.spec.ts.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, reset } from "./helpers";

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

test("the bell, with nothing waiting, says what lands there and sets up Claude Code's hooks", async ({ page }) => {
  const posted = await hooksMissing(page);
  await reset(page);
  await page.locator("[data-open-attention]").click();
  const empty = page.locator("[data-attention-drawer] [data-attention-empty]");
  await expect(empty).toContainText("Nothing needs you on any machine.");
  await expect(empty).toContainText("Claude in a terminal? Its questions land here");
  await empty.locator("[data-attention-setup]").click();
  await expect(empty.locator('[data-attention-setup-said="done"]')).toContainText("Installed Arugula's hooks");
  await expect(empty.locator("[data-attention-setup]")).toHaveCount(0);
  expect(posted).toEqual(["POST"]);
});

test("the bell offers no Set up where the hooks are in", async ({ page }) => {
  await page.route(/\/api\/setup\?part=agents$/, (r) => r.fulfill({ json: { claude: { installed: true, tools: true, hooks: true }, adapters: [] } }));
  await reset(page);
  await page.locator("[data-open-attention]").click();
  await expect(page.locator("[data-attention-empty]")).toBeVisible();
  // Long enough for the daemon's answer to have come.
  await page.waitForTimeout(500);
  await expect(page.locator("[data-attention-setup]")).toHaveCount(0);
});

test("the Docs page with no Docs offers Start an agent, which opens its dialog", async ({ page }) => {
  await page.route(/\/api\/docs$/, (r) => (r.request().method() === "GET" ? r.fulfill({ json: [] }) : r.fallback()));
  await reset(page);
  await page.locator(".ws-rail [data-open-docs]").click();
  const empty = page.locator("[data-docs-page] [data-docs-empty]");
  await expect(empty).toContainText("Ask any agent for a Doc");
  await empty.locator("[data-docs-start-agent]").click();
  await expect(page.locator("[data-docs-page]")).toHaveCount(0);
  await expect(page.locator(".agent-dialog")).toBeVisible();
});

test("the activity drawer says what it keeps the first time it's open, and not again", async ({ page }) => {
  await page.addInitScript(() => {
    try {
      localStorage.removeItem("arugula.workspace.activity");
    } catch {
      // No storage: the drawer starts closed anyway.
    }
  });
  await reset(page);
  const drawer = page.locator("[data-activity-drawer]");
  await page.locator("[data-activity-toggle]").click();
  await expect(drawer).toHaveClass(/open/);
  await expect(drawer.locator("[data-activity-intro]")).toContainText("Activity keeps what happened in this session");
  // Seen when shown: this browser's (a daemon's page has no account).
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}")["seen:activity-intro"])).toBeTruthy();
  await drawer.locator(".ws-activity-title").click();
  await expect(drawer).not.toHaveClass(/open/);
  await drawer.locator(".ws-activity-title").click();
  await expect(drawer).toHaveClass(/open/);
  await expect(drawer.locator("[data-activity-intro]")).toHaveCount(0);
  // Nor after a reload (which starts the drawer closed, as the init script says).
  await page.reload();
  await page.locator("[data-activity-toggle]").click();
  await expect(page.locator("[data-activity-drawer]")).toHaveClass(/open/);
  await expect(page.locator("[data-activity-intro]")).toHaveCount(0);
});
