// Team chat as a block in the layout (#123): Alice and Bob in team Acme with
// chat on, and a machine of Alice's (alices). Alice opens a chat block beside
// a terminal on it and talks in #ops; a block pinned to #ops (no sidebar)
// comes back after a reload and a restart of the machine's daemon; a quote of
// the terminal beside it is one click; and its tab says what's unread. The
// rig is chat-control's.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";
import { pixel7 } from "./phones";

const t = new TeamControl("chatb");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let team = "";
let ops = "";
let terminal = 0;
let chatBlock = 0;
let pinnedBlock = 0;

const chatOn = (p: Page) => p.evaluate(() => window.__arugula.chat.on);
const block = (id: number) => alice.locator(`[data-chat-block="${id}"]`);
const stateOf = async (id: number) => ((await call(alice, "GET", `/api/blocks/${id}`)).body as { state: { room?: string; pinned?: boolean } }).state;

/** The shown machine's layout, opened again from nothing: a reload. */
async function reopen() {
  await home(alice);
  await expect.poll(() => chatOn(alice), { timeout: 15_000 }).toBe(true);
  await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "alices");
}

test("a team with chat on, a machine of Alice's, and #ops", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
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
  await home(alice);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await t.addMachine(alice, "alices");
  for (const p of [alice, bob]) {
    await home(p);
    await expect.poll(() => chatOn(p), { timeout: 15_000 }).toBe(true);
  }
  await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "alices");
  ops = await alice.evaluate(async (tm) => (await window.__arugula.chat.newChannel(tm, "ops")).id, team);
  // A channel is its members' (#118): Bob is added to #ops.
  const bobAccount = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "bob")!.account);
  const added = await alice.evaluate(
    async ([r, b]) => (await fetch(`/api/chat/rooms/${r}/members`, { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ accounts: [b] }) })).status,
    [ops, bobAccount] as const,
  );
  expect(added).toBe(200);
  await expect.poll(() => bob.evaluate((r) => !!window.__arugula.chat.room(r), ops), { timeout: 15_000 }).toBe(true);
  const run = await call(alice, "POST", "/api/run", { command: "echo MEAN-TEST-FAILED; sleep 600" });
  expect(run.status, JSON.stringify(run.body)).toBe(200);
  terminal = (run.body as { pane: number }).pane;
  await expect.poll(() => alice.evaluate((p) => window.__arugula.text(p), terminal), { timeout: 15_000 }).toContain("MEAN-TEST-FAILED");
});

test("a chat block opens beside the terminal, and posts as Alice", async () => {
  test.setTimeout(90_000);
  const opened = await call(alice, "POST", "/api/blocks", { type: "team_chat", config: { room: null, pinned: false }, split: terminal, local: true });
  expect(opened.status, JSON.stringify(opened.body)).toBe(200);
  chatBlock = (opened.body as { block: number }).block;
  // The terminal's tab, where the block went (`/api/run` made a new tab).
  await alice.bringToFront();
  await alice.evaluate((p) => window.__arugula.client.setActive(p), terminal);
  const chat = block(chatBlock);
  await expect(chat).toBeVisible({ timeout: 15_000 });
  // In the layout beside the terminal: one tab, both panes.
  await expect(alice.locator(`[data-pane="${terminal}"]`)).toBeVisible();
  // Its rooms: a sidebar, or in a narrow slot (as here) the list, as a phone has.
  await expect(chat.locator(".chat-side, .chat-list").first()).toBeVisible();
  await chat.locator(`[data-chat-room="${ops}"]`).click();
  const view = chat.locator("[data-room-view]");
  await expect(view.locator(".chat-title")).toContainText("ops");
  const box = view.getByRole("textbox", { name: "Message #ops" });
  await box.fill("deploy is green");
  await box.press("Enter");
  await expect(view.locator(".thread-msg").last()).toContainText("deploy is green");
  // Where it is is kept in its layout entry.
  await expect.poll(async () => (await stateOf(chatBlock)).room, { timeout: 10_000 }).toBe(ops);
  // Live for the other side.
  const lastFor = (r: string) =>
    bob.evaluate(async (room) => {
      const v = (await (await fetch(`/api/chat/rooms/${room}/messages`, { credentials: "same-origin" })).json()) as { messages: { text: string }[] };
      return v.messages.at(-1)?.text ?? "";
    }, r);
  await expect.poll(() => lastFor(ops), { timeout: 15_000 }).toContain("deploy is green");
});

