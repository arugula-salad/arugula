// #429 (shared by chat-agent-session.spec.ts and its WebKit twin): an agent's
// post in chat names the session that wrote it, after "<owner>'s agent on
// <machine>", muted; one with no session names none; a phone doesn't
// overflow. The posts are rows in control's own database (the daemon's
// post_thread is tested in Rust, with two panes), so what is under test is
// what the page makes of the message.

import { spawnSync } from "node:child_process";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, live, show } from "./team-fixture";
import { pixel7 } from "./phones";

export function agentSessionTests(what: string, context: { serviceWorkers?: "block" } = {}) {
  const t = new TeamControl(what);
  let alice: Page;
  let aliceChat: Page;
  let app = "";
  let general = "";

  test.describe.configure({ mode: "serial" });
  test.afterAll(async ({ browser }) => {
    await closeContexts({ browser });
    t.stop();
  });

  /** An agent's message in `room`, as post_thread leaves it: written by `author`'s agent on `machine`. */
  function seed(room: string, author: string, machine: string, text: string, session: string | null) {
    const q = (s: string | null) => (s === null ? "NULL" : `'${s.replaceAll("'", "''")}'`);
    const sql = `UPDATE chat_rooms SET last_seq = last_seq + 1 WHERE id = ${q(room)};
      INSERT INTO chat_messages (room, seq, author, at, text, mentions, agent, agent_session)
      VALUES (${q(room)}, (SELECT last_seq FROM chat_rooms WHERE id = ${q(room)}), ${q(author)}, ${Date.now()}, ${q(text)}, '[]', ${q(machine)}, ${q(session)});`;
    const r = spawnSync("sqlite3", ["-cmd", ".timeout 5000", t.db, sql], { encoding: "utf8" });
    expect(r.status, r.stderr).toBe(0);
  }

  test("a team with chat on, a machine of Alice's, and #general", async ({ browser }) => {
    test.setTimeout(240_000);
    await t.start(["--platform-host", "app.localhost"]);
    app = t.base.replace("//127.0.0.1", "//app.localhost");
    alice = await t.person(browser, "alice", context);
    await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
    const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
    await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
    await t.addMachine(alice, "alices");
    await home(alice);
    await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
    await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
    await show(alice, "alices");
    const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-chat]").click()]);
    aliceChat = opened;
    await aliceChat.locator(`[data-room-new="${team}/channels"]`).click();
    await aliceChat.locator("[data-new-channel-name]").fill("general");
    await aliceChat.locator("[data-new-channel-name]").press("Enter");
    await expect(aliceChat.locator("[data-room-view] .chat-title")).toContainText("general");
    general = (await aliceChat.locator("[data-room-view]").getAttribute("data-room-view"))!;
  });

  test("an agent's post names its session beside its machine, muted; one without names none", async () => {
    test.setTimeout(60_000);
    const machine = await alice.evaluate(() => window.__arugula.fleet.host("alices")!.id!);
    const account = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "alice")!.account);
    seed(general, account, machine, "from the ux session", "UX Improvements");
    seed(general, account, machine, "from a session with no name", null);
    await aliceChat.bringToFront();
    await aliceChat.goto(`${app}/chat/${general}`);
    const posts = aliceChat.locator(".thread-msg.by-agent");
    await expect(posts).toHaveCount(2, { timeout: 15_000 });
    const named = posts.filter({ hasText: "from the ux session" });
    await expect(named.locator(".thread-name")).toHaveText("alice's agent on alices");
    const session = named.locator(".thread-session");
    await expect(session).toHaveText("· UX Improvements");
    // Muted: not the name's colour.
    const colour = (l: typeof session) => l.evaluate((e) => getComputedStyle(e).color);
    expect(await colour(session)).not.toBe(await colour(named.locator(".thread-name")));
    await expect(posts.filter({ hasText: "from a session with no name" }).locator(".thread-session")).toHaveCount(0);
  });

  test("on a phone it still fits, and a long name gives way", async ({ browser }) => {
    test.setTimeout(60_000);
    const machine = await alice.evaluate(() => window.__arugula.fleet.host("alices")!.id!);
    const account = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "alice")!.account);
    seed(general, account, machine, "from a long one", "Refactor the whole of the billing pipeline so every retry is idempotent");
    const phone = await browser.newContext({ ...pixel7, ...context, baseURL: t.base });
    // Alice's browser session is a cookie in her context: share it.
    await phone.addCookies(await alice.context().cookies());
    const page = await phone.newPage();
    await page.goto(`${app}/chat/${general}`);
    const post = page.locator(".thread-msg.by-agent", { hasText: "from a long one" });
    await expect(post.locator(".thread-session")).toContainText("Refactor the whole");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await phone.close();
  });
}
