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

import { devices, expect, test, type Page } from "@playwright/test";
import { closeContexts, closeSettingsPage } from "./helpers";
import { TeamControl, home, show } from "./team-fixture";

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
  await alice.locator("[data-open-chat]").click();
  const page = alice.locator("[data-chat-control]");
  await expect(page).toBeVisible({ timeout: 15_000 });
  await page.locator(`[data-room-new="${team}/channels"]`).click();
  await alice.locator("[data-new-channel-name]").fill("general");
  await alice.locator("[data-new-channel-name]").press("Enter");
  await expect(page.locator("[data-room-view] .chat-title")).toContainText("general");
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/chat\/ch_[0-9a-f]+-general$/);
  const box = page.locator(".thread-compose textarea");
  await box.fill("@bob the platform page is up");
  await box.press("Enter");
  await expect(page.locator(".thread-msg").last()).toContainText("the platform page is up");

  await platform(bob, "/chat");
  const bobs = bob.locator("[data-chat-control]");
  // A public channel is joined from the browser (#118), and its history is there.
  await bobs.locator(`[data-room-browse="${team}/channels"]`).click();
  await bob.locator('[data-browse-channel="general"] [data-browse-join]').click();
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

test("on a phone, Back climbs chat one level at a time: thread, room, list (#817)", async () => {
  test.setTimeout(120_000);
  const { viewport } = devices["Pixel 7"];
  // Alice's own page at a Pixel 7's size (a second sign-in as her stalls).
  const before = alice.viewportSize()!;
  await alice.setViewportSize(viewport);
  const phone = alice;
  // The seeding is control's page's (the platform page has no test handle).
  await home(alice);
  await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
  const ids = await alice.evaluate(async (tm) => {
    const c = window.__arugula.chat;
    const general = c.rooms.find((r) => r.name === "general")!.id;
    const random = (await c.newChannel(tm, "random")).id;
    const parent = (await c.post(general, "a thread starts here")).seq;
    await c.reply(general, parent, "and goes on", false);
    return { general, random, parent };
  }, team);
  const path = () => new URL(phone.url()).pathname;
  const room = (id: string) => new RegExp(`/chat/${id}-[a-z]+$`);
  await platform(phone, "/chat");
  await phone.locator(`[data-chat-room="${ids.general}"]`).click();
  await expect.poll(path).toMatch(room(ids.general));
  // Sideways, to the other room and back: each replaces the room's entry, it adds none.
  // (The phone shows no list beside a room, so the rooms are clicked at a desktop's width.)
  await phone.setViewportSize(before);
  await phone.locator(`[data-chat-room="${ids.random}"]`).click();
  await expect.poll(path).toMatch(room(ids.random));
  await phone.locator(`[data-chat-room="${ids.general}"]`).click();
  await expect.poll(path).toMatch(room(ids.general));
  await phone.setViewportSize(viewport);
  // Down: its thread pushes.
  await phone.locator(`[data-thread-footer="${ids.parent}"]`).click();
  await expect(phone).toHaveURL(new RegExp(`/chat/${ids.general}-general\\?.*thread=${ids.parent}`));
  const threadUrl = phone.url();
  await phone.goBack();
  await expect.poll(path).toMatch(room(ids.general));
  await phone.goBack();
  await expect.poll(path).toMatch(/\/chat$/);
  await phone.screenshot({ path: "/tmp/claude-arugula-817/list.png" });

  // A thread opened cold (a notification): the room and the list are put under it.
  await phone.goto(threadUrl);
  await expect(phone.getByText("and goes on")).toBeVisible({ timeout: 15_000 });
  await phone.goBack();
  await expect.poll(path).toMatch(room(ids.general));
  await expect.poll(() => new URL(phone.url()).search).toBe("");
  await phone.goBack();
  await expect.poll(path).toMatch(/\/chat$/);
  await phone.setViewportSize(before);
});

