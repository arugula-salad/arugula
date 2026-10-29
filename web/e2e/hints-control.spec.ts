// Hints from facts, batch 2 (DECISIONS.md "Hints come from facts, once per
// account"; hints.spec.ts has the first), on control's page, with a
// machine shown: push turned on through control says once that the
// account chooses what it's told about (chat, machines, projects), and a
// GitHub-only account on a device that can make a passkey is told it can
// add one. Control is on localhost, where WebAuthn makes passkeys. Seen is
// the account's here (control's /api/me/prefs): nvim's chips (#1735),
// answered on one device, are never said on another, a key tip answered
// for one nvim neither.
// Hints keep quiet under a test runner unless a spec asks: this one does.

import { expect, test, type Page } from "@playwright/test";
import { mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { closeContexts, menuItem, panes } from "./helpers";
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

test("nvim's chips are the account's: answered on one device, never on another", async ({ browser }) => {
  test.setTimeout(180_000);
  const said = ["getting-started", "command-mark", "add-passkey", "push-device", "claude-conversations", "nvim-help", "nvim-connect"];
  const one = await t.person(browser, "carol");
  await hints(one, said);
  await t.addMachine(one, "vimbox");
  await home(one);
  await show(one, "vimbox");
  await expect.poll(async () => (await panes(one)).length, { timeout: 30_000 }).toBeGreaterThan(0);
  const [term] = await panes(one);
  // nvim with directories of its own, not this machine's config.
  const own = mkdtempSync(join(tmpdir(), "arugula-e2e-hints-control-nvim-"));
  for (const d of ["data", "config", "state"]) mkdirSync(join(own, d));
  const cmd = `XDG_DATA_HOME=${own}/data XDG_CONFIG_HOME=${own}/config XDG_STATE_HOME=${own}/state nvim -n -i NONE\r`;
  await one.evaluate(({ p, cmd }) => window.__arugula.client.input(p, new TextEncoder().encode(cmd)), { p: term, cmd });
  await expect.poll(() => one.evaluate((p) => window.__arugula.client.info(p)?.editor?.pid ?? null, term), { timeout: 30_000 }).not.toBeNull();

  const header = one.locator(`[data-pane-header="${term}"]`);
  await expect(header.locator('[data-hint="nvim-open-here"]')).toBeVisible({ timeout: 15_000 });
  await header.locator('[data-hint="nvim-open-here"] [data-hint-dismiss]').click();
  // Then this nvim's key tip: answered too.
  await expect(header.locator('[data-hint="nvim-key-tip"]')).toBeVisible({ timeout: 15_000 });
  await header.locator('[data-hint="nvim-key-tip"] [data-hint-dismiss]').click();
  await expect(one.locator("[data-hint]")).toHaveCount(0);

  // Another page of the account, nothing of this browser's own kept (a
  // device's seen would be in localStorage), the same nvim still running:
  // neither, since control's page keeps seen on the account.
  await one.evaluate(() => localStorage.removeItem("arugula.seen"));
  const two = await one.context().newPage();
  await two.addInitScript(() => Object.assign(window, { __arugulaHints: true }));
  await home(two);
  await show(two, "vimbox");
  await expect.poll(() => two.evaluate((p) => window.__arugula.client.info(p)?.editor?.pid ?? null, term), { timeout: 30_000 }).not.toBeNull();
  await two.evaluate((p) => window.__arugula.client.setActive(p), term);
  await two.waitForTimeout(2500);
  await expect(two.locator('[data-hint^="nvim-"]')).toHaveCount(0);
  const prefs = await two.evaluate(() => fetch("/api/me/prefs").then((r) => r.json()));
  expect(prefs).toHaveProperty("seen:nvim-open-here");
  expect(prefs["seen:nvim-key-tip"]).toHaveLength(1);
});
