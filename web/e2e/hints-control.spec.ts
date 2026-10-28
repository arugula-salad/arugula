// Hints from facts, batch 2 (DECISIONS.md "Hints come from facts, once per
// account"; hints.spec.ts has the first), on control's page, with a
// machine shown: push turned on through control says once that the
// account chooses what it's told about (chat, machines, projects), and a
// GitHub-only account on a device that can make a passkey is told it can
// add one. Control is on localhost, where WebAuthn makes passkeys. Seen is
// the account's here (control's /api/me/prefs).
// Hints keep quiet under a test runner unless a spec asks: this one does.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem } from "./helpers";
import { FakePush } from "./phones";
import { TeamControl, home, show } from "./team-fixture";

const t = new TeamControl("hints-control", { localhost: true });
let push: FakePush;

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  push = await FakePush.start();
  await t.start(["--push-host", new URL(push.origin).host]);
});

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
  push?.close();
});

/** Hints on, with the account having seen `ids` already. */
async function hints(page: Page, ids: string[]) {
  await page.addInitScript(() => Object.assign(window, { __arugulaHints: true }));
  await page.evaluate(
    (ids) => fetch("/api/me/prefs", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(Object.fromEntries(ids.map((id) => [`seen:${id}`, 1]))) }),
    ids,
  );
}

test("push on through control: Choose what to be told about", async ({ browser }) => {
  test.setTimeout(180_000);
  const alice = await t.person(browser, "alice");
  await alice.context().grantPermissions(["notifications"]);
  await push.stub(alice, "alice");
  await hints(alice, ["getting-started", "command-mark", "add-passkey"]);
  await t.addMachine(alice, "pushbox");
  await home(alice);
  await show(alice, "pushbox");
  const toast = alice.locator('.hint-toast[data-hint="push-kinds"]');
  await alice.waitForTimeout(1000);
  await expect(toast).toHaveCount(0);

  await alice.locator("[data-account-menu]").click();
  await (await menuItem(alice, "Notify this device")).click();
  await expect(toast).toContainText("Choose what to be told about: chat, machines, projects");
  await toast.locator("[data-hint-act]").click();
  await expect(alice.getByRole("menu")).toContainText("Push: Chat");
  await expect(alice.getByRole("menu")).toContainText("Push: Projects");
  await expect(alice.locator("[data-hint]")).toHaveCount(0);
});

test("a GitHub-only account on a device that makes passkeys: Add a passkey", async ({ browser }) => {
  test.setTimeout(180_000);
  const bob = await t.person(browser, "bob");
  // Touch ID's stand-in: this device can make one.
  const cdp = await bob.context().newCDPSession(bob);
  await cdp.send("WebAuthn.enable");
  await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true, hasUserVerification: true, isUserVerified: true },
  });
  await hints(bob, ["getting-started", "command-mark"]);
  await t.addMachine(bob, "keybox");
  await home(bob);
  await show(bob, "keybox");
  expect(await bob.evaluate(() => window.__arugula.control!.passkeys)).toBe(0);

  const toast = bob.locator('.hint-toast[data-hint="add-passkey"]');
  await expect(toast).toContainText("Add a passkey: sign in without GitHub");
  await toast.locator("[data-hint-act]").click();
  await expect(bob.getByRole("heading", { name: "Sign-in and account" })).toBeVisible();
  await expect(bob.locator("[data-add-passkey]").first()).toBeVisible();
  await expect(bob.locator("[data-hint]")).toHaveCount(0);
});
