// Control's chat (M123, over M122's API) on the platform page (M153, M155:
// the shell draws no chat page of its own): Alice and Bob in team Acme,
// with control's `chat` flag on for the team. Alice has one machine of her
// own (alices), which Bob can't reach; Bob has none, and reads chat on a
// phone.
//
// The rail's Chat on control's page opens the platform page, where Alice
// makes #general and mentions Bob: control pushes him, his page has it over
// the live stream, and a pane's quote chip opens for her and not for him. A
// pane's thread opens beside the pane on control's page, with the lines of
// a pane quoted in it, and only its machine's owner reads it. Then every
// daemon is stopped: they write in #general and a DM, from the web and the
// phone. A push's tap (`#chat=<room>&msg=<n>` on control's page) opens the
// room at the message on the platform page.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, paneEl } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";
import { pixel7 } from "./phones";

const t = new TeamControl("chatc");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
/** Alice's platform page, opened from the rail. */
let aliceChat: Page;
let bob: Page;
let team = "";
let app = "";
let general = "";
let bobAccount = "";
let quotedPane = 0;

const chatOn = (p: Page) => p.evaluate(() => window.__arugula.chat.on);
const roomsOf = (p: Page) => p.evaluate(() => window.__arugula.chat.rooms.map((r) => ({ id: r.id, kind: r.kind, name: r.name, unread: r.unread, mentions: r.mentions })));

/** The platform page at `path`, signed in by the hop if it has to be. */
async function platform(p: Page, path: string) {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-chat-control]")).toBeVisible({ timeout: 15_000 });
}

test("a team with chat on, and a machine of Alice's", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
  bob = await t.person(browser, "bob", pixel7);
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await expect(bob.locator("[data-invite-pending]")).toBeVisible();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  bobAccount = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "bob")!.account);
  // Off until a team (or the account) turns it on: the page asks control.
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
  // Bob has no machine: control's page offers him chat anyway, on the
  // platform page.
  await expect(bob.locator("[data-open-chat]")).toBeVisible();
  // The shell draws no chat page of its own: `#chat` isn't a place of its.
  await expect(alice.locator("[data-chat-control]")).toHaveCount(0);
});

test("the rail's Chat opens the platform page, where Alice makes #general and mentions Bob", async () => {
  test.setTimeout(90_000);
  const run = await call(alice, "POST", "/api/run", { command: "echo MEAN-TEST-FAILED; sleep 600" });
  expect(run.status, JSON.stringify(run.body)).toBe(200);
  quotedPane = (run.body as { pane: number }).pane;
  await expect.poll(() => alice.evaluate((p) => window.__arugula.text(p), quotedPane), { timeout: 15_000 }).toContain("MEAN-TEST-FAILED");

  // A tab of its own, beside the terminals.
  const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-chat]").click()]);
  aliceChat = opened;
  await expect(aliceChat).toHaveURL(new RegExp(`^${app}/p/prj_[0-9a-f]+-[a-z]+/chat`), { timeout: 15_000 });
  const page = aliceChat.locator("[data-chat-control]");
  await page.locator(`[data-room-new="${team}/channels"]`).click();
  await aliceChat.locator(".prompt input").fill("general");
  await aliceChat.locator(".prompt input").press("Enter");
  const view = page.locator("[data-room-view]");
  await expect(view.locator(".chat-title")).toContainText("general");
  general = (await view.getAttribute("data-room-view"))!;
  const box = view.getByRole("textbox", { name: "Message #general" });
  await box.fill("@bob the mean test fails on alices");
  await box.press("Enter");
  await expect(view.locator(".thread-msg").last()).toContainText("the mean test fails");

  // A quote of a pane on alices: the platform page reads no panes, so it's
  // sent from control's page, where they are.
  const machine = await alice.evaluate(() => window.__arugula.fleet.host("alices")!.id!);
  await alice.evaluate(
    ([r, m, p]) => window.__arugula.chat.post(r as string, "the failing line", { machine: m as string, pane: p as number, text: "MEAN-TEST-FAILED" }),
    [general, machine, quotedPane] as const,
  );
  const quoted = view.locator(".thread-msg").last();
  await expect(quoted).toContainText("the failing line", { timeout: 15_000 });
  // Alice's pane: control says it opens for her (on control's page).
  await expect(quoted.locator("[data-pane-chip]")).toHaveAttribute("data-opens", "1");
  await expect(quoted.locator(".thread-quote pre")).toHaveText("MEAN-TEST-FAILED");
});

