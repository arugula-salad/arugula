// DK7 (#388): making someone a team's owner, or removing an owner, takes
// the owner's passkey over the new roster version. On the Teams panel an
// owner with no passkey is told so and offered one; with one, the role
// picker asks in a dialog, and the browser's passkey sheet (a virtual
// authenticator here) confirms it. Control on localhost: WebAuthn needs a
// name. Chromium only (the authenticator is CDP's).

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, controlPanel, withPasskey } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("owners", { localhost: true });

test.describe.configure({ mode: "serial" });
test.beforeAll(() => t.start());
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const roles = (p: Page) => p.evaluate(() => window.__arugula.control!.teams[0].roster.members.map((m) => `${m.name}:${m.role}`));

test("an owner confirms a new owner with their passkey", async ({ browser }) => {
  test.setTimeout(120_000);
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor"), team);
  const bob = await t.person(browser, "bob");
  const seed = link.split("#pinvite=")[1].split(".")[1];
  await bob.evaluate(([tm, s]) => window.__arugula.control!.redeem(tm, s), [team, seed]);
  await alice.evaluate(() => window.__arugula.control!.refresh());

  // No passkey yet: the panel says why it matters, and the dialog can't go.
  await controlPanel(alice, "teams");
  await expect(alice.locator("[data-owners-need-passkey]")).toContainText("takes your passkey");
  await alice.getByRole("combobox", { name: "bob's role" }).selectOption("owner");
  await expect(alice.getByRole("heading", { name: "Make bob an owner of Acme?" })).toBeVisible();
  await expect(alice.locator("[data-owner-change-go]")).toBeDisabled();
  await alice.getByRole("button", { name: "Cancel" }).click();
  expect(await roles(alice)).toEqual(["alice:owner", "bob:editor"]);
  // Nor does the change go through without one, asked for directly.
  const refused = await alice.evaluate(async (tm) => {
    const c = window.__arugula.control!;
    return c.changeTeam(tm, (ms) => ms.map((m) => (m.name === "bob" ? { ...m, role: "owner" } : m))).then(
      () => "",
      (e: Error) => e.message,
    );
  }, team);
  expect(refused).toContain("takes your passkey");

  // With one: confirmed in the dialog, and Bob is an owner.
  await withPasskey(alice);
  await expect(alice.locator("[data-owners-need-passkey]")).toHaveCount(0);
  await alice.getByRole("combobox", { name: "bob's role" }).selectOption("owner");
  await alice.locator("[data-owner-change-go]").click();
  await expect(alice.getByRole("heading", { name: "Make bob an owner of Acme?" })).toHaveCount(0);
  await expect.poll(() => roles(alice)).toEqual(["alice:owner", "bob:owner"]);
  await bob.evaluate(() => window.__arugula.control!.refresh());
  expect(await roles(bob)).toEqual(["alice:owner", "bob:owner"]);
});
