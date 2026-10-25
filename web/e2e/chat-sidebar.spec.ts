// The chat sidebar's sections and stars (#120), and the agents chip (#122):
// Alice and Bob in team Acme with chat on, Alice with a machine (alices).
// Alice stars #general, makes a section "Ops" and puts #ops and her DM with
// Bob in it (one by drag, one from the room's menu); Bob's phone lists them
// in the same order, and Ops stays collapsed there once she collapses it.
// Then Alice gives her machine's agents threads.read and threads.post over
// the team's project: #general's header shows the chip, not the lines, its
// popover lists the machine and what it may do, and the room says so once.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, live, show } from "./team-fixture";
import { pixel7 } from "./phones";

const t = new TeamControl("chatsb");

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
let ops = "";
let dm = "";
let bobAccount = "";
let section = "";

async function platform(p: Page, path: string) {
  // In front: a page behind another of its browser's gets no frames, so what
  // waits for one (a render, a caret) waits until something else draws.
  await p.bringToFront();
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-chat-control]")).toBeVisible({ timeout: 15_000 });
}

/// The ids of the rooms in a group, top to bottom.
const rooms = (p: Page, group: string) => p.locator(`[data-chat-group="${group}"] [data-chat-room]`).evaluateAll((els) => els.map((e) => e.getAttribute("data-chat-room")));
/// The groups, top to bottom.
const groupsOf = (p: Page) => p.locator("[data-chat-group]").evaluateAll((els) => els.map((e) => e.getAttribute("data-chat-group")));

test("a team with chat on, a machine of Alice's, and three rooms", async ({ browser }) => {
  test.setTimeout(240_000);
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
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await t.addMachine(alice, "alices");
  for (const p of [alice, bob]) {
    await home(p);
    await expect.poll(() => p.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
  }
  await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "alices");

  const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-chat]").click()]);
  aliceChat = opened;
  const page = aliceChat.locator("[data-chat-control]");
  for (const name of ["general", "ops"]) {
    await page.locator(`[data-room-new="${team}/channels"]`).click();
    await aliceChat.locator("[data-new-channel-name]").fill(name);
    await aliceChat.locator("[data-new-channel-name]").press("Enter");
    await expect(page.locator("[data-room-view] .chat-title")).toContainText(name);
    const id = (await page.locator("[data-room-view]").getAttribute("data-room-view"))!;
    // A channel is its members' (#118): Bob is added to see it.
    await aliceChat.evaluate(
      async ([room, who]) => {
        const r = await fetch(`/api/chat/rooms/${room}/members`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ accounts: [who] }) });
        if (!r.ok) throw new Error(`adding: ${r.status}`);
      },
      [id, bobAccount] as const,
    );
    if (name === "general") general = id;
    else ops = id;
  }
  await page.locator(`[data-room-new="${team}/dms"]`).click();
  await aliceChat.locator("[data-new-dm] li", { hasText: "bob" }).locator("input").check();
  await aliceChat.locator("[data-dm-start]").click();
  dm = (await page.locator("[data-room-view]").getAttribute("data-room-view"))!;
});

test("Alice stars #general, makes Ops and fills it by drag and by menu", async () => {
  await platform(aliceChat, `/chat/${general}`);
  const page = aliceChat.locator("[data-chat-control]");
  // Starred leads the sidebar; #general is no longer under Channels.
  await page.locator("[data-room-star]").click();
  await expect.poll(() => groupsOf(aliceChat)).toEqual(["starred", `${team}/channels`, `${team}/dms`]);
  expect(await rooms(aliceChat, "starred")).toEqual([general]);
  expect(await rooms(aliceChat, `${team}/channels`)).toEqual([ops]);

  await page.locator("[data-chat-workspace]").click();
  await aliceChat.getByRole("menuitem", { name: "New section…" }).click();
  await aliceChat.locator(".prompt input").fill("Ops");
  await aliceChat.locator(".prompt input").press("Enter");
  await expect(page.locator("[data-chat-group] h2", { hasText: "Ops" })).toBeVisible();
  section = (await groupsOf(aliceChat)).filter((g) => g && !g.includes("/") && g !== "starred")[0]!;
  // By drag: #ops onto the section. By menu: the DM.
  await page.locator(`[data-chat-room="${ops}"]`).dragTo(page.locator(`[data-chat-group="${section}"]`));
  await page.locator(`[data-chat-room="${dm}"]`).click({ button: "right" });
  await aliceChat.getByRole("menuitemradio", { name: "Ops" }).click();
  await expect.poll(() => rooms(aliceChat, section)).toEqual(expect.arrayContaining([ops, dm]));
  expect(await rooms(aliceChat, `${team}/channels`)).toEqual([]);
  expect(await groupsOf(aliceChat)).toEqual(["starred", section, `${team}/channels`, `${team}/dms`]);

  // Collapsed, and it stays so.
  await page.locator(`[data-chat-group="${section}"] h2 button[aria-expanded]`).first().click();
  await expect(page.locator(`[data-chat-group="${section}"] [data-chat-room]`)).toHaveCount(0);
});

test("Bob's sidebar is his own, and his phone stars a room", async () => {
  await platform(bob, "/chat");
  const list = bob.locator("[data-chat-control]");
  // Alice's star and section are hers: Bob has the built-in groups.
  await expect.poll(() => groupsOf(bob)).toEqual([`${team}/channels`, `${team}/dms`]);
  // From the room's header on the phone (it has no drag).
  await list.locator(`[data-chat-room="${general}"]`).click();
  await list.locator("[data-room-star]").click();
  await expect(list.locator("[data-room-star]")).toHaveAttribute("aria-pressed", "true");
  // Back to the list (on a phone the room has the screen).
  await platform(bob, "/chat");
  await expect.poll(() => groupsOf(bob)).toEqual(["starred", `${team}/channels`, `${team}/dms`]);
  await bob.reload();
  await expect(list.locator("[data-chat-group]").first()).toBeVisible({ timeout: 15_000 });
  expect(await rooms(bob, "starred")).toEqual([general]);
  // The phone does not overflow sideways.
  expect(await bob.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("an agent grant: the room shows a chip and says so once", async () => {
  test.setTimeout(60_000);
  const machine = await alice.evaluate(() => window.__arugula.fleet.host("alices")!.id!);
  const project = await aliceChat.evaluate(async (tm) => {
    const r = await fetch("/api/projects");
    const v = (await r.json()) as { projects: { id: string; team?: { id: string } }[] };
    return v.projects.find((p) => p.team?.id === tm)!.id;
  }, team);
  const give = (power: string) =>
    aliceChat.evaluate(
      async ([d, p, id]) => (await fetch("/api/grants", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ daemon: d, power: p, scope: { kind: "project", id } }) })).status,
      [machine, power, project] as const,
    );
  expect(await give("threads.read")).toBe(200);
  expect(await give("threads.post")).toBe(200);
  expect(await give("threads.post")).toBe(200);
  await platform(aliceChat, `/chat/${general}`);
  const page = aliceChat.locator("[data-chat-control]");
  // The chip in the header, not a line across the top.
  await expect(page.locator("[data-agents-chip]")).toContainText("1 machine's agents");
  await expect(page.locator(".room-agents")).toHaveCount(0);
  await page.locator("[data-agents-chip]").click();
  await expect(page.locator("[data-agents-pop] [data-agents-machine]")).toContainText("read and post");
  await expect(page.locator("[data-agents-grants]")).toBeVisible();
  // Said in the room: once for read, once for read and post; not again.
  const said = page.locator("[data-system]");
  await expect(said).toHaveCount(2);
  await expect(said.last()).toContainText("can now read and post here");
});