test("Bob has it live, mentioned; the chip doesn't open for him", async () => {
  await expect.poll(() => roomsOf(bob), { timeout: 15_000 }).toContainEqual(expect.objectContaining({ id: general, unread: 2, mentions: 1 }));
  // A push's tap opens the room (sw.ts makes this address on control's
  // page; it lands on the platform page, M154).
  await bob.goto(`/#chat=${general}`);
  await expect(bob).toHaveURL(new RegExp(`^${app}/p/prj_[0-9a-f]+-acme/chat/${general}-general`), { timeout: 15_000 });
  const page = bob.locator("[data-chat-control]");
  const mention = page.locator(".thread-msg", { hasText: "the mean test fails" });
  await expect(mention).toBeVisible({ timeout: 15_000 });
  await expect(mention).toHaveClass(/for-me/);
  const msg = page.locator(".thread-msg").last();
  await expect(msg.locator("[data-pane-chip]")).toHaveAttribute("data-opens", "0");
  await expect(msg.locator(".thread-quote.inert pre")).toHaveText("MEAN-TEST-FAILED");
  // He answers; Alice has it without a reload.
  const box = page.getByRole("textbox", { name: "Message #general" });
  await box.fill("on it @alice");
  await box.press("Enter");
  await expect(aliceChat.locator("[data-room-view] .thread-msg").last()).toContainText("on it", { timeout: 15_000 });
  // The platform page reads no panes: nothing to quote from.
  await expect(page.locator("[data-quote-pane]")).toHaveCount(0);
  // Read now: the count is gone, on control's page too.
  await home(bob);
  await expect.poll(() => roomsOf(bob), { timeout: 15_000 }).toContainEqual(expect.objectContaining({ id: general, unread: 0, mentions: 0 }));
  // Activity lists the mention after a reload, though he's read it.
  await platform(bob, "/chat/activity");
  await bob.reload();
  await expect(bob.locator(`[data-chat-activity-view] [data-hit-room="${general}"]`)).toContainText("the mean test fails", { timeout: 15_000 });
});

test("a pane's thread opens beside it, quotes its pane, and only its machine's owner reads it", async () => {
  test.setTimeout(60_000);
  await alice.bringToFront();
  await alice.evaluate((p) => window.__arugula.client.setActive(p), quotedPane);
  await paneEl(alice, quotedPane).click({ button: "right", position: { x: 60, y: 60 } });
  await alice.getByRole("menuitem", { name: "Thread", exact: true }).click();
  const drawer = alice.locator("[data-room-drawer]");
  await expect(drawer).toBeVisible();
  const room = (await drawer.getAttribute("data-room-drawer"))!;
  // The quote: the pane read end to end, and the lines kept.
  await drawer.locator("[data-quote-pane]").click();
  await drawer.locator(`[data-quote-from="alices:${quotedPane}"]`).click();
  await expect(drawer.locator(".chat-quote-text")).toHaveValue(/MEAN-TEST-FAILED/, { timeout: 15_000 });
  await drawer.locator(".chat-quote-text").fill("MEAN-TEST-FAILED");
  await drawer.locator("[data-quote-take]").click();
  await expect(drawer.locator("[data-quote-pending]")).toContainText("on alices");
  await drawer.getByRole("textbox").fill("only mine to read");
  await drawer.getByRole("textbox").press("Enter");
  const mine = drawer.locator(".thread-msg").last();
  await expect(mine).toContainText("only mine to read");
  // Here, where the pane is, its chip opens it.
  await expect(mine.locator("[data-pane-chip]")).toHaveAttribute("data-opens", "1");
  await expect(mine.locator("[data-pane-chip]")).not.toHaveAttribute("data-asleep", "1");
  await alice.keyboard.press("Escape");
  // Bob reads no thread of a machine he can't reach: control says 404.
  const seen = await bob.evaluate(async (r) => (await fetch(`/api/chat/rooms/${r}`)).status, room);
  expect(seen).toBe(404);
});

