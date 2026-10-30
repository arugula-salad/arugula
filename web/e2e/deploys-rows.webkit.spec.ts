// A project's Deploys page, in WebKit (#373): a click on a row opens that
// row's deploy, not the last one's (a stretched link over a positioned `tr`
// covered the whole page there, and Chromium honours it, so only WebKit
// shows it). And a deploy's page goes back to Deploys.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

// The fake driver is the platform's only when it's asked for (testnet's switch).
process.env.ARUGULA_PLATFORM_FAKE_DRIVER = "1";

// Playwright's WebKit stops loading the page once the service worker is
// registered (real Safari doesn't), so none here.
const t = new TeamControl("deploys-rows");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

/** A request to the platform's API, from its page (the person's session). */
const api = (p: Page, method: string, path: string, body?: unknown) =>
  p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, headers: { "content-type": "application/json" }, body: b ? JSON.stringify(b) : undefined });
      return { status: r.status, body: await r.json().catch(() => null) };
    },
    [method, path, body] as const,
  );

test("a click on a row opens its deploy, and the deploy goes back to Deploys", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost", "--hosted-accounts", "*"]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice", { serviceWorkers: "block" });
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  await alice.goto(app);
  await expect(alice.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  // Three deploys of one environment, so a row that isn't the last has others after it.
  const env = await api(alice, "POST", `/api/projects/${project}/environments`, { name: "production", driver: "fake" });
  expect(env.status, JSON.stringify(env.body)).toBe(200);
  const ids: string[] = [];
  for (const c of ["a", "b", "c"]) {
    const d = await api(alice, "POST", `/api/projects/${project}/deploys`, {
      environment: env.body.id,
      source: { kind: "commit", ref: c.repeat(40), where: "acme/app" },
    });
    expect(d.status, JSON.stringify(d.body)).toBe(200);
    ids.push(d.body.id);
  }

  await alice.locator("[data-platform-deploys]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/deploys$/);
  const rows = alice.locator("[data-deploys-list] [data-deploy-row]");
  await expect(rows).toHaveCount(3);
  const list = alice.url();

  // The first row's environment cell (not its commit link), then the middle row's state.
  const first = await rows.first().getAttribute("data-deploy-row");
  await rows.first().locator("td").nth(1).click();
  await expect(alice).toHaveURL(new RegExp(`/deploys/${first}$`));
  await expect(alice.locator(`[data-deploy-page="${first}"]`)).toBeVisible();

  // The back link says Deploys and goes to the project's.
  const back = alice.locator(".deploy-page a.page-back");
  await expect(back).toHaveText(/Deploys/);
  await back.click();
  await expect(alice).toHaveURL(list);

  const middle = await rows.nth(1).getAttribute("data-deploy-row");
  expect(middle).not.toBe(first);
  await rows.nth(1).locator("td").first().click();
  await expect(alice).toHaveURL(new RegExp(`/deploys/${middle}$`));
});
