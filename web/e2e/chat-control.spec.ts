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
  const md = room.locator(".thread-msg").last().locator(".md.md-chat");
  await expect(md.locator("ul > li")).toHaveCount(2);
  await expect(md.locator("blockquote")).toHaveText("quoted");
  await expect(md.locator(".md-code pre")).toContainText("const a = 1;");
  await expect(md.locator(".md-code .hl-k")).toHaveText("const");
  await expect(md.locator(".md-code [data-copy]")).toBeAttached();

  // A short message is one line, with no margins around its paragraph.
  await box.fill("just one line");
  await box.press("Enter");
  const one = room.locator(".thread-msg").last().locator(".md.md-chat p");
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
  await expect(room.locator(".thread-msg").last().locator(".md.md-chat.md-jumbo")).toBeVisible();

  // Phone width: the code block scrolls inside the message, the page doesn't.
  await platform(bob, "/chat");
  const list = bob.locator("[data-chat-control]");
  await list.locator(`[data-chat-room="${general}"]`).click();
  await expect(list.locator(".thread-msg .md-code").last()).toBeVisible({ timeout: 15_000 });
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await list.locator("[data-emoji-button]").click();
  const sheet = list.locator("[data-emoji-picker]");
  await expect(sheet).toBeVisible();
  // A sheet along the bottom: the phone's width, and no wider.
  expect((await sheet.boundingBox())!.width).toBeLessThanOrEqual(await bob.evaluate(() => innerWidth));
});

test("reactions: Alice and Bob react to one message, each sees both live; a long-press on the phone opens the menu", async () => {
  test.setTimeout(90_000);
  await platform(aliceChat, `/chat/${general}`);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const seq = await aliceChat.evaluate((r) => window.__arugula.chat.post(r, "ship it?").then((p) => p.seq), general);
  const msg = room.locator(`[data-msg="${seq}"]`);
  await expect(msg).toBeVisible({ timeout: 15_000 });

  // Bob is on his phone, in #general, with the message in front of him.
  await platform(bob, "/chat");
  const list = bob.locator("[data-chat-control]");
  await list.locator(`[data-chat-room="${general}"]`).click();
  const onPhone = list.locator(`[data-msg="${seq}"]`);
  await expect(onPhone).toBeVisible({ timeout: 15_000 });

  // Alice: the toolbar's menu, the quick set.
  await msg.hover();
  await msg.locator("[data-react-button]").first().click();
  await expect(msg.locator("[data-react-menu] [data-quick]")).toHaveText(["👍", "❤️", "😂", "🎉", "👀", "✅"]);
  await msg.locator('[data-quick="👍"]').click();
  await expect(msg.locator('[data-reaction="👍"]')).toHaveClass(/mine/);
  // Bob has it live, not his.
  const theirs = onPhone.locator('[data-reaction="👍"]');
  await expect(theirs).toContainText("1", { timeout: 15_000 });
  await expect(theirs).not.toHaveClass(/mine/);

  // Bob adds his to the same chip by tapping it; Alice sees two, and who.
  await theirs.click();
  await expect(theirs).toHaveClass(/mine/);
  await expect(theirs).toContainText("2");
  const chip = msg.locator('[data-reaction="👍"]');
  await expect(chip).toContainText("2", { timeout: 15_000 });
  await expect(chip).toHaveAttribute("title", /You and bob reacted with 👍/);

  // A long-press on the phone opens the menu as a sheet; a quick one adds.
  await onPhone.locator(".md").dispatchEvent("pointerdown", { pointerType: "touch", clientX: 40, clientY: 40 });
  const sheet = list.locator("[data-react-menu]");
  await expect(sheet).toBeVisible({ timeout: 3_000 });
  expect((await sheet.boundingBox())!.width).toBeLessThanOrEqual(await bob.evaluate(() => innerWidth));
  await sheet.locator('[data-quick="✅"]').click();
  await expect(msg.locator('[data-reaction="✅"]')).toContainText("1", { timeout: 15_000 });
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);

  // Taking back: Alice's 👍 leaves 1; the last one takes the chip away.
  await chip.click();
  await expect(chip).toContainText("1");
  await expect(theirs).toContainText("1", { timeout: 15_000 });
  await expect(onPhone.locator('[data-reaction="👍"]')).toHaveClass(/mine/);
  // Deleting the message drops its reactions: only its tombstone is left.
  await msg.hover();
  await msg.locator("[data-msg-delete]").click();
  await expect(room.locator(`[data-msg="${seq}"]`)).toContainText("This message was deleted.", { timeout: 15_000 });
  await expect(room.locator(`[data-msg="${seq}"] [data-reaction]`)).toHaveCount(0);
});

