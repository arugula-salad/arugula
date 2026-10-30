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

import { expect, test, type Page, type Route } from "@playwright/test";
import { closeContexts, menuItem, paneEl } from "./helpers";
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

/** Control's API as whoever `p` is signed in as (`call` is the daemon's). */
async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as unknown };
    },
    [method, path, body] as const,
  );
}

/** Post as whoever `p` is signed in as, from any page of theirs (the
 * platform page has no `window.__arugula`). */
async function say(p: Page, room: string, text: string): Promise<{ seq: number }> {
  return p.evaluate(
    async ([r, t]) => {
      const res = await fetch(`/api/chat/rooms/${r}/messages`, { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: t }) });
      if (!res.ok) throw new Error(`post: ${res.status} ${await res.text()}`);
      return (await res.json()) as { seq: number };
    },
    [room, text] as const,
  );
}

/** Add Bob to a channel Alice made over the API: a channel is its members' (#118). */
const withBob = (room: string) => api(aliceChat, "POST", `/api/chat/rooms/${room}/members`, { accounts: [bobAccount] });

/** The platform page at `path`, signed in by the hop if it has to be. */
async function platform(p: Page, path: string) {
  // In front: a page behind another of its browser's gets no frames, so what
  // waits for one (a render, a caret) waits until something else draws.
  await p.bringToFront();
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

  // The rail's places (#338): with Labs on, every flag is on until a list
  // says otherwise, so Boards sits between Chat and Docs, and Agents after
  // Docs; turn the `boards` flag off and it goes.
  const places = () => alice.locator(".ws-rail > .ws-rail-button .ws-rail-label").allTextContents();
  await expect.poll(places).toEqual(["Panes", "Swarm", "Chat", "Boards", "Docs", "Agents", "Settings"]);
  expect((await call(alice, "PUT", "/api/flags/boards", { on: false })).status).toBe(200);
  // The page reads the flags as it loads.
  await home(alice);
  await show(alice, "alices");
  await expect.poll(places).toEqual(["Panes", "Swarm", "Chat", "Docs", "Agents", "Settings"]);
  await expect(alice.locator(".ws-rail [data-open-boards]")).toHaveCount(0);
  expect((await call(alice, "PUT", "/api/flags/boards", { on: true })).status).toBe(200);
  await home(alice);
  await show(alice, "alices");
  await expect.poll(places).toEqual(["Panes", "Swarm", "Chat", "Boards", "Docs", "Agents", "Settings"]);
  // Boards opens every project's boards (#339), not the project list.
  const [boards] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-boards]").click()]);
  await expect(boards).toHaveURL(`${app}/boards`, { timeout: 15_000 });
  // This team has no board, and the page's own flag list may say boards are off: either way, it's the Boards page.
  await expect(boards.getByRole("heading", { level: 1, name: "Boards" })).toBeVisible({ timeout: 15_000 });
  await boards.close();
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
  await aliceChat.locator("[data-new-channel-name]").fill("general");
  await aliceChat.locator("[data-new-channel-name]").press("Enter");
  const view = page.locator("[data-room-view]");
  await expect(view.locator(".chat-title")).toContainText("general");
  general = (await view.getAttribute("data-room-view"))!;
  // A channel is its members' (#118): Bob is added before he's mentioned.
  await aliceChat.evaluate(
    async ([room, who]) => {
      const r = await fetch(`/api/chat/rooms/${room}/members`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ accounts: [who] }) });
      if (!r.ok) throw new Error(`adding: ${r.status}`);
    },
    [general, bobAccount] as const,
  );
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

