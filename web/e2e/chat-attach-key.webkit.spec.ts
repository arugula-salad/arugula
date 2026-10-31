// ⌘U (Ctrl+U off macOS) opens the file picker of the chat composer that has
// focus (#412): on the platform page's chat and in the team-chat block on the
// panes page. WebKit version. Playwright's `filechooser` event is the picker opening.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";
import { pixel7 } from "./phones";

const t = new TeamControl("chat-attach-key-webkit");
// Playwright's WebKit stops loading the page once the service worker is registered (as in teams-presigned.webkit.spec.ts).
const noSw = { serviceWorkers: "block" } as const;

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let aliceChat: Page;
let bob: Page;
let team = "";
let app = "";
let general = "";
let chatBlock = 0;
let bobAccount = "";

const chatOn = (p: Page) => p.evaluate(() => window.__arugula.chat.on);

test("setup: team with chat on", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice", noSw);
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
  bob = await t.person(browser, "bob", { ...pixel7, ...noSw });
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await expect(bob.locator("[data-invite-pending]")).toBeVisible();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  bobAccount = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "bob")!.account);
  await home(alice);
  await expect.poll(() => chatOn(alice)).toBe(false);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await t.addMachine(alice, "alices");
  for (const p of [alice, bob]) {
    await home(p);
    await expect.poll(() => chatOn(p), { timeout: 15_000 }).toBe(true);
  }
  await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "alices");
});

test("setup: #general, and a team-chat block beside a terminal", async () => {
  test.setTimeout(90_000);
  const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-chat]").click()]);
  aliceChat = opened;
  await expect(aliceChat).toHaveURL(new RegExp(`^${app}/p/prj_[0-9a-f]+-[a-z]+/chat`), { timeout: 15_000 });
  const page = aliceChat.locator("[data-chat-control]");
  await page.locator(`[data-room-new="${team}/channels"]`).click();
  await aliceChat.locator("[data-new-channel-name]").fill("general");
  await aliceChat.locator("[data-new-channel-name]").press("Enter");
  await expect(page.locator("[data-room-view] .chat-title")).toContainText("general");
  general = (await page.locator("[data-room-view]").getAttribute("data-room-view"))!;
  const run = await call(alice, "POST", "/api/run", { command: "sleep 600" });
  expect(run.status, JSON.stringify(run.body)).toBe(200);
  const terminal = (run.body as { pane: number }).pane;
  const block = await call(alice, "POST", "/api/blocks", { type: "team_chat", config: { room: null, pinned: false }, split: terminal, local: true });
  expect(block.status, JSON.stringify(block.body)).toBe(200);
  chatBlock = (block.body as { block: number }).block;
  await alice.bringToFront();
  await alice.evaluate((p) => window.__arugula.client.setActive(p), terminal);
  await expect(alice.locator(`[data-chat-block="${chatBlock}"]`)).toBeVisible({ timeout: 15_000 });
});

/** The key the picker opens on, for the OS the browser runs on. */
const PICKER_KEY = process.platform === "darwin" ? "Meta+U" : "Control+U";

test("the key in the platform page's composer opens its file picker", async () => {
  test.setTimeout(60_000);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  await aliceChat.bringToFront();
  const box = room.getByRole("textbox", { name: "Message #general" });
  // The tooltip says the shortcut.
  await expect(room.locator("[data-attach-button]")).toHaveAttribute("title", process.platform === "darwin" ? "Attach files (⌘U)" : "Attach files (Ctrl+U)");
  await box.focus();
  const [chooser] = await Promise.all([aliceChat.waitForEvent("filechooser", { timeout: 10_000 }), aliceChat.keyboard.press(PICKER_KEY)]);
  expect(chooser.isMultiple()).toBe(true);
});

test("the key in the team-chat block's composer opens its file picker", async () => {
  test.setTimeout(60_000);
  await alice.bringToFront();
  const view = alice.locator(`[data-chat-block="${chatBlock}"]`);
  await view.locator(`[data-chat-room="${general}"]`).click();
  const box = view.locator("[data-room-view]").getByRole("textbox", { name: "Message #general" });
  await box.focus();
  const [chooser] = await Promise.all([alice.waitForEvent("filechooser", { timeout: 10_000 }), alice.keyboard.press(PICKER_KEY)]);
  expect(chooser.isMultiple()).toBe(true);
});

test("outside a chat composer the key is left alone", async () => {
  test.setTimeout(30_000);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  await aliceChat.bringToFront();
  await room.getByRole("textbox", { name: "Message #general" }).blur();
  await aliceChat.evaluate(() => {
    (window as unknown as { u: boolean[] }).u = [];
    addEventListener("keydown", (e) => e.code === "KeyU" && (window as unknown as { u: boolean[] }).u.push(e.defaultPrevented));
  });
  await aliceChat.keyboard.press(PICKER_KEY);
  expect(await aliceChat.evaluate(() => (window as unknown as { u: boolean[] }).u)).toEqual([false]);
});