test("pictures come through control, and a teammate's dot shows them around, away or offline", async () => {
  test.setTimeout(90_000);
  // No request from either page leaves control for GitHub's picture host.
  const hosts: string[] = [];
  for (const p of [aliceChat, bob]) p.on("request", (r) => void hosts.push(new URL(r.url()).hostname));
  await platform(aliceChat, `/chat/${general}`);
  await platform(bob, `/chat/${general}`);
  const room = bob.locator("[data-chat-control] [data-room-view]");
  const head = room.locator(".thread-msg.first").first();
  // Alice's message head has her picture's slot and a dot: she's here.
  await expect(head.locator(".msg-avatar")).toBeVisible({ timeout: 15_000 });
  await expect(head.locator("[data-presence]")).toHaveAttribute("data-presence", "active", { timeout: 15_000 });
  // She chooses to appear offline: Bob sees it on his next beat.
  const choose = async (name: string) => {
    await aliceChat.locator(".platform-me").click();
    await aliceChat.getByRole("menuitemradio", { name }).click();
  };
  await choose("Appear offline");
  await expect(head.locator("[data-presence]")).toHaveAttribute("data-presence", "offline", { timeout: 30_000 });
  await choose("Automatic");
  await expect(head.locator("[data-presence]")).toHaveAttribute("data-presence", "active", { timeout: 30_000 });
  // Closing her page: offline for Bob within about a minute.
  await aliceChat.close();
  await expect(head.locator("[data-presence]")).toHaveAttribute("data-presence", "offline", { timeout: 70_000 });
  expect(hosts.filter((h) => h.includes("githubusercontent"))).toEqual([]);
  // The phone's width holds with pictures and dots in it.
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  aliceChat = await alice.context().newPage();
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

test("typing shows to the other reader and clears; @channel is offered; a keyword counts as a mention", async () => {
  test.setTimeout(90_000);
  const room = await alice.evaluate((tm) => window.__arugula.chat.newChannel(tm, "alerts").then((r) => r.id), team);
  await platform(alice, `/chat/${room}-alerts`);
  await platform(bob, `/chat/${room}-alerts`);
  const a = alice.locator("[data-chat-control]");
  const b = bob.locator("[data-chat-control]");
  const abox = a.getByRole("textbox", { name: "Message #alerts" });
  const bbox = b.getByRole("textbox", { name: "Message #alerts" });
  // Nothing while the box is empty.
  await expect(b.locator(".typing-line")).toHaveText("");
  await abox.fill("");
  await abox.pressSequentially("one moment");
  await expect(b.locator(".typing-line")).toContainText(/is typing/, { timeout: 5_000 });
  // Not to the typist, and gone within 6 s of stopping.
  await expect(a.locator(".typing-line")).toHaveText("");
  await expect(b.locator(".typing-line")).toHaveText("", { timeout: 9_000 });
  // On send it clears at once.
  await abox.pressSequentially(" again");
  await expect(b.locator(".typing-line")).toContainText(/is typing/, { timeout: 5_000 });
  await abox.press("Enter");
  await expect(b.locator(".typing-line")).toHaveText("", { timeout: 3_000 });

  // `@` offers the people, then @channel and @here.
  await bbox.fill("@");
  const picks = b.locator(".mention-pick li");
  await expect(picks.nth(0)).toContainText("alice");
  await expect(picks.getByText("@channel")).toBeVisible();
  await expect(picks.getByText("@here")).toBeVisible();
  await bbox.fill("");

  // Bob's keyword makes Alice's message a mention for him.
  await call(bob, "PUT", "/api/chat/keywords", { words: ["deploy failed"] });
  const posted = await alice.evaluate((r) => window.__arugula.chat.post(r, "the deploy failed on geek"), room);
  expect(posted.pushed).toContainEqual({ account: bobAccount, why: "keyword" });
  expect(posted.mentions).toContain(bobAccount);
  // And @channel reaches him as a mention too.
  const all = await alice.evaluate((r) => window.__arugula.chat.post(r, "@channel standup"), room);
  expect(all.mentions).toContain(bobAccount);
});

test("an issue's link unfurls into a card for those who can see it, follows the issue, and a private address makes none", async () => {
  test.setTimeout(90_000);
  // Alice's personal project: Bob is not in it, though he reads #general.
  const api = (p: Page, method: string, path: string, body?: unknown) =>
    p.evaluate(
      async ([m, u, b]) => {
        const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
        return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
      },
      [method, path, body] as const,
    );
  await platform(aliceChat, `/chat/${general}`);
  const made = await api(aliceChat, "POST", "/api/projects/personal/issues", { title: "Link cards", body: "", labels: [], assignees: [] });
  expect(made.status).toBe(200);
  const id = made.body!.id as string;
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const box = room.getByRole("textbox", { name: "Message #general" });
  await box.fill(`look: ${app}/issues/${id} and http://169.254.169.254/latest/meta-data/`);
  await box.press("Enter");
  const card = room.locator(".thread-msg").last().locator("[data-link-card-object]");
  await expect(card).toBeVisible({ timeout: 15_000 });
  await expect(card).toContainText("Link cards");
  await expect(card.locator("[data-link-card-state]")).toHaveText("open");
  // The private address is only text: no web card, and the link is still there.
  await expect(room.locator(".thread-msg").last().locator("[data-link-card-web]")).toHaveCount(0);
  // Phone width: the card stays inside the message.
  expect((await card.boundingBox())!.width).toBeLessThanOrEqual(440);

  // Closing the issue changes the open card (it asks again while open).
  const closed = await api(aliceChat, "PATCH", `/api/issues/${id}`, { state: "closed" });
  expect(closed.status).toBe(200);
  await expect(card.locator("[data-link-card-state]")).toHaveText("closed", { timeout: 45_000 });

  // Bob can't see the issue: the plain link, and nothing of it leaks.
  await platform(bob, `/chat/${general}`);
  const theirs = bob.locator("[data-chat-control] .thread-msg").last();
  await expect(theirs).toContainText(`/issues/${id}`, { timeout: 15_000 });
  await expect(theirs.locator("[data-link-card]")).toHaveCount(0);
  await expect(theirs).not.toContainText("Link cards");

  // The author takes the preview off; it goes for everyone.
  await aliceChat.reload();
  const again = aliceChat.locator("[data-chat-control] .thread-msg").last();
  await expect(again.locator("[data-link-card-object]")).toBeVisible({ timeout: 15_000 });
  await again.locator("[data-link-card-remove]").click();
  await expect(again.locator("[data-link-card]")).toHaveCount(0);
});

test("a channel is quiet until its bell says all, and a snooze shows Alice a moon on Bob", async () => {
  const quiet = await alice.evaluate((tm) => window.__arugula.chat.newChannel(tm, "quiet").then((r) => r.id), team);
  await platform(bob, "/chat");
  const list = bob.locator("[data-chat-control]");
  const row = list.locator(`[data-chat-room="${quiet}"]`);
  await expect(row).toBeVisible({ timeout: 15_000 });
  // Mentions only is the channel default: a plain message pushes nobody, and
  // the row is bold with no count.
  const plain = await alice.evaluate((r) => window.__arugula.chat.post(r, "no mention here"), quiet);
  expect(plain.pushed).toEqual([]);
  await expect(row).toHaveClass(/unread/, { timeout: 15_000 });
  await expect(row.locator(".chat-count")).toHaveCount(0);
  // The bell sets it to every message; the next one pushes him.
  await row.click();
  await list.locator("[data-room-bell]").click();
  await menuItem(bob, "All messages").click();
  await expect(list.locator("[data-room-bell]")).toHaveAttribute("data-room-level", "all");
  const loud = await alice.evaluate((r) => window.__arugula.chat.post(r, "now with feeling"), quiet);
  expect(loud.pushed).toEqual([{ account: bobAccount, why: "message" }]);
  // A phone's bell fits the width.
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  // Pausing for an hour: the bell is a moon, and Alice's DM row with him
  // shows one too.
  await list.locator("[data-room-bell]").click();
  await menuItem(bob, "1 hour").click();
  await expect(list.locator("[data-room-bell]")).toHaveText("🌙");
  const dm = await alice.evaluate(([tm, b]) => window.__arugula.chat.newDm(tm, b).then((r) => r.id), [team, bobAccount]);
  await platform(alice, "/chat");
  await expect(alice.locator(`[data-chat-control] [data-chat-room="${dm}"] [data-dnd]`)).toBeVisible({ timeout: 15_000 });
  const held = await alice.evaluate((r) => window.__arugula.chat.post(r, "are you there?"), dm);
  expect(held.pushed).toEqual([]);
});

/** A menu's item by its label (`openMenu`'s layer). */
function menuItem(p: Page, label: string) {
  return p.locator(".menu .menu-item", { hasText: label }).first();
}