test("with every daemon stopped, they write in #general and a DM, from the web and a phone", async () => {
  test.setTimeout(90_000);
  await t.stopMachine("alices");
  await expect.poll(() => alice.evaluate(() => window.__arugula.fleet.host("alices")?.state), { timeout: 30_000 }).not.toBe("connected");

  // Alice opens a DM with Bob from the sidebar.
  await platform(aliceChat, `/chat/${general}`);
  const page = aliceChat.locator("[data-chat-control]");
  await expect(page.locator("[data-room-view]")).toBeVisible({ timeout: 15_000 });
  await page.locator(`[data-room-new="${team}/dms"]`).click();
  await aliceChat.locator(".prompt-choices").getByRole("button", { name: "bob" }).click();
  const dm = page.locator("[data-room-view]");
  await expect(dm.locator(".chat-title")).toContainText("bob");
  const dmId = (await dm.getAttribute("data-room-view"))!;
  await dm.getByRole("textbox", { name: "Message bob" }).fill("are you there?");
  await dm.getByRole("textbox", { name: "Message bob" }).press("Enter");
  await expect(dm.locator(".thread-msg").last()).toContainText("are you there?");
  // Her chip says where the pane is; opening it is control's page's.
  await platform(aliceChat, `/chat/${general}`);
  const chip = page.locator("[data-room-view] .thread-quote").first();
  await chip.click();
  await expect(aliceChat.locator("[data-platform-toast]")).toContainText("control's page");
  await platform(aliceChat, `/dm/${dmId}`);
  await expect(dm.locator(".thread-msg").last()).toContainText("are you there?", { timeout: 15_000 });

  // Bob, on his phone: the list, then the DM.
  await platform(bob, "/chat");
  const list = bob.locator("[data-chat-control]");
  await expect(list.locator(`[data-chat-room="${dmId}"]`)).toBeVisible({ timeout: 15_000 });
  await expect(list.locator(`[data-chat-room="${dmId}"] .chat-count`)).toHaveText("1");
  await list.locator(`[data-chat-room="${dmId}"]`).click();
  await expect(list.locator(".thread-msg").last()).toContainText("are you there?");
  const box = list.getByRole("textbox", { name: "Message alice" });
  await box.fill("yes, from my phone");
  await box.press("Enter");
  await expect(dm.locator(".thread-msg").last()).toContainText("yes, from my phone", { timeout: 15_000 });
  // Back to the list, and to #general, where he writes too.
  await list.getByRole("button", { name: "Every room" }).click();
  await list.locator(`[data-chat-room="${general}"]`).click();
  await list.getByRole("textbox", { name: "Message #general" }).fill("still here with alices asleep");
  await list.getByRole("textbox", { name: "Message #general" }).press("Enter");
  await expect(list.locator(".thread-msg").last()).toContainText("still here with alices asleep");
});

