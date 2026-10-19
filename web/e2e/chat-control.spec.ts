// The Chat page on control's chat (M123, over M122's API): Alice and Bob in
// team Acme, with control's `chat` flag on for the team. Alice has one
// machine of her own (alices), which Bob can't reach; Bob has none, and
// reads chat on a phone.
//
// Alice makes #general and quotes a pane on alices in it, mentioning Bob:
// control pushes him, his page has it over the live stream, and the quote's
// chip opens for her and not for him. A pane's thread opens beside the
// pane, and only its machine's owner reads it. Then every daemon is
// stopped: they write in #general and a DM, from the web and the phone. A
// push's tap (`#chat=<room>&msg=<n>`) opens the room at the message.

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
let bob: Page;
let team = "";
let general = "";
let bobAccount = "";

const chatOn = (p: Page) => p.evaluate(() => window.__arugula.chat.on);
const roomsOf = (p: Page) => p.evaluate(() => window.__arugula.chat.rooms.map((r) => ({ id: r.id, kind: r.kind, name: r.name, unread: r.unread, mentions: r.mentions })));

test("a team with chat on, and a machine of Alice's", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start();
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
  // Bob has no machine: control's page shows him chat anyway.
  await expect(bob.locator("[data-open-chat]")).toBeVisible();
});

test("Alice makes #general and quotes a pane on alices in it, mentioning Bob", async () => {
  test.setTimeout(90_000);
  const run = await call(alice, "POST", "/api/run", { command: "echo MEAN-TEST-FAILED; sleep 600" });
  expect(run.status, JSON.stringify(run.body)).toBe(200);
  const pane = (run.body as { pane: number }).pane;
  await expect.poll(() => alice.evaluate((p) => window.__arugula.text(p), pane), { timeout: 15_000 }).toContain("MEAN-TEST-FAILED");

  await alice.locator(".ws-rail [data-open-chat]").click();
  const page = alice.locator("[data-chat-control]");
  await expect(page).toBeVisible();
  await page.locator(`[data-room-new="${team}/channels"]`).click();
  await alice.locator(".prompt input").fill("general");
  await alice.locator(".prompt input").press("Enter");
  const view = page.locator("[data-room-view]");
  await expect(view.locator(".chat-title")).toContainText("general");
  general = (await view.getAttribute("data-room-view"))!;

  // The quote: the pane read end to end, and the lines kept.
  await view.locator("[data-quote-pane]").click();
  await view.locator(`[data-quote-from="alices:${pane}"]`).click();
  await expect(view.locator(".chat-quote-text")).toHaveValue(/MEAN-TEST-FAILED/, { timeout: 15_000 });
  await view.locator(".chat-quote-text").fill("MEAN-TEST-FAILED");
  await view.locator("[data-quote-take]").click();
  await expect(view.locator("[data-quote-pending]")).toContainText("on alices");
  const box = view.getByRole("textbox", { name: "Message #general" });
  await box.fill("@bob the mean test fails on alices");
  await box.press("Enter");
  const mine = view.locator(".thread-msg").last();
  await expect(mine).toContainText("the mean test fails");
  await expect(mine.locator("[data-pane-chip]")).toHaveAttribute("data-opens", "1");
  await expect(mine.locator(".thread-quote pre")).toHaveText("MEAN-TEST-FAILED");
});

test("Bob has it live, mentioned; the chip doesn't open for him", async () => {
  await expect.poll(() => roomsOf(bob), { timeout: 15_000 }).toContainEqual(expect.objectContaining({ id: general, unread: 1, mentions: 1 }));
  // A push's tap opens the room (sw.ts makes this address).
  await bob.goto(`/#chat=${general}`);
  const page = bob.locator("[data-chat-control]");
  const msg = page.locator(".thread-msg").last();
  await expect(msg).toContainText("the mean test fails", { timeout: 15_000 });
  await expect(msg).toHaveClass(/for-me/);
  await expect(msg.locator("[data-pane-chip]")).toHaveAttribute("data-opens", "0");
  await expect(msg.locator(".thread-quote.inert pre")).toHaveText("MEAN-TEST-FAILED");
  // Read now: the count is gone.
  await expect.poll(() => roomsOf(bob)).toContainEqual(expect.objectContaining({ id: general, unread: 0, mentions: 0 }));
  // He answers; Alice has it without a reload.
  const box = page.getByRole("textbox", { name: "Message #general" });
  await box.fill("on it @alice");
  await box.press("Enter");
  await expect(alice.locator("[data-room-view] .thread-msg").last()).toContainText("on it", { timeout: 15_000 });
  // No machine of his: nothing to quote from, and nothing to connect to.
  await expect(page.locator("[data-quote-pane]")).toHaveCount(0);
  await expect(bob.locator("#status")).toHaveCount(0);
  // Activity lists the mention after a reload, though he's read it.
  await bob.goto("/#chat=activity");
  await bob.reload();
  await expect(bob.locator(`[data-chat-activity-view] [data-hit-room="${general}"]`)).toContainText("the mean test fails", { timeout: 15_000 });
});

