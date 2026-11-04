// Arugula #586: the platform rail's Projects goes to the project last open
// (kept in this browser), not the list. A first visit lands on the list; after
// opening Acme, Projects lands on Acme; once the page's projects no longer
// hold Acme (deleted, or the reader lost it), Projects lands on the list and
// forgets it rather than drawing a "not found" project page.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("activeproject");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let app = "";

const projects = () => alice.locator(".ws-rail [data-open-projects]");

test("a first visit: Projects lands on the list", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  // One visit: the first signs in by a hop through the hub, and a second so soon is refused.
  await alice.goto(`${app}/`);
  await expect(alice.locator("section[data-platform-home] [data-project]")).toHaveCount(2, { timeout: 30_000 });
  await projects().click();
  await expect(alice).toHaveURL(`${app}/`);
  await expect(alice.locator("section[data-platform-home]")).toBeVisible();
});

test("after opening a project, Projects lands on it", async () => {
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).first().click();
  await expect(alice.locator("[data-project-switcher]")).toContainText("Acme");
  const there = new URL(alice.url()).pathname;
  expect(there).not.toBe("/");
  // Away from every project, then back by the rail.
  await alice.locator("[data-open-settings]").click();
  await expect(alice).toHaveURL(`${app}/settings`);
  await projects().click();
  await expect(alice).toHaveURL(`${app}${there}`);
});

test("with the project gone from the reader's list, Projects lands on the list", async () => {
  await alice.route("**/api/projects", async (route) => {
    const res = await route.fetch();
    const body = (await res.json()) as { projects: { name: string }[] };
    await route.fulfill({ response: res, json: { ...body, projects: body.projects.filter((p) => p.name !== "Acme") } });
  });
  await alice.goto(`${app}/settings`);
  await expect(alice.locator("[data-platform-bar]")).toBeVisible({ timeout: 15_000 });
  await projects().click();
  await expect(alice).toHaveURL(`${app}/`);
  await expect(alice.locator("section[data-platform-home]")).toBeVisible();
  expect(await alice.evaluate(() => localStorage.getItem("arugula.platform-active-project"))).toBeNull();
});