test("messages are Markdown with emoji: lists, quotes and code; :ta offers 🎉; the picker inserts; a phone fits", async () => {
  test.setTimeout(90_000);
  await platform(aliceChat, `/chat/${general}`);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const box = room.getByRole("textbox", { name: "Message #general" });
  await expect(box).toBeVisible({ timeout: 15_000 });

  // Enter inside an open fence is a new line; the closing fence lets it send.
  await box.fill("- one\n- two\n\n> quoted\n\n```ts\nconst a = 1;");
  await box.press("Enter");
  await expect(box).toHaveValue(/const a = 1;\n$/);
  await box.pressSequentially("```");
  await box.press("Enter");
  const md = room.locator(".thread-msg").last().locator(".md.chat");
  await expect(md.locator("ul > li")).toHaveCount(2);
  await expect(md.locator("blockquote")).toHaveText("quoted");
  await expect(md.locator(".md-code pre")).toContainText("const a = 1;");
  await expect(md.locator(".md-code .hl-k")).toHaveText("const");
  await expect(md.locator(".md-code [data-copy]")).toBeAttached();

  // A short message is one line, with no margins around its paragraph.
  await box.fill("just one line");
  await box.press("Enter");
  const one = room.locator(".thread-msg").last().locator(".md.chat p");
  await expect(one).toHaveText("just one line");
  expect((await one.boundingBox())!.height).toBeLessThan(26);

  // `:ta` offers the party popper; Tab puts the character in the text.
  await box.fill("party :ta");
  await expect(room.locator(".emoji-pick li").first()).toContainText("🎉");
  await box.press("Tab");
  await expect(box).toHaveValue("party 🎉 ");

  // The picker inserts at the caret, and old stored shortcodes draw as emoji.
  await box.fill("ab");
  await box.press("ArrowLeft");
  await room.locator("[data-emoji-button]").click();
  await room.locator("[data-emoji-picker] .emoji-search").fill("tada");
  await room.locator('[data-emoji-picker] [data-emoji="🎉"]').click();
  await expect(box).toHaveValue("a🎉b");
  await box.fill(":joy: :+1:");
  await box.press("Enter");
  await expect(room.locator(".thread-msg").last()).toContainText("😂 👍");
  await box.fill("🎉");
  await box.press("Enter");
  await expect(room.locator(".thread-msg").last().locator(".md.chat.jumbo")).toBeVisible();

  // Phone width: the code block scrolls inside the message, the page doesn't.
  await platform(bob, "/chat");
  const list = bob.locator("[data-chat-control]");
  await list.locator(`[data-chat-room="${general}"]`).click();
  await expect(list.locator(".thread-msg .md-code").last()).toBeVisible({ timeout: 15_000 });
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await list.locator("[data-emoji-button]").click();
  const sheet = list.locator("[data-emoji-picker]");
  await expect(sheet).toBeVisible();
  expect((await sheet.boundingBox())!.width).toBeLessThanOrEqual(390);
});

test("a mention pushes, and the push's tap opens the room at the message", async () => {
  // A room Bob doesn't have open, and wasn't pushed for in the last minute
  // (control pushes once a minute per room).
  const deploys = await alice.evaluate((tm) => window.__arugula.chat.newChannel(tm, "deploys").then((r) => r.id), team);
  await platform(bob, "/chat");
  await expect(bob.locator(`[data-chat-control] [data-chat-room="${deploys}"]`)).toBeVisible({ timeout: 15_000 });
  const posted = await alice.evaluate((r) => window.__arugula.chat.post(r, "@bob one more"), deploys);
  expect(posted.pushed).toContainEqual({ account: bobAccount, why: "mention" });
  expect(posted.unreached).toEqual([]);
  // His list counts it as a mention, live.
  await expect(bob.locator(`[data-chat-control] [data-chat-room="${deploys}"] .chat-count.mention`)).toHaveText("@1", { timeout: 15_000 });
  // What sw.ts opens for that push (`chat: {room, seq}`), on control's page.
  await bob.goto(`${t.base}/#chat=${deploys}&msg=${posted.seq}`);
  const msg = bob.locator(`[data-chat-control] [data-msg="${posted.seq}"]`);
  await expect(msg).toBeVisible({ timeout: 15_000 });
  await expect(msg).toContainText("one more");
  expect(bob.url().startsWith(app)).toBe(true);
  // Read, now he's looked: nothing for the badge.
  await home(bob);
  await expect.poll(async () => (await roomsOf(bob)).find((r) => r.id === deploys)?.mentions, { timeout: 15_000 }).toBe(0);
});