test("a quote of the terminal beside it is one click", async () => {
  test.setTimeout(60_000);
  const view = block(chatBlock).locator("[data-room-view]");
  await view.locator("[data-quote-pane]").click();
  const pending = view.locator("[data-quote-pending]");
  await expect(pending).toContainText("MEAN-TEST-FAILED", { timeout: 10_000 });
  await view.getByRole("textbox", { name: "Message #ops" }).fill("the failing line");
  await view.getByRole("textbox", { name: "Message #ops" }).press("Enter");
  const sent = view.locator(".thread-msg").last();
  await expect(sent).toContainText("the failing line");
  await expect(sent.locator(".thread-quote pre")).toContainText("MEAN-TEST-FAILED");
  // The chip opens the pane in this layout, not a new tab.
  const tabs = await alice.locator(".tab[data-tab-id]").count();
  await sent.locator("[data-pane-chip]").click();
  await expect(alice.locator(`[data-pane="${terminal}"].active`)).toBeVisible();
  expect(await alice.locator(".tab[data-tab-id]").count()).toBe(tabs);
});

test("a block pinned to #ops has no sidebar and survives a reload and a restart", async () => {
  test.setTimeout(180_000);
  const opened = await call(alice, "POST", "/api/blocks", { type: "team_chat", config: { room: ops, pinned: true }, split: terminal, local: true });
  expect(opened.status, JSON.stringify(opened.body)).toBe(200);
  pinnedBlock = (opened.body as { block: number }).block;
  const pinned = block(pinnedBlock);
  await expect(pinned.locator(`[data-room-view="${ops}"]`)).toBeVisible({ timeout: 15_000 });
  await expect(pinned.locator(".chat-side")).toHaveCount(0);
  await expect(pinned).toHaveAttribute("data-chat-pinned-to", ops);

  await reopen();
  await expect(block(pinnedBlock).locator(`[data-room-view="${ops}"]`)).toBeVisible({ timeout: 20_000 });
  await expect(block(pinnedBlock).locator(".chat-side")).toHaveCount(0);

  await t.restartMachine("alices");
  await expect.poll(() => live(alice, ["alices"]), { timeout: 60_000 }).toBe(true);
  await reopen();
  expect(await stateOf(pinnedBlock)).toMatchObject({ room: ops, pinned: true });
  await expect(block(pinnedBlock).locator(`[data-room-view="${ops}"]`)).toBeVisible({ timeout: 20_000 });
  // The unpinned one came back where it was left, with its sidebar.
  expect((await stateOf(chatBlock)).room).toBe(ops);
});

test("a tab with a chat block says what's unread in chat", async () => {
  test.setTimeout(90_000);
  // Only a chat block nobody sees: the others read what comes (a room
  // shown is read), so they go, and this one has a tab of its own.
  for (const p of [chatBlock, pinnedBlock]) await alice.evaluate((b) => window.__arugula.client.intent({ op: "close_pane", pane: b }), p);
  const other = await call(alice, "POST", "/api/blocks", { type: "team_chat", config: { room: null, pinned: false }, local: true });
  expect(other.status, JSON.stringify(other.body)).toBe(200);
  await alice.evaluate((p) => window.__arugula.client.setActive(p), terminal);
  // A mention: a channel's plain message counts for nothing at its default level (#113).
  await bob.evaluate((r) => window.__arugula.chat.post(r, "@alice who is on call?"), ops);
  await expect(alice.locator("[data-chat-tab-badge]").first()).toBeVisible({ timeout: 15_000 });
});