test("on a phone, a swipe right goes up a chat level and a swipe left back down (#816)", async () => {
  test.setTimeout(120_000);
  const { viewport } = devices["Pixel 7"];
  // A page of its own, so the history under chat is just this test's.
  const phone = await alice.context().newPage();
  await phone.setViewportSize(viewport);
  await home(alice);
  await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
  const ids = await alice.evaluate(async () => {
    const c = window.__arugula.chat;
    const general = c.rooms.find((r) => r.name === "general")!.id;
    const parent = (await c.post(general, "a swipe thread starts here")).seq;
    await c.reply(general, parent, "and swipes on", false);
    return { general, parent };
  });
  const path = () => new URL(phone.url()).pathname;
  const search = () => new URL(phone.url()).search;
  const room = new RegExp(`/chat/${ids.general}-general$`);
  await platform(phone, "/chat");
  await phone.locator(`[data-chat-room="${ids.general}"]`).click();
  await expect.poll(path).toMatch(room);
  await phone.locator(`[data-thread-footer="${ids.parent}"]`).click();
  await expect.poll(search).toContain(`thread=${ids.parent}`);

  // Real touches (CDP), as phone.spec.ts's pane swipe.
  const cdp = await phone.context().newCDPSession(phone);
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true });
  let x0 = 60;
  const y0 = 400;
  const touch = (type: string, dx: number, dy = 0) =>
    cdp.send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x: x0 + dx, y: y0 + dy }] });
  const swipe = async (dx: number, dy = 0, steps = 8) => {
    await touch("touchStart", 0);
    for (let i = 1; i <= steps; i++) await touch("touchMove", (dx * i) / steps, (dy * i) / steps);
    await touch("touchEnd", dx, dy);
  };

  // A thread, swiped right: its room, the way Back goes (the entry the thread pushed is popped).
  await swipe(300);
  await expect.poll(search).toBe("");
  await expect.poll(path).toMatch(room);
  await phone.goBack();
  await expect.poll(path).toMatch(/\/chat$/);
  await phone.locator(`[data-chat-room="${ids.general}"]`).click();
  await expect.poll(path).toMatch(room);
  await expect(phone.locator(".thread-compose textarea")).toBeVisible();

  // A vertical drag on the messages doesn't navigate.
  await swipe(0, -250);
  await phone.waitForTimeout(400);
  expect(path()).toMatch(room);

  // A room, swiped right: the list. Part way, the list shows under the room.
  await touch("touchStart", 0);
  for (let i = 1; i <= 4; i++) await touch("touchMove", 30 * i);
  await expect(phone.locator('[data-chat-beneath="list"] .chat-list')).toBeVisible();
  await phone.screenshot({ path: "/tmp/claude-arugula-816/mid-swipe.png" });
  for (let i = 5; i <= 9; i++) await touch("touchMove", 30 * i);
  await touch("touchEnd", 270);
  await expect.poll(path).toMatch(/\/chat$/);
  await expect(phone.locator("[data-chat-beneath]")).toHaveCount(0);

  // The list, swiped left: back into the room it had open.
  x0 = 380;
  await swipe(-300);
  await expect.poll(path).toMatch(room);
  await phone.goBack();
  await expect.poll(path).toMatch(/\/chat$/);
  // The list is one entry, not two: what's under it isn't a chat page (a
  // cross-origin entry, from the sign-in hop, reads as no url), so Back
  // leaves chat in one press.
  const under = await phone.evaluate(() => {
    const nav = (window as any).navigation;
    return nav.entries()[nav.currentEntry.index - 1]?.url ?? null;
  });
  expect(under ?? "").not.toMatch(/\/chat/);
  await phone.close();
});

test("Settings is one page across both origins: each index links to the other's sections", async () => {
  await t.addMachine(alice, "settingsbox");
  await home(alice);
  await show(alice, "settingsbox");
  await alice.bringToFront();
  // The machine menu has one row to the page, not the rows themselves.
  await alice.getByTitle("Hosts").click();
  await expect(alice.getByRole("menuitem", { name: "Developer settings…" })).toHaveCount(0);
  await alice.getByRole("menuitem", { name: "Machine settings…" }).click();
  await expect(alice).toHaveURL(/#settings=machine$/);
  await closeSettingsPage(alice);
  // The user menu is who you are: Settings goes to the page.
  await alice.locator("[data-account-menu]").first().click();
  await expect(alice.getByRole("menuitem", { name: "Appearance…" })).toHaveCount(0);
  await expect(alice.getByRole("menuitem", { name: "Teams…" })).toBeVisible();
  await alice.getByRole("menuitem", { name: "Settings", exact: true }).click();
  const shell = alice.locator("[data-settings-page]");
  await expect(shell.locator('[data-settings-section="account"]').getByRole("button", { name: "Teams…" })).toBeVisible();
  await expect(shell.locator("[data-settings-index-elsewhere] a, [data-settings-elsewhere] a").filter({ hasText: "Your agent tokens" }).first()).toHaveAttribute("href", `${app}/settings#tokens`);
  await closeSettingsPage(alice);

  // And back the other way, from `/settings`.
  const page = await alice.context().newPage();
  await page.goto(`${app}/settings`);
  await expect(page.locator("section[data-platform-settings]")).toBeVisible({ timeout: 15_000 });
  const links = page.locator("[data-settings-index-elsewhere] a, [data-settings-elsewhere] a").filter({ hasText: "This device" }).first();
  await expect(links).toHaveAttribute("href", /\/#settings=device$/);
  await page.close();
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
