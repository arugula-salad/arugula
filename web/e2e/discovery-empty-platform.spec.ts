// Empty states that do the next step (Discovery 5), on the platform page:
// a new team project's home offers each section's next step as a button,
// and each one does it. Turn on chat opens Teams on control's page, Link
// a repo the repo field, Make an environment the environment form, and
// Create a cloud workspace makes one (a name and a size, as `arugula
// cloud create` does) and opens its page. Then Runs' Start a run opens the
// form, the bar has Workspaces and Deploys, and Deploys with no
// environment offers to make one. A viewer gets Ask an owner, and one line
// saying why they can't start a run. A local control (team-fixture.ts),
// cloud workspaces on for everyone against an E2B nobody answers: the
// workspace is made, and its box never comes.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

const t = new TeamControl("discovery-empty");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let app = "";
let team = "";
let project = "";

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

test("a new project's home: each empty section's button does its step", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost", "--hosted-accounts", "*", "--e2b-api-key", "e2e-nobody", "--e2b-url", "http://127.0.0.1:9"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);

  await platform(alice);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];
  const home$ = alice.url();

  // Chat is off for the team: its owner turns it on, under Teams on Control.
  const turnOn = alice.locator("[data-chat-off] [data-chat-turn-on]");
  await expect(turnOn).toHaveAttribute("href", `${t.base}/#teams`);

  // Link a repo: the settings' repo field, ready to type in.
  await expect(alice.locator("[data-project-repos-none]")).toContainText("pull requests and issues");
  await alice.locator("[data-project-link-repo]").click();
  await expect(alice).toHaveURL(/\/settings#repos$/);
  await expect(alice.locator("[data-link-repo]")).toBeFocused();

  // Make an environment: the form, open.
  await alice.goto(home$);
  await expect(alice.locator("[data-project-envs-none]")).toContainText("where the project deploys to");
  await alice.locator("[data-project-make-env]").click();
  await expect(alice).toHaveURL(/\/settings#environments$/);
  await expect(alice.locator("[data-environment-form]")).toBeVisible();

  // No runs and no cloud workspace to run one on: make one first.
  await alice.goto(home$);
  await expect(alice.locator("[data-project-runs-none] [data-hosted-new]")).toBeVisible();
  await alice.locator("[data-no-workspaces] [data-hosted-new]").click();
  const form = alice.locator("[data-hosted-new-form]");
  await form.locator("[data-hosted-new-name]").fill("bad name");
  await form.locator("[data-hosted-new-create]").click();
  await expect(form.locator("[data-hosted-new-error]")).toContainText("A name");
  await form.locator("[data-hosted-new-name]").fill("dev");
  await form.locator('[data-hosted-new-size="large"]').check();
  await form.locator("[data-hosted-new-create]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/workspaces\/hw_[0-9a-f]+/);
  await expect(alice.locator("[data-hosted-size]")).toHaveAttribute("data-hosted-size", "large");
  await expect(alice.locator("[data-hosted-workspace]")).toContainText("dev");

  // The bar's Workspaces lists it, with Create beside.
  await alice.locator("[data-platform-workspaces]").click();
  await expect(alice.locator("[data-hosted-workspaces-list] [data-hosted-workspace-link]")).toHaveCount(1);
  await expect(alice.locator("[data-hosted-workspaces] [data-hosted-new]")).toBeVisible();

  // Now there's a workspace, Runs' empty state starts one.
  await alice.locator("[data-platform-runs]").click();
  await alice.locator("[data-runs-none-start]").click();
  await expect(alice.locator("[data-run-start-command-mode]")).toBeVisible();
  // And from the home, Start a run comes with the form open.
  await alice.goto(home$);
  await alice.locator("[data-project-start-run]").click();
  await expect(alice).toHaveURL(/\/runs#new-run$/);
  await expect(alice.locator("[data-run-start-command-mode]")).toBeVisible();

  // Deploys has a place in the bar; with no environment, its owner makes one.
  await alice.locator("[data-platform-deploys]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/deploys$/);
  await expect(alice.locator("[data-deploys-none]")).toContainText("A deploy puts a commit");
  await alice.locator("[data-deploys-make-env]").click();
  await expect(alice.locator("[data-environment-form]")).toBeVisible();

  // Turn on chat lands on Teams, on control's page, with the team's switch.
  await alice.goto(home$);
  await turnOn.click();
  await expect(alice.locator(`[data-team-chat="${team}"]`)).toBeVisible({ timeout: 15_000 });
});

test("a viewer: Ask an owner for chat, and why they can't start a run", async ({ browser }) => {
  test.setTimeout(120_000);
  await home(alice);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "viewer", true), team);
  const vic = await t.person(browser, "vic");
  await vic.goto(link);
  await vic.locator("[data-accept-invite]").click();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);

  await platform(vic, `/p/${project}`);
  await vic.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: app });
  const off = vic.locator("[data-chat-off]");
  await expect(off).toContainText("Its owners turn it on");
  await expect(off.locator("[data-chat-turn-on]")).toHaveCount(0);
  await off.locator("[data-chat-ask-owner]").click();
  expect(await vic.evaluate(() => navigator.clipboard.readText())).toContain(`${t.base}/#teams`);
  // A viewer neither links repos nor makes workspaces: they're told who does.
  await expect(vic.locator("[data-project-link-repo]")).toHaveCount(0);
  await expect(vic.locator("[data-project-runs-why]")).toContainText("owners and editors start its runs");

  await vic.locator("[data-platform-runs]").click();
  await expect(vic.locator("[data-runs-none]")).toContainText("owners and editors start its runs");
  await expect(vic.locator("[data-runs-none-start]")).toHaveCount(0);
});
