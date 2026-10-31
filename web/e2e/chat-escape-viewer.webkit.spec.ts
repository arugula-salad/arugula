// Escape closes a full-size picture and the page marks the key handled (#380),
// so the desktop app doesn't take it as leave-full-screen.
// WebKit version (runs on Playwright's WebKit engine).

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, live, show } from "./team-fixture";
import { pixel7 } from "./phones";

const t = new TeamControl("chatc-escape-webkit");
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

test("setup: create #general and send an image", async () => {
  test.setTimeout(90_000);
  const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-chat]").click()]);
  aliceChat = opened;
  await expect(aliceChat).toHaveURL(new RegExp(`^${app}/p/prj_[0-9a-f]+-[a-z]+/chat`), { timeout: 15_000 });
  const page = aliceChat.locator("[data-chat-control]");
  await page.locator(`[data-room-new="${team}/channels"]`).click();
  await aliceChat.locator("[data-new-channel-name]").fill("general");
  await aliceChat.locator("[data-new-channel-name]").press("Enter");
  const view = page.locator("[data-room-view]");
  await expect(view.locator(".chat-title")).toContainText("general");
  general = (await view.getAttribute("data-room-view"))!;
  await aliceChat.evaluate(
    async ([room, who]) => {
      const r = await fetch(`/api/chat/rooms/${room}/members`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ accounts: [who] }) });
      if (!r.ok) throw new Error(`adding: ${r.status}`);
    },
    [general, bobAccount] as const,
  );
  // A 2x2 PNG.
  const PNG_B64 = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAFklEQVR4nGP8z8Dwn4GBgYGJAQoAHhgCAh6X4CYAAAAASUVORK5CYII=";
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const box = room.getByRole("textbox", { name: "Message #general" });
  await box.focus();
  await box.evaluate((el, b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const data = new DataTransfer();
    data.items.add(new File([bytes], "image.png", { type: "image/png" }));
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, PNG_B64);
  const chip = room.locator("[data-attach-chip]");
  await expect(chip).toHaveCount(1);
  await expect(chip.locator("progress")).toHaveCount(0, { timeout: 15_000 });
  await box.fill("the image");
  await box.press("Enter");
  const mine = room.locator(".thread-msg").last();
  await expect(mine.locator("[data-msg-image] img")).toBeVisible({ timeout: 15_000 });
});

test("escape closes the image viewer and is prevented", async () => {
  test.setTimeout(60_000);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const mine = room.locator(".thread-msg").last();
  // Open the viewer.
  await mine.locator("[data-msg-image]").click();
  await expect(aliceChat.locator("[data-image-viewer] img")).toBeVisible();
  await aliceChat.bringToFront();
  // Preact runs the viewer's effect (its keydown listener) a frame after it paints.
  await aliceChat.evaluate(() => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 150))));
  // Record the Escape as the page's handlers left it: a capture listener on
  // window, added after the viewer's, runs after it (stopPropagation doesn't
  // stop listeners on the same target).
  await aliceChat.evaluate(() => {
    (window as unknown as { escapes: boolean[] }).escapes = [];
    addEventListener("keydown", (e) => e.key === "Escape" && (window as unknown as { escapes: boolean[] }).escapes.push(e.defaultPrevented), true);
  });
  await aliceChat.keyboard.press("Escape");
  await expect(aliceChat.locator("[data-image-viewer]")).toHaveCount(0);
  // The viewer took the key (an Escape left unhandled would leave full screen in the desktop app).
  expect(await aliceChat.evaluate(() => (window as unknown as { escapes: boolean[] }).escapes)).toEqual([true]);
});

test("escape cancels a message edit and is prevented", async () => {
  test.setTimeout(60_000);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  // ↑ in an empty composer edits the last message of one's own.
  const box = room.getByRole("textbox", { name: "Message #general" });
  await box.focus();
  await box.press("ArrowUp");
  const edit = room.getByRole("textbox", { name: "Edit message" });
  await expect(edit).toBeVisible();
  // On the box itself, so it runs after the page's own handler (stopPropagation
  // doesn't stop listeners on the same target).
  await edit.evaluate((el) => {
    const seen: boolean[] = ((window as unknown as { edits: boolean[] }).edits = []);
    el.addEventListener("keydown", (e) => e.key === "Escape" && seen.push(e.defaultPrevented));
  });
  await edit.press("Escape");
  await expect(room.locator(".msg-edit")).toHaveCount(0);
  expect(await aliceChat.evaluate(() => (window as unknown as { edits: boolean[] }).edits)).toEqual([true]);
});