test("a pane's thread opens beside it, and only its machine's owner reads it", async () => {
  test.setTimeout(60_000);
  await alice.locator(".ws-rail [data-open-panes]").click();
  const panes = (await call(alice, "GET", "/api/panes")).body as unknown as { id: number }[];
  const pane = panes[0].id;
  await alice.evaluate((p) => window.__arugula.client.setActive(p), pane);
  await paneEl(alice, pane).click({ button: "right", position: { x: 60, y: 60 } });
  await alice.getByRole("menuitem", { name: "Thread", exact: true }).click();
  const drawer = alice.locator("[data-room-drawer]");
  await expect(drawer).toBeVisible();
  const room = (await drawer.getAttribute("data-room-drawer"))!;
  await drawer.getByRole("textbox").fill("only mine to read");
  await drawer.getByRole("textbox").press("Enter");
  await expect(drawer.locator(".thread-msg")).toContainText("only mine to read");
  await alice.keyboard.press("Escape");
  // Bob reads no thread of a machine he can't reach: control says 404.
  const seen = await bob.evaluate(async (r) => (await fetch(`/api/chat/rooms/${r}`)).status, room);
  expect(seen).toBe(404);
  expect((await roomsOf(bob)).map((r) => r.id)).not.toContain(room);
});

test("with every daemon stopped, they write in #general and a DM, from the web and a phone", async () => {
  test.setTimeout(90_000);
  await t.stopMachine("alices");
  await expect.poll(() => alice.evaluate(() => window.__arugula.fleet.host("alices")?.state), { timeout: 30_000 }).not.toBe("connected");

  // Alice opens a DM with Bob from the sidebar.
  await alice.goto(`/#chat=${general}`);
  const page = alice.locator("[data-chat-control]");
  await expect(page.locator("[data-room-view]")).toBeVisible({ timeout: 15_000 });
  await page.locator(`[data-room-new="${team}/dms"]`).click();
  await alice.locator(".prompt-choices").getByRole("button", { name: "bob" }).click();
  const dm = page.locator("[data-room-view]");
  await expect(dm.locator(".chat-title")).toContainText("bob");
  const dmId = (await dm.getAttribute("data-room-view"))!;
  await dm.getByRole("textbox", { name: "Message bob" }).fill("are you there?");
  await dm.getByRole("textbox", { name: "Message bob" }).press("Enter");
  await expect(dm.locator(".thread-msg").last()).toContainText("are you there?");
  // Her chip on the asleep machine says so rather than open nothing.
  await alice.goto(`/#chat=${general}`);
  const chip = page.locator("[data-room-view] [data-pane-chip][data-opens='1']").first();
  await expect(chip).toHaveAttribute("data-asleep", "1");
  await expect(page.locator("[data-room-view] .thread-quote.asleep").first()).toContainText("asleep");
  // Its machine asleep, nothing says it's connecting over the room.
  await expect(alice.locator("#status")).toHaveCount(0);
  await chip.click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.client.error)).toContain("asleep");
  await alice.goto(`/#chat=${dmId}`);
  await expect(dm.locator(".thread-msg").last()).toContainText("are you there?", { timeout: 15_000 });

  // Bob, on his phone: the list, then the DM.
  await bob.goto("/#chat");
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

test("a mention pushes, and the push's tap opens the room at the message", async () => {
  // A room Bob doesn't have open, and wasn't pushed for in the last minute
  // (control pushes once a minute per room).
  const deploys = await alice.evaluate((tm) => window.__arugula.chat.newChannel(tm, "deploys").then((r) => r.id), team);
  await bob.goto("/#chat");
  await expect(bob.locator(`[data-chat-control] [data-chat-room="${deploys}"]`)).toBeVisible({ timeout: 15_000 });
  const posted = await alice.evaluate((r) => window.__arugula.chat.post(r, "@bob one more"), deploys);
  expect(posted.pushed).toContainEqual({ account: bobAccount, why: "mention" });
  expect(posted.unreached).toEqual([]);
  // His list counts it as a mention, live.
  await expect(bob.locator(`[data-chat-control] [data-chat-room="${deploys}"] .chat-count.mention`)).toHaveText("@1", { timeout: 15_000 });
  // What sw.ts opens for that push (`chat: {room, seq}`).
  await bob.goto(`/#chat=${deploys}&msg=${posted.seq}`);
  const msg = bob.locator(`[data-chat-control] [data-msg="${posted.seq}"]`);
  await expect(msg).toBeVisible({ timeout: 15_000 });
  await expect(msg).toContainText("one more");
  // Read, now he's looked: nothing for the badge.
  await expect.poll(async () => (await roomsOf(bob)).find((r) => r.id === deploys)?.mentions).toBe(0);
});