// #288: in the desktop app, Chat goes to the platform page in this window,
// not a new one (macOS would show it as a second native tab).
test("in the app, the rail's Chat goes to the platform page in this window", async () => {
  const shell = await alice.context().newPage();
  await shell.addInitScript(() => Object.assign(window, { __arugulaApp: { name: "Arugula app on test-mac", signedIn: true } }));
  await shell.goto(`${t.base}/`);
  const pages = alice.context().pages().length;
  await shell.locator(".ws-rail [data-open-chat]").click();
  await expect(shell).toHaveURL(new RegExp(`^${app}/`), { timeout: 15_000 });
  expect(alice.context().pages().length).toBe(pages);
  // With AppKit's tab strip showing (a second window), its bar starts below it.
  // The rooms view's bar: while chat loads, the page shows a bar of its own,
  // which the rooms view's replaces (#297).
  const bar = shell.locator("[data-chat-control] .platform-bar");
  await expect(bar).toBeVisible({ timeout: 15_000 });
  await shell.evaluate(() => {
    document.documentElement.dataset.nativeTabs = "";
    document.documentElement.style.setProperty("--native-tabs", "64px");
  });
  expect((await bar.boundingBox())!.y).toBe(64);
  await shell.close();
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
  await (await menuItem(alice, "Thread", { exact: true })).click();
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
  await aliceChat.locator('[data-dm-person] input[type="checkbox"]').first().check();
  await aliceChat.locator("[data-dm-start]").click();
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
  await expect(aliceChat.locator("[data-platform-toast]")).toContainText("on Control");
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
  const seq = await say(aliceChat, general, "ship it?").then((p) => p.seq);
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
  await msg.locator("[data-msg-more]").click();
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
  // (Closing this page wouldn't make her offline: her control page is open
  // and beats too. The timeout is the server's, tested there.)
  expect(hosts.filter((h) => h.includes("githubusercontent"))).toEqual([]);
  // The phone's width holds with pictures and dots in it.
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("polish: ↑ edits the last message and Esc cancels, ⋯ has the rest, a scrolled-up room offers the newest, compact rows are one line", async () => {
  test.setTimeout(90_000);
  await platform(aliceChat, `/chat/${general}`);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const box = room.locator(".thread-compose textarea");
  await box.fill("first draft");
  await box.press("Enter");
  const mine = room.locator(".thread-msg.mine").last();
  await expect(mine).toContainText("first draft", { timeout: 15_000 });

  // ↑ in the empty box opens my last message for editing; Esc leaves it alone.
  await box.focus();
  await box.press("ArrowUp");
  const edit = room.locator(".msg-edit textarea");
  await expect(edit).toBeFocused();
  await expect(edit).toHaveValue("first draft");
  await edit.press("Escape");
  await expect(room.locator(".msg-edit")).toHaveCount(0);
  await expect(box).toBeFocused();
  await expect(mine).toContainText("first draft");
  // ... and saves on Enter.
  await box.press("ArrowUp");
  await edit.fill("second draft");
  await edit.press("Enter");
  await expect(mine).toContainText("second draft", { timeout: 15_000 });
  await expect(mine).toContainText("(edited)");
  // With something written, ↑ is the caret's.
  await box.fill("x");
  await box.press("ArrowUp");
  await expect(room.locator(".msg-edit")).toHaveCount(0);
  await box.fill("");

  // The toolbar: quick emoji, the picker, the save star (#116), and ⋯ with
  // quote, link, edit and delete among the rest.
  await mine.hover();
  await expect(mine.locator("[data-tool-quick]")).toHaveCount(3);
  await expect(mine.locator("[data-msg-save]")).toHaveCount(1);
  await expect(mine.locator("[data-msg-thread]")).toHaveCount(1);
  await mine.locator("[data-msg-more]").click();
  await expect(mine.locator("[data-msg-menu] [data-msg-reply], [data-msg-menu] [data-msg-link], [data-msg-menu] [data-msg-edit], [data-msg-menu] [data-msg-delete]")).toHaveCount(4);
  // A press outside closes it. (So does Escape in a browser; this
  // harness's key reaches the page but not the menu, so that stays manual.)
  await room.locator(".chat-head").click({ position: { x: 5, y: 5 } });
  await expect(mine.locator("[data-msg-menu]")).toHaveCount(0);

  // A mention is an accent bar and a tint, not the amber band.
  const mention = room.locator(".thread-msg.for-me").first();
  if (await mention.count()) {
    const shadow = await mention.evaluate((el) => getComputedStyle(el).boxShadow);
    const accent = await aliceChat.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--accent").trim());
    expect(shadow.length).toBeGreaterThan(4);
    expect(accent).not.toBe("");
  }

  // Scrolled up, Bob's new message is "↓ 1 new message"; the button and Esc go down.
  // (Twenty, with what the room has: sixty posts a minute is an account's most.)
  let filler = 0;
  for (let i = 0; i < 20; i++) filler = (await say(aliceChat, general, `filler ${Date.now()}`)).seq;
  // All of them in the room first: one of mine that comes after the scroll
  // takes the room back down (a room follows my own messages).
  await expect(room.locator(`[data-msg="${filler}"]`)).toBeVisible({ timeout: 15_000 });
  const list = room.locator(".thread-list");
  // The scroll's event comes with a frame, which a page in the background
  // doesn't get: send it with the scroll.
  const scrollUp = () =>
    list.evaluate((el) => {
      el.scrollTop = 0;
      el.dispatchEvent(new Event("scroll"));
    });
  await scrollUp();
  await expect(room.locator("[data-jump-newest]")).toBeVisible();
  await platform(bob, "/chat");
  await say(bob, general, "while you were up there");
  await expect(room.locator("[data-jump-newest]")).toContainText("1 new message", { timeout: 15_000 });
  await room.locator("[data-jump-newest]").click();
  await expect(room.locator("[data-jump-newest]")).toHaveCount(0);
  await expect(room.locator(".thread-msg").last()).toContainText("while you were up there");
  // Up again, and Esc once the room knows it's up: before its scroll event,
  // Esc finds the room at the bottom and leaves it be.
  await scrollUp();
  await expect(room.locator("[data-jump-newest]")).toBeVisible();
  await aliceChat.keyboard.press("Escape");
  await expect(room.locator("[data-jump-newest]")).toHaveCount(0);

  // Compact: no pictures, and a row's time, name and text share a line.
  await aliceChat.evaluate(() => (document.documentElement.dataset.density = "compact"));
  const row = room.locator(".thread-msg.first").last();
  await expect(row.locator(".msg-avatar")).toBeHidden();
  const [t, n, x] = await Promise.all(["compact-time", "thread-name", "md"].map((c) => row.locator(`.${c}`).first().boundingBox()));
  expect(Math.abs(t!.y - n!.y)).toBeLessThan(12);
  expect(Math.abs(n!.y - x!.y)).toBeLessThan(12);
  await aliceChat.evaluate(() => (document.documentElement.dataset.density = "cozy"));
});

test("a message that comes while the room's read is out shows: the room's first read, and a read of what's new", async () => {
  test.setTimeout(60_000);
  // What the page's live stream says, in order, as its handler hears it.
  await aliceChat.addInitScript(() => {
    const d = Object.getOwnPropertyDescriptor(EventSource.prototype, "onmessage")!;
    const w = window as unknown as { __heard?: string[] };
    Object.defineProperty(EventSource.prototype, "onmessage", {
      configurable: true,
      get() {
        return d.get!.call(this);
      },
      set(fn: (e: MessageEvent) => void) {
        d.set!.call(this, (e: MessageEvent) => {
          (w.__heard ??= []).push(e.data as string);
          fn(e);
        });
      },
    });
  });
  const heard = () =>
    aliceChat.evaluate(() => ((window as unknown as { __heard?: string[] }).__heard ?? []).map((d) => JSON.parse(d) as { type: string; room?: string; seq?: number }));
  const told = (seq: number) => (e: { type: string; room?: string; seq?: number }) => e.type === "message" && e.room === general && e.seq === seq;
  /** Hold the first answer to a read of #general that `want` picks, once
   * control has given it: `answered` is when it has. The reads after it
   * pass. (Taken off only at the end: a read the page sends while a route
   * is being taken off can hang.) */
  const routes: [(u: URL) => boolean, (r: Route) => Promise<void>][] = [];
  const hold = async (want: (u: URL) => boolean) => {
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    let answered!: () => void;
    const got = new Promise<void>((r) => (answered = r));
    let first = true;
    const pick = (u: URL) => u.pathname === `/api/chat/rooms/${general}/messages` && want(u);
    const handler = async (route: Route) => {
      if (!first) return route.continue();
      first = false;
      const res = await route.fetch();
      answered();
      await held;
      await route.fulfill({ response: res });
    };
    await aliceChat.route(pick, handler);
    routes.push([pick, handler]);
    return { answered: got, release };
  };
  await platform(bob, "/chat");

  // The room's first read is out when Bob writes: on its way back, it
  // hasn't what he wrote, and the page heard of it before it had the room.
  const first = await hold((u) => !u.search);
  await aliceChat.bringToFront();
  const opening = aliceChat.goto(`${app}/chat/${general}`);
  await first.answered;
  const since = (await heard()).length;
  const one = await say(bob, general, "while the room was opening");
  // The stream told of it, or said hello (and so of everything) after it.
  await expect
    .poll(async () => (await heard()).slice(since).some((e) => e.type === "hello" || told(one.seq)(e)))
    .toBe(true);
  first.release();
  await opening;
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  await expect(room.locator(`[data-msg="${one.seq}"]`)).toBeVisible({ timeout: 10_000 });

  // A read of what's new is out when Bob writes again: the page heard of
  // the second while the first's answer was on its way.
  const next = await hold((u) => u.searchParams.has("after"));
  const two = await say(bob, general, "the first of two");
  await next.answered;
  const three = await say(bob, general, "the second of two");
  await expect.poll(async () => (await heard()).some(told(three.seq))).toBe(true);
  next.release();
  await expect(room.locator(`[data-msg="${two.seq}"]`)).toBeVisible({ timeout: 10_000 });
  await expect(room.locator(`[data-msg="${three.seq}"]`)).toBeVisible({ timeout: 10_000 });
  for (const [pick, handler] of routes) await aliceChat.unroute(pick, handler);
});

test("topic and pins: Alice sets a topic and pins two messages, Bob sees both live and the pin list jumps to the message", async () => {
  test.setTimeout(90_000);
  await platform(aliceChat, `/chat/${general}`);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const first = await say(aliceChat, general, "the runbook lives in the wiki").then((p) => p.seq);
  const second = await say(aliceChat, general, "freeze starts friday").then((p) => p.seq);
  // Bob, on his phone, has the room open.
  await platform(bob, "/chat");
  const list = bob.locator("[data-chat-control]");
  await list.locator(`[data-chat-room="${general}"]`).click();
  await expect(list.locator(`[data-msg="${second}"]`)).toBeVisible({ timeout: 15_000 });

  // Alice writes the topic in the header; Bob's header replaces its computed line.
  await expect(room.locator("[data-chat-topic]")).not.toHaveAttribute("data-topic-set", "");
  await room.locator("[data-topic-add]").click();
  await aliceChat.locator(".prompt input").fill("Ship on Fridays");
  await aliceChat.locator(".prompt input").press("Enter");
  await expect(room.locator("[data-chat-topic][data-topic-set]")).toHaveText("Ship on Fridays");
  await expect(list.locator("[data-chat-topic][data-topic-set]")).toHaveText("Ship on Fridays", { timeout: 15_000 });
  // A small system line says who, in both rooms; it is nobody's unread.
  await expect(list.locator('[data-system="topic"]')).toContainText("set the topic: Ship on Fridays");
  await expect(room.locator('[data-system="topic"]')).toContainText("set the topic: Ship on Fridays");

  // Alice pins both from the messages' menus.
  for (const seq of [first, second]) {
    const msg = room.locator(`[data-msg="${seq}"]`);
    await msg.hover();
    await msg.locator("[data-msg-more]").click();
    await msg.locator("[data-msg-pin]").click();
    await expect(msg.locator("[data-pinned-mark]")).toBeVisible();
  }
  await expect(room.locator("[data-pins-count]")).toHaveText("2");
  // Bob has the count and the marks live, and a phone's width holds.
  await expect(list.locator("[data-pins-count]")).toHaveText("2", { timeout: 15_000 });
  await expect(list.locator(`[data-msg="${first}"] [data-pinned-mark]`)).toBeVisible({ timeout: 15_000 });
  await expect(list.locator('[data-system="pin"]')).toHaveCount(2);

  // Bob opens the pins (newest first) and jumps to the older one.
  await list.locator("[data-chat-pins]").click();
  const pins = list.locator("[data-chat-pins-card] [data-pin]");
  await expect(pins).toHaveCount(2);
  await expect(pins.first()).toHaveAttribute("data-pin", String(second));
  const card = await list.locator("[data-chat-pins-card]").boundingBox();
  expect(card!.x + card!.width).toBeLessThanOrEqual(await bob.evaluate(() => innerWidth));
  await pins.nth(1).locator("[data-pin-jump]").click();
  await expect(bob).toHaveURL(new RegExp(`msg=${first}`));
  await expect(list.locator(`[data-msg="${first}"]`)).toBeVisible();
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);

  // Unpinning is live too, and the system line says so.
  await list.locator("[data-chat-pins]").click();
  await list.locator(`[data-pin="${first}"] [data-pin-remove]`).click();
  await expect(room.locator("[data-pins-count]")).toHaveText("1", { timeout: 15_000 });
  await expect(room.locator('[data-system="unpin"]')).toHaveCount(1);
  // Deleting a pinned message drops its pin.
  const msg = room.locator(`[data-msg="${second}"]`);
  await msg.hover();
  await msg.locator("[data-msg-more]").click();
  await msg.locator("[data-msg-delete]").click();
  await expect(room.locator("[data-chat-pins]")).toHaveCount(0, { timeout: 15_000 });
});

test("a mention pushes, and the push's tap opens the room at the message", async () => {
  // A room Bob doesn't have open, and wasn't pushed for in the last minute
  // (control pushes once a minute per room).
  const deploys = await alice.evaluate((tm) => window.__arugula.chat.newChannel(tm, "deploys").then((r) => r.id), team);
  await withBob(deploys);
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

test("mark as unread brings the room back on another page; saved for later syncs, reminds, and is private", async () => {
  test.setTimeout(120_000);
  const room = await alice.evaluate((tm) => window.__arugula.chat.newChannel(tm, "later").then((r) => r.id), team);
  await withBob(room);
  for (const text of ["first thing", "second thing", "third thing"]) {
    await alice.evaluate(([r, t]) => window.__arugula.chat.post(r, t), [room, text]);
  }
  // Bob's phone reads the room; a desktop-sized page of his is on the list.
  await platform(bob, `/chat/${room}-later`);
  const phone = bob.locator("[data-chat-control]");
  await expect(phone.locator(".thread-msg", { hasText: "third thing" })).toBeVisible({ timeout: 15_000 });
  const desk = await bob.context().newPage();
  await desk.setViewportSize({ width: 1280, height: 800 });
  await platform(desk, "/chat");
  const unreadOf = (p: Page) =>
    p.evaluate(async (r) => {
      const v = (await (await fetch("/api/chat", { credentials: "same-origin" })).json()) as { rooms: { id: string; unread: number }[] };
      return v.rooms.find((x) => x.id === r)?.unread;
    }, room);
  await expect.poll(() => unreadOf(desk), { timeout: 15_000 }).toBe(0);

  // Mark the second unread from its menu: the phone keeps its "New" line
  // there, and the other page sees the room unread.
  const second = phone.locator(".thread-msg", { hasText: "second thing" });
  await second.hover();
  await second.locator("[data-msg-more]").click();
  await second.locator("[data-msg-unread]").click();
  await expect(phone.locator("[data-new-line]")).toBeVisible();
  await expect(second.locator("xpath=preceding-sibling::*[1]")).toHaveAttribute("data-new-line");
  await expect.poll(() => unreadOf(desk), { timeout: 15_000 }).toBe(2);
  // A channel is "mentions only" by default (#113): its other unread is
  // bold, not a count.
  await expect(desk.locator(`[data-chat-room="${room}"]`)).toHaveClass(/\bunread\b/);
  await expect.poll(() => unreadOf(bob), { timeout: 5_000 }).toBe(2);

  // Save the third on the phone, with a reminder; it is in Saved on the desktop.
  const third = phone.locator(".thread-msg", { hasText: "third thing" });
  await third.hover();
  await third.locator("[data-msg-more]").click();
  await third.locator("[data-msg-remind-hour]").click();
  await desk.locator("[data-chat-saved]").click();
  const item = desk.locator("[data-chat-saved-view] [data-saved]", { hasText: "third thing" });
  await expect(item).toBeVisible({ timeout: 15_000 });
  await expect(item.locator(".saved-note")).toContainText("Reminder");
  await expect(item).toHaveAttribute("data-saved", `${room}/3`);
  // A plain save from the hover star, on the desktop, shows on the phone.
  await desk.locator(`[data-chat-room="${room}"]`).click();
  const first = desk.locator(".thread-msg", { hasText: "first thing" });
  await first.hover();
  await first.locator("[data-msg-save]").click();
  // (On a phone the toolbar is the long-press sheet: the star is there, not shown.)
  await expect(phone.locator(".thread-msg", { hasText: "first thing" }).locator("[data-msg-save]")).toHaveAttribute("aria-pressed", "true", { timeout: 15_000 });
  // Done moves it down; Remove drops it. It is nobody else's: Alice has none.
  await desk.locator("[data-chat-saved]").click();
  await desk.locator('[data-saved$="/3"] [data-saved-done]').click();
  await expect(desk.locator('[data-saved$="/3"].done')).toBeVisible({ timeout: 10_000 });
  await desk.locator('[data-saved$="/1"] [data-saved-remove]').click();
  await expect(desk.locator('[data-saved$="/1"]')).toHaveCount(0, { timeout: 10_000 });
  expect(await alice.evaluate(async () => ((await (await fetch("/api/chat/saved")).json()) as { items: unknown[] }).items.length)).toBe(0);
  await desk.close();
});

test("typing shows to the other reader and clears; @channel is offered; a keyword counts as a mention", async () => {
  test.setTimeout(90_000);
  const room = await alice.evaluate((tm) => window.__arugula.chat.newChannel(tm, "alerts").then((r) => r.id), team);
  await withBob(room);
  await platform(aliceChat, `/chat/${room}-alerts`);
  // Typing is told only to a page that has told control it shows the room,
  // and never again (#285): Bob's page must have, before Alice types.
  const shown = bob.waitForResponse((r) => r.url().endsWith("/api/chat/live/open") && r.ok() && r.request().postDataJSON()?.room === room);
  await platform(bob, `/chat/${room}-alerts`);
  await shown;
  const a = aliceChat.locator("[data-chat-control]");
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
  await api(bob, "PUT", "/api/chat/keywords", { words: ["deploy failed"] });
  // Out of the room: nobody's pushed for a room they have open.
  await platform(bob, "/chat");
  const posted = await alice.evaluate((r) => window.__arugula.chat.post(r, "the deploy failed on geek"), room);
  expect(posted.pushed).toContainEqual({ account: bobAccount, why: "keyword" });
  expect(posted.mentions).toContain(bobAccount);
  // And @channel reaches him as a mention too.
  const all = await alice.evaluate((r) => window.__arugula.chat.post(r, "@channel standup"), room);
  expect(all.mentions).toContain(bobAccount);
});

test("an issue's link unfurls into a card for those who can see it, follows the issue, and a private address makes none", async () => {
  test.setTimeout(150_000);
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
  // Cards ask again every 60 s while the room is open.
  await expect(card.locator("[data-link-card-state]")).toHaveText("closed", { timeout: 75_000 });

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
  await withBob(quiet);
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
  await chatMenuItem(bob, "All messages").click();
  await expect(list.locator("[data-room-bell]")).toHaveAttribute("data-room-level", "all");
  // Out of the room: nobody's pushed for a room they have open.
  await platform(bob, "/chat");
  const loud = await alice.evaluate((r) => window.__arugula.chat.post(r, "now with feeling"), quiet);
  expect(loud.pushed).toEqual([{ account: bobAccount, why: "message" }]);
  // A phone's bell fits the width.
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  // Pausing for an hour: the bell is a moon, and Alice's DM row with him
  // shows one too.
  await row.click();
  await list.locator("[data-room-bell]").click();
  await chatMenuItem(bob, "1 hour").click();
  await expect(list.locator("[data-room-bell]")).toHaveText("🌙");
  const dm = await alice.evaluate(([tm, b]) => window.__arugula.chat.newDm(tm, b).then((r) => r.id), [team, bobAccount]);
  await platform(aliceChat, "/chat");
  await expect(aliceChat.locator(`[data-chat-control] [data-chat-room="${dm}"] [data-dnd]`)).toBeVisible({ timeout: 15_000 });
  const held = await alice.evaluate((r) => window.__arugula.chat.post(r, "are you there?"), dm);
  expect(held.pushed).toEqual([]);
});

test("threads: Alice starts one, Bob answers from his phone, each sees it live; the footer, unread, the Threads view, a deleted parent", async () => {
  test.setTimeout(120_000);
  await platform(aliceChat, `/chat/${general}`);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const { seq } = await say(aliceChat, general, "who takes the deploy?");
  const msg = room.locator(`[data-msg="${seq}"]`);
  await expect(msg).toBeVisible({ timeout: 15_000 });

  // The toolbar's ↩ opens a thread beside the room, and its link names it.
  await msg.hover();
  await msg.locator("[data-msg-thread]").click();
  const panel = aliceChat.locator(`[data-thread-panel="${seq}"]`);
  await expect(panel).toBeVisible();
  await expect(aliceChat).toHaveURL(new RegExp(`thread=${seq}`));
  await expect(panel.locator(`[data-msg="${seq}"]`)).toContainText("who takes the deploy?");
  const reply = panel.getByRole("textbox", { name: "Reply in thread" });
  await reply.fill("not me");
  await reply.press("Enter");
  await expect(panel.locator(".thread-msg").last()).toContainText("not me", { timeout: 15_000 });
  // The reply is the thread's, not the room's; the parent says it has one.
  await expect(room.locator(".thread-msg", { hasText: "not me" })).toHaveCount(0);
  await expect(msg.locator("[data-thread-footer]")).toContainText("1 reply", { timeout: 15_000 });

  // Bob, on his phone, finds the footer and opens the thread: the whole screen.
  await platform(bob, "/chat");
  const list = bob.locator("[data-chat-control]");
  await list.locator(`[data-chat-room="${general}"]`).click();
  const onPhone = list.locator(`[data-msg="${seq}"]`);
  await expect(onPhone.locator("[data-thread-footer]")).toContainText("1 reply", { timeout: 15_000 });
  await onPhone.locator("[data-thread-footer]").click();
  const sheet = bob.locator(`[data-thread-panel="${seq}"]`);
  await expect(sheet).toBeVisible();
  await expect(list.locator("[data-room-view]")).toHaveCount(0);
  await expect(sheet.locator(".thread-msg").last()).toContainText("not me");
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect((await sheet.boundingBox())!.width).toBeLessThanOrEqual(await bob.evaluate(() => innerWidth));

  // He answers, and also sends it to the channel: both see it live, in the
  // thread and in the room.
  await sheet.locator("[data-thread-also] input").check();
  const box = sheet.getByRole("textbox", { name: "Reply in thread" });
  await box.fill("me, from the bus");
  await box.press("Enter");
  await expect(panel.locator(".thread-msg").last()).toContainText("me, from the bus", { timeout: 15_000 });
  await expect(room.locator(".thread-msg", { hasText: "me, from the bus" })).toBeVisible();
  await expect(msg.locator("[data-thread-footer]")).toContainText("2 replies");
  await expect(panel.locator("[data-reply-also]")).toBeVisible();
  // Back to the room, with the footer where the message is.
  await sheet.locator("[data-thread-back]").click();
  await expect(list.locator("[data-room-view]")).toBeVisible();
  await expect(onPhone.locator("[data-thread-footer]")).toContainText("2 replies");

  // Alice answers again; Bob follows (he replied), so it is unread for him in
  // the Threads view and its badge, and not in the room.
  await api(aliceChat, "POST", `/api/chat/rooms/${general}/messages`, { text: "ok, I will", parent: seq });
  const threadsUnread = async () => ((await api(bob, "GET", "/api/chat")).body as { threads_unread: number }).threads_unread;
  await expect.poll(threadsUnread, { timeout: 15_000 }).toBe(1);
  await platform(bob, "/chat/threads");
  const row = bob.locator(`[data-chat-threads-view] [data-thread-row="${seq}"]`);
  await expect(row).toBeVisible({ timeout: 15_000 });
  await expect(row.locator("[data-thread-row-unread]")).toHaveText("1");
  await expect(row).toContainText("ok, I will");
  // Opening it puts the New line before it, and reads the thread.
  await row.click();
  const opened = bob.locator(`[data-thread-panel="${seq}"]`);
  await expect(opened.locator("[data-new-line]")).toBeVisible({ timeout: 15_000 });
  await expect.poll(threadsUnread, { timeout: 15_000 }).toBe(0);
  // A link to a reply names its thread, and opens it.
  const replyLink = ((await api(bob, "GET", `/api/chat/rooms/${general}/messages?thread=${seq}`)).body as { messages: { seq: number }[] }).messages[0].seq;
  await platform(bob, `/chat/${general}`);
  await bob.goto(`${bob.url().split("?")[0]}?msg=${replyLink}&thread=${seq}`);
  await expect(bob.locator(`[data-thread-panel="${seq}"] [data-msg="${replyLink}"]`)).toBeVisible({ timeout: 15_000 });

  // Search finds a reply, and its hit opens the thread.
  const found = ((await api(aliceChat, "GET", "/api/chat/search?q=bus")).body as { found: { message: { parent?: number } }[] }).found.map((h) => h.message.parent);
  expect(found).toEqual([seq]);

  // Alice unfollows from the panel; deleting the parent leaves its tombstone
  // with the thread under it.
  await panel.locator("[data-thread-follow]").click();
  await expect(panel.locator("[data-thread-follow]")).toHaveText("Follow");
  // At this width the panel lies over the room's right side, where ⋯ is.
  await panel.locator("[data-thread-close]").click();
  await msg.hover();
  await msg.locator("[data-msg-more]").click();
  await msg.locator("[data-msg-delete]").click();
  await expect(room.locator(`[data-msg="${seq}"]`)).toContainText("This message was deleted.", { timeout: 15_000 });
  await expect(room.locator(`[data-msg="${seq}"] [data-thread-footer]`)).toContainText("3 replies");
});

/** A menu's item by its label (`openMenu`'s layer). */
function chatMenuItem(p: Page, label: string) {
  return p.locator(".menu .menu-item", { hasText: label }).first();
}

// A 2x2 PNG, so the browser decodes it (a broken image has no width).
const PNG_B64 = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAFklEQVR4nGP8z8Dwn4GBgYGJAQoAHhgCAh6X4CYAAAAASUVORK5CYII=";

test("files: Alice pastes a screenshot and Bob's phone sees it inline; she drops a PDF and Bob downloads it; outsiders get nothing", async ({ browser }) => {
  test.setTimeout(120_000);
  await platform(aliceChat, `/chat/${general}`);
  const room = aliceChat.locator("[data-chat-control] [data-room-view]");
  const box = room.getByRole("textbox", { name: "Message #general" });

  // Paste: the clipboard holds a picture, not text. It goes up as a chip.
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
  await expect(chip).toHaveAttribute("data-attach-chip", /^pasted-.*\.png$/);

  // Drop: a PDF onto the room, beside the picture. Remove works per chip.
  await room.locator(".thread-list").evaluate((el) => {
    const data = new DataTransfer();
    data.items.add(new File(["%PDF-1.4\n% e2e\n"], "plan.pdf", { type: "application/pdf" }));
    el.dispatchEvent(new DragEvent("drop", { dataTransfer: data, bubbles: true, cancelable: true }));
  });
  await expect(chip).toHaveCount(2);
  await expect(room.locator('[data-attach-chip="plan.pdf"] progress')).toHaveCount(0, { timeout: 15_000 });
  await room.locator('[data-attach-chip="plan.pdf"] [data-attach-remove]').click();
  await expect(chip).toHaveCount(1);
  await room.locator("[data-attach-input]").setInputFiles({ name: "plan.pdf", mimeType: "application/pdf", buffer: Buffer.from("%PDF-1.4\n% e2e\n") });
  await expect(room.locator('[data-attach-chip="plan.pdf"]')).toBeVisible();
  await expect(room.locator("[data-attach-chip] progress")).toHaveCount(0, { timeout: 15_000 });

  await box.fill("the screenshot and the plan");
  await box.press("Enter");
  const mine = room.locator(".thread-msg").last();
  const image = mine.locator("[data-msg-image] img");
  await expect(image).toBeVisible({ timeout: 15_000 });
  // Under control's CSP the picture really loaded (a blocked one has no width).
  await expect.poll(() => image.evaluate((i: HTMLImageElement) => i.naturalWidth)).toBe(2);
  await expect(room.locator("[data-attach-chip]")).toHaveCount(0);
  await expect(mine.locator('[data-msg-file="plan.pdf"]')).toContainText("PDF");

  // It opens full size, and a press beside the picture closes it. (Escape
  // does too in a browser; this harness's key doesn't reach it, as with ⋯.)
  await mine.locator("[data-msg-image]").click();
  await expect(aliceChat.locator("[data-image-viewer] img")).toBeVisible();
  await aliceChat.locator("[data-image-viewer]").click({ position: { x: 5, y: 5 } });
  await expect(aliceChat.locator("[data-image-viewer]")).toHaveCount(0);

  // Bob, on his phone, sees it inline within the screen, and downloads the PDF.
  await platform(bob, `/chat/${general}`);
  const theirs = bob.locator("[data-chat-control] .thread-msg").last();
  await expect(theirs.locator("[data-msg-image] img")).toBeVisible({ timeout: 15_000 });
  await expect.poll(() => theirs.locator("[data-msg-image] img").evaluate((i: HTMLImageElement) => i.naturalWidth)).toBe(2);
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  const [download] = await Promise.all([bob.waitForEvent("download"), theirs.locator('[data-msg-file="plan.pdf"] [data-file-download]').click()]);
  expect(download.suggestedFilename()).toBe("plan.pdf");

  // The address answers readers of the room and no one else: not a visitor
  // with no session, and not through a public path.
  const url = await theirs.locator("[data-msg-image] img").getAttribute("src");
  const stranger = await browser.newContext();
  const denied = await stranger.request.get(`${app}${url}`);
  expect(denied.status()).not.toBe(200);
  const seen = await bob.request.get(`${app}${url}`);
  expect(seen.status()).toBe(200);
  expect(seen.headers()["x-content-type-options"]).toBe("nosniff");
  await stranger.close();

  // Deleting the message takes the files with it.
  await mine.hover();
  await mine.locator("[data-msg-more]").click();
  await mine.locator("[data-msg-delete]").click();
  await expect(mine).toContainText("This message was deleted.", { timeout: 15_000 });
  expect((await bob.request.get(`${app}${url}`)).status()).toBe(404);
});

test("private channels, the browser, leaving, and a group DM of three (#117, #118)", async ({ browser }) => {
  test.setTimeout(240_000);
  // Carol joins Acme as an editor.
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
  const carol = await t.person(browser, "carol", pixel7);
  await carol.goto(link);
  await carol.locator("[data-accept-invite]").click();
  await expect(carol.locator("[data-invite-pending]")).toBeVisible();
  await alice.bringToFront();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(3);
  await home(carol);
  await expect.poll(() => chatOn(carol), { timeout: 15_000 }).toBe(true);

  // Alice makes a private channel and invites Bob, from the details panel.
  await platform(aliceChat, "/chat");
  const page = aliceChat.locator("[data-chat-control]");
  await page.locator(`[data-room-new="${team}/channels"]`).click();
  await aliceChat.locator("[data-new-channel-name]").fill("plans");
  await aliceChat.locator("[data-new-channel-private]").check();
  await aliceChat.locator("[data-new-channel-name]").press("Enter");
  const view = page.locator("[data-room-view]");
  await expect(view.locator(".chat-title")).toContainText("plans");
  await expect(view.locator("[data-chat-topic]")).toContainText("private");
  const plans = (await view.getAttribute("data-room-view"))!;
  await page.locator("[data-chat-details]").click();
  await page.locator("[data-member-add]").click();
  await page.locator("[data-member-addable] li", { hasText: "bob" }).locator("button").click();
  await expect(page.locator(`[data-room-person="${bobAccount}"]`)).toBeVisible({ timeout: 15_000 });
  const box = view.getByRole("textbox", { name: "Message #plans" });
  await box.fill("the secret roadmap");
  await box.press("Enter");
  await expect(view.locator(".thread-msg").last()).toContainText("the secret roadmap");

  // Bob has it; Carol has no sign of it: not in her list, not in the
  // browser, and its address is as if it weren't there.
  await platform(bob, "/chat");
  await expect(bob.locator(`[data-chat-room="${plans}"]`)).toBeVisible({ timeout: 15_000 });
  await platform(carol, "/chat");
  const hers = carol.locator("[data-chat-control]");
  await expect(hers.locator(`[data-chat-room="${plans}"]`)).toHaveCount(0);
  await hers.locator(`[data-room-browse="${team}/channels"]`).click();
  await expect(carol.locator("[data-channel-browser]")).toBeVisible();
  await expect(carol.locator('[data-browse-channel="plans"]')).toHaveCount(0);
  // #general is Alice's and Bob's, public: Carol sees it, and joins.
  await carol.locator('[data-browse-channel="general"] [data-browse-join]').click();
  await expect(hers.locator("[data-room-view] .chat-title")).toContainText("general", { timeout: 15_000 });
  // Back on her list (a phone's room has the screen), it's hers now.
  await platform(carol, "/chat");
  await expect(hers.locator(`[data-chat-room="${general}"]`)).toBeVisible();
  await platform(carol, `/chat/${plans}`);
  await expect(carol.locator("[data-room-missing]")).toBeVisible({ timeout: 15_000 });

  // Leaving #general (from the room's menu, UI audit #169) takes it from
  // her list, and it stays in the browser.
  await platform(carol, `/chat/${general}`);
  await hers.locator("[data-room-move]").click();
  await carol.getByRole("menuitem", { name: "Leave #general" }).click();
  await platform(carol, "/chat");
  await expect(hers.locator(`[data-chat-room="${general}"]`)).toHaveCount(0, { timeout: 15_000 });
  await hers.locator(`[data-room-browse="${team}/channels"]`).click();
  await expect(carol.locator('[data-browse-channel="general"] [data-browse-join]')).toBeVisible();
  await carol.keyboard.press("Escape");

  // A DM of Alice, Bob and Carol: named after them, live for all three.
  await platform(aliceChat, "/chat");
  await page.locator(`[data-room-new="${team}/dms"]`).click();
  for (const who of ["bob", "carol"]) await aliceChat.locator("[data-new-dm] li", { hasText: who }).locator("input").check();
  await aliceChat.locator("[data-dm-start]").click();
  const trio = page.locator("[data-room-view]");
  await expect(trio.locator(".chat-title")).toContainText(/bob, carol|carol, bob/, { timeout: 15_000 });
  const trioId = (await trio.getAttribute("data-room-view"))!;
  const trioBox = trio.getByRole("textbox").first();
  await trioBox.fill("hello, the three of us");
  await trioBox.press("Enter");
  await platform(carol, "/chat");
  await expect(hers.locator(`[data-chat-room="${trioId}"]`)).toBeVisible({ timeout: 15_000 });
  await hers.locator(`[data-chat-room="${trioId}"]`).click();
  await expect(hers.locator(".thread-msg").last()).toContainText("hello, the three of us");
  const back = hers.getByRole("textbox").first();
  await back.fill("hi from my phone");
  await back.press("Enter");
  await expect(trio.locator(".thread-msg").last()).toContainText("hi from my phone", { timeout: 15_000 });
  // The phone's list and the room fit its width.
  expect(await carol.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  // Opening a DM with the same people reopens it.
  await platform(aliceChat, "/chat");
  await page.locator(`[data-room-new="${team}/dms"]`).click();
  for (const who of ["bob", "carol"]) await aliceChat.locator("[data-new-dm] li", { hasText: who }).locator("input").check();
  await aliceChat.locator("[data-dm-start]").click();
  await expect(page.locator("[data-room-view]")).toHaveAttribute("data-room-view", trioId, { timeout: 15_000 });
});
