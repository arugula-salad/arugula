// The platform page (M150–M153): projects and chat on
// an origin of their own (`--platform-host app.localhost`, which Chrome
// sends to loopback), served through the hub, apart from the shell's keys.
//
// Alice signs in on control's page and opens the platform page: one silent
// hop through the hub (M152) and she's there, as herself, with her
// projects. Her team Acme has chat on; she makes #general on the platform
// page and writes in it, mentioning Bob, who reads it there over the live
// stream and answers. Then she signs out on control's page, and the
// platform page's session ends with it.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

const t = new TeamControl("platform");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let team = "";
let app = "";

/** The platform page at `path`, signed in by the hop if it has to be. */
async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

test("signed in on control's page, the platform page signs in with one silent hop", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  const info = (await (await fetch(`${t.base}/control.json`)).json()) as { platform: string };
  expect(info.platform).toBe(app);
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);

  const hops: string[] = [];
  alice.on("framenavigated", (f) => f === alice.mainFrame() && hops.push(new URL(f.url()).origin));
  await platform(alice);
  // control's /signin/platform, then straight back: no page of control's drawn.
  expect(hops.filter((o) => o === t.base).length).toBeLessThanOrEqual(1);
  await expect(alice.locator("[data-signed-in-as]")).toContainText("alice");
  await expect(alice.locator("section[data-platform-home] [data-project]")).toHaveCount(2);
  await expect(alice.locator("section[data-platform-home]")).toContainText("Acme");
  // The code never stays in the address.
  expect(alice.url()).not.toContain("code=");
  // The page's cookie is its host's alone; the hub's never reaches it.
  const cookies = await alice.context().cookies(app);
  const own = cookies.find((c) => c.name === "arugula_app");
  expect(own && own.httpOnly && own.domain === "app.localhost").toBe(true);
  expect(cookies.some((c) => c.name === "ilg_session")).toBe(false);
});

test("a project's home lists its channels and its team, read only, with the way to change it", async ({ browser }) => {
  await home(alice);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
  bob = await t.person(browser, "bob");
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);

  await platform(alice);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  const members = alice.locator("[data-project-members]");
  await expect(members.locator("[data-member]")).toHaveCount(2, { timeout: 15_000 });
  await expect(members).toContainText("Owner");
  await expect(members.locator("[data-manage-team]")).toHaveAttribute("href", t.base);
  // Only the id is read: an old slug lands, and the address gets the new one.
  const id = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];
  await platform(alice, `/p/${id}-an-old-name`);
  await expect(alice).toHaveURL(new RegExp(`/p/${id}-acme$`));
});

test("chat on the platform page: Alice makes #general and mentions Bob, who answers", async () => {
  test.setTimeout(90_000);
  await alice.locator("[data-platform-chat]").click();
  const page = alice.locator("[data-chat-control]");
  await expect(page).toBeVisible({ timeout: 15_000 });
  await page.locator(`[data-room-new="${team}/channels"]`).click();
  await alice.locator(".prompt input").fill("general");
  await alice.locator(".prompt input").press("Enter");
  await expect(page.locator("[data-room-view] .chat-title")).toContainText("general");
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/chat\/ch_[0-9a-f]+-general$/);
  const box = page.locator(".thread-compose textarea");
  await box.fill("@bob the platform page is up");
  await box.press("Enter");
  await expect(page.locator(".thread-msg").last()).toContainText("the platform page is up");

  await platform(bob, "/chat");
  const bobs = bob.locator("[data-chat-control]");
  await bobs.locator("[data-chat-room]", { hasText: "general" }).click();
  await expect(bobs.locator(".thread-msg").last()).toContainText("the platform page is up");
  await bobs.locator(".thread-compose textarea").fill("seen it, thanks");
  await bobs.locator(".thread-compose textarea").press("Enter");
  // Alice has it over the live stream, with no reload.
  await expect(page.locator(".thread-msg").last()).toContainText("seen it, thanks", { timeout: 15_000 });
  // A message's link is the platform page's, by id.
  await page.locator(".thread-msg").last().hover();
  await alice.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: app });
  await page.locator(".thread-msg").last().locator("[data-msg-more]").click();
  await page.locator(".thread-msg").last().locator("[data-msg-link]").click();
  const copied = await alice.evaluate(() => navigator.clipboard.readText());
  expect(copied).toMatch(new RegExp(`^${app}/p/prj_[0-9a-f]+-acme/chat/ch_[0-9a-f]+-general\\?msg=\\d+$`));
});

test("the account menu's Plan and usage opens the platform page, which draws it", async () => {
  // Billing on (faked here: no Stripe in tests), on both pages.
  const billing = { billing: true, plan: "personal", relay: { bytes: 0, allowance: 1e9, warning: false, slowed: false }, sandbox_minutes: 0, teams: [] };
  await alice.context().route("**/api/billing", (r) => r.fulfill({ json: billing }));
  await home(alice);
  await alice.evaluate(() => window.__arugula.control!.refresh());
  const menu = async (item: string | RegExp) => {
    await alice.bringToFront();
    await alice.locator("[data-account-menu]").first().click();
    await alice.getByRole("menuitem", { name: item }).click();
  };
  const [settings] = await Promise.all([alice.context().waitForEvent("page"), menu(/^Plan and usage/)]);
  await expect(settings).toHaveURL(`${app}/settings`, { timeout: 15_000 });
  await settings.locator("[data-open-plan]").click();
  await expect(settings.getByRole("heading", { name: "Plan and usage" })).toBeVisible();
  await settings.locator(".prompt").getByRole("button", { name: "Done" }).click();
  // The cloud agent library went with Managoat's Cloud workspaces.
  await alice.bringToFront();
  await alice.locator("[data-account-menu]").first().click();
  await expect(alice.getByRole("menuitem", { name: "Cloud agents…" })).toHaveCount(0);
  await alice.keyboard.press("Escape");
  await alice.context().unroute("**/api/billing");
  for (const p of alice.context().pages()) if (p !== alice) await p.close();
});

test("signing out on control's page ends the platform page's session at once", async () => {
  // Control's sign-out, with Alice's cookie, as her page sends it (the
  // page's own sign-out reloads it, under the next step).
  const out = await alice.context().request.post(`${t.base}/auth/logout`, { headers: { origin: t.base } });
  expect(out.ok()).toBe(true);
  expect((await alice.context().request.get(`${app}/api/projects`)).status()).toBe(401);
  // A hop came back moments ago, so the page doesn't go round again by
  // itself: it says to sign in, and that goes through the hub, which has
  // no session now, to control's page.
  await alice.goto(`${app}/settings`);
  await expect(alice.locator("[data-platform-signed-out]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-sign-in]").click();
  await expect(alice).toHaveURL(new RegExp(`^${t.base}/#platform-signin=`), { timeout: 15_000 });
  await expect(alice.locator("[data-signin=github]")).toBeVisible();
  // Signing in there comes back to the page, signed in.
  await alice.locator("[data-signin=github]").click();
  await expect(alice).toHaveURL(new RegExp(`^${app}/settings`), { timeout: 30_000 });
  await expect(alice.locator("[data-signed-in-as]")).toContainText("alice");
});
