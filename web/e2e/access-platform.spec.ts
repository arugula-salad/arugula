// A project's Access view (Arugula #682) on the platform page: the first
// section of a project's settings, for every member. Alice owns team
// Acme; Bob is a viewer in it; Alice's machine geek joins the team and she
// gives its agents issues.write over the project. Both of them read the
// same Access section: the roles (Alice an owner, Bob a viewer), the one
// power (Alice's agent on geek, issues.write), what an owner allowed
// (nothing yet) and no cloud workspaces, each part saying whose word it
// is, and no button to change any of it. A platform from before the route
// answers 404, and the section says it's older than the page. A local
// control (team-fixture.ts, `--platform-host app.localhost`), as
// boards-platform.spec.ts runs it.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("access");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let team = "";
let app = "";
let project = "";

async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
    },
    [method, path, body] as const,
  );
}

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

test("Alice and Bob read the same Access section of the project's settings", async ({ browser }) => {
  test.setTimeout(240_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  // Bob, a viewer.
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "viewer", true), team);
  bob = await t.person(browser, "bob");
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  // Alice's machine geek, in the team.
  await t.addMachine(alice, "geek", team);

  await platform(alice);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  // geek's agents may write the project's issues: a power, given by Alice.
  const mine = await api(alice, "GET", "/api/grants");
  expect(mine.status).toBe(200);
  const geek = (mine.body!.grants as { subject: { id: string; name: string } }[]).find((g) => g.subject.name === "geek")!.subject.id;
  const given = await api(alice, "POST", "/api/grants", { daemon: geek, power: "issues.write", scope: { kind: "project", id: project } });
  expect(given.status).toBe(200);

  // The route: both members, the same answer; nothing of anyone's account.
  const view = await api(alice, "GET", `/api/projects/${project}/grants`);
  expect(view.status).toBe(200);
  expect(view.body!.role).toBe("owner");
  expect((view.body!.roles.members as { name: string; role: string }[]).map((m) => `${m.name}:${m.role}`).sort()).toEqual(["alice:owner", "bob:viewer"]);
  expect((view.body!.powers.grants as { power: string; reaches: boolean; subject: { name: string } }[]).map((g) => [g.subject.name, g.power, g.reaches])).toEqual([["geek", "issues.write", true]]);
  expect(JSON.stringify(view.body)).not.toContain("cloud.drive");

  for (const [who, page] of [
    ["Alice", alice],
    ["Bob", bob],
  ] as const) {
    await platform(page, `/p/${project}/settings`);
    const access = page.locator("[data-settings-section=access]");
    await expect(access, who).toHaveAttribute("data-access", "yes", { timeout: 15_000 });
    // First on the page, for everyone.
    await expect(page.locator(".settings-sec").first()).toHaveAttribute("id", "access");
    await expect(access.locator("[data-access-role]")).toHaveCount(2);
    await expect(access.locator("[data-access-roles]")).toContainText("alice");
    await expect(access.locator("[data-access-roles]")).toContainText("Owner");
    await expect(access.locator("[data-access-roles]")).toContainText("Viewer");
    await expect(access.locator(".settings-note").first()).toContainText("roster");
    const power = access.locator("[data-access-power]");
    await expect(power).toHaveCount(1);
    await expect(power).toContainText("alice's agent on geek");
    await expect(power).toContainText("issues.write");
    await expect(power.locator("[data-access-idle]")).toHaveCount(0);
    await expect(access.locator("[data-access-delegation]")).toHaveAttribute("data-access-delegation", "no");
    await expect(access.locator("[data-access-run-grant]")).toHaveAttribute("data-access-run-grant", "no");
    await expect(access.locator("[data-access-workspaces-none]")).toContainText("editors have full use");
    // Read-only: no button, switch or form in it.
    await expect(access.locator("button, input, select, form")).toHaveCount(0);
  }

  // Bob as a viewer sees it (the same section, the same rows), and the
  // route says so too.
  const asBob = await api(bob, "GET", `/api/projects/${project}/grants`);
  expect(asBob.status).toBe(200);
  expect(asBob.body!.role).toBe("viewer");
  expect(asBob.body!.powers.grants.length).toBe(1);

  // A revoke goes at once, and the section follows on reload.
  const revoked = await api(alice, "POST", `/api/grants/${given.body!.id}/revoke`);
  expect(revoked.status).toBe(200);
  await platform(bob, `/p/${project}/settings`);
  await expect(bob.locator("[data-settings-section=access] [data-access-powers-none]")).toBeVisible({ timeout: 15_000 });
});

test("a platform from before the view: the section says it's older than the page", async () => {
  await alice.route(new RegExp(`/api/projects/${project}/grants$`), (route) => route.fulfill({ status: 404, json: { error: "not found" } }));
  await platform(alice, `/p/${project}/settings`);
  const access = alice.locator("[data-settings-section=access]");
  await expect(access).toHaveAttribute("data-access", "old", { timeout: 15_000 });
  await expect(access.locator("[data-access-old]")).toContainText("older than this page");
  await expect(access.locator(".control-error")).toHaveCount(0);
  await alice.unroute(new RegExp(`/api/projects/${project}/grants$`));
});
