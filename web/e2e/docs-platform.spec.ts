// Project Docs on the platform page (aru-118): a project's Docs, one Doc
// drawn by the Doc block's renderer (a chart fence draws a chart), and who
// may write. Alice owns team Acme, with chat's Labs flag on (native Docs
// come with it); Bob is a viewer. Alice makes a Doc from the project's
// Docs and writes a section; Bob reads it on a phone with no form, sees
// Alice's next section arrive without a reload, and finds it on the
// project's home. A local control (team-fixture.ts, `--platform-host
// app.localhost`), as platform.spec.ts runs it.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

const t = new TeamControl("docs");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let app = "";
let docPath = "";

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

const CHART = '```chart\n{"type": "bar", "x": "week", "y": "shipped", "rows": [{"week": "w1", "shipped": 3}, {"week": "w2", "shipped": 5}]}\n```';

test("an owner makes a Doc from the project's Docs and writes it", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "viewer", true), team);
  bob = await t.person(browser, "bob", { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);

  await platform(alice);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice.locator("[data-project-home]")).toContainText("No Docs yet", { timeout: 15_000 });
  await alice.locator("[data-project-docs-link]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/docs$/);
  await expect(alice.locator("[data-docs-none]")).toBeVisible();

  await alice.locator("[data-doc-new-title]").fill("Release plan");
  await alice.locator("[data-doc-new]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/docs\/pdc_[0-9a-f]{32}-release-plan$/, { timeout: 15_000 });
  docPath = new URL(alice.url()).pathname;
  const page = alice.locator("[data-doc-page]");
  await expect(page.locator(".doc-title")).toHaveText("Release plan");
  await expect(page).toHaveAttribute("data-writer", "true");
  await expect(page.locator("[data-doc-maker]")).toContainText("made by you");

  // A first section, with a chart.
  await page.locator("[data-doc-heading-input]").fill("Goals");
  await page.locator("[data-doc-markdown-input]").fill(`Ship **0.64** this week.\n\n${CHART}`);
  await page.locator("[data-doc-save]").click();
  const goals = page.locator(".doc-section[data-section=s1]");
  await expect(goals.locator(".doc-heading")).toHaveText("Goals");
  await expect(goals.locator(".doc-body strong")).toHaveText("0.64");
  await expect(goals.locator(".chart-box svg")).toBeVisible();
  await expect(page.locator("[data-doc-progress]")).toHaveText("1 of 1 sections written");
});

test("a viewer reads it on a phone, with no form, and has the next section live", async () => {
  test.setTimeout(90_000);
  await home(bob);
  await platform(bob, docPath);
  const page = bob.locator("[data-doc-page]");
  await expect(page.locator(".doc-title")).toHaveText("Release plan", { timeout: 15_000 });
  await expect(page.locator(".doc-section[data-section=s1] .chart-box svg")).toBeVisible();
  await expect(page).toHaveAttribute("data-writer", "false");
  await expect(page.locator("[data-doc-writer]")).toHaveCount(0);
  // The phone layout: nothing wider than the screen.
  const wide = await bob.evaluate(() => {
    const main = document.querySelector(".platform-main")!;
    return main.scrollWidth - main.clientWidth;
  });
  expect(wide).toBeLessThanOrEqual(0);

  // Alice adds a section; Bob's page has it without a reload.
  const mine = alice.locator("[data-doc-page]");
  await mine.locator("[data-doc-heading-input]").fill("Risks");
  await mine.locator("[data-doc-markdown-input]").fill("- the canary\n- the weekend");
  await mine.locator("[data-doc-save]").click();
  await expect(mine.locator(".doc-section[data-section=s2] li")).toHaveCount(2);
  await expect(page.locator(".doc-section[data-section=s2] .doc-heading")).toHaveText("Risks", { timeout: 15_000 });
  await expect(page.locator(".doc-section[data-section=s2] li")).toHaveText(["the canary", "the weekend"]);

  // The project's home lists it, and its Docs page has no form for him.
  await platform(bob, docPath.replace(/\/docs\/.*$/, ""));
  await expect(bob.locator("[data-project-docs] [data-doc-link]")).toHaveText(["Release plan"], { timeout: 15_000 });
  await platform(bob, docPath.replace(/\/docs\/.*$/, "/docs"));
  await expect(bob.locator("[data-docs] [data-doc-link]")).toHaveText(["Release plan"], { timeout: 15_000 });
  await expect(bob.locator("[data-doc-new]")).toHaveCount(0);
});

test("an owner deletes it", async () => {
  test.setTimeout(60_000);
  const page = alice.locator("[data-doc-page]");
  await page.locator("[data-doc-delete]").click();
  await page.locator("[data-doc-confirm-delete]").click();
  await expect(alice).toHaveURL(/\/docs$/, { timeout: 15_000 });
  await expect(alice.locator("[data-docs-none]")).toBeVisible({ timeout: 15_000 });
});
