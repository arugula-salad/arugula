// #428 (shared by chat-mention-machines.spec.ts and its WebKit twin): `@` in
// a room lists the machines whose agents hear it, beside the people: each
// with its icon, its owner and an online dot, and its recent sessions under
// it. A machine without a `threads.read` grant over the room isn't listed.
// Picking one posts a message that reaches it (`pushed` has `machine:<id>`),
// and the message shows it as a chip with the machine's icon. The sessions
// are rows in control's own database (post_thread is tested in Rust).

import { spawnSync } from "node:child_process";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, live, show } from "./team-fixture";
import { pixel7 } from "./phones";

export function mentionMachinesTests(what: string, context: { serviceWorkers?: "block" } = {}) {
  const t = new TeamControl(what);
  let alice: Page;
  let aliceChat: Page;
  let app = "";
  let general = "";
  let machine = "";
  let account = "";

  test.describe.configure({ mode: "serial" });
  test.afterAll(async ({ browser }) => {
    await closeContexts({ browser });
    t.stop();
  });

  function seed(room: string, text: string, session: string) {
    const q = (s: string) => `'${s.replaceAll("'", "''")}'`;
    const sql = `UPDATE chat_rooms SET last_seq = last_seq + 1 WHERE id = ${q(room)};
      INSERT INTO chat_messages (room, seq, author, at, text, mentions, agent, agent_session)
      VALUES (${q(room)}, (SELECT last_seq FROM chat_rooms WHERE id = ${q(room)}), ${q(account)}, ${Date.now()}, ${q(text)}, '[]', ${q(machine)}, ${q(session)});`;
    const r = spawnSync("sqlite3", ["-cmd", ".timeout 5000", t.db, sql], { encoding: "utf8" });
    expect(r.status, r.stderr).toBe(0);
  }

  const compose = (p: Page) => p.locator("[data-chat-control] textarea, [data-room-view] textarea").first();

  test("a team with chat on, two machines of Alice's, one granted the room, and #general", async ({ browser }) => {
    test.setTimeout(300_000);
    await t.start(["--platform-host", "app.localhost"]);
    app = t.base.replace("//127.0.0.1", "//app.localhost");
    alice = await t.person(browser, "alice", context);
    await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
    const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
    await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
    await t.addMachine(alice, "alices");
    await t.addMachine(alice, "quiet");
    await home(alice);
    await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
    await expect.poll(() => live(alice, ["alices", "quiet"]), { timeout: 30_000 }).toBe(true);
    await show(alice, "alices");
    const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-chat]").click()]);
    aliceChat = opened;
    await aliceChat.locator(`[data-room-new="${team}/channels"]`).click();
    await aliceChat.locator("[data-new-channel-name]").fill("general");
    await aliceChat.locator("[data-new-channel-name]").press("Enter");
    await expect(aliceChat.locator("[data-room-view] .chat-title")).toContainText("general");
    general = (await aliceChat.locator("[data-room-view]").getAttribute("data-room-view"))!;
    machine = await alice.evaluate(() => window.__arugula.fleet.host("alices")!.id!);
    account = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "alice")!.account);
    const project = await aliceChat.evaluate(async (tm) => {
      const v = (await (await fetch("/api/projects")).json()) as { projects: { id: string; team?: { id: string } }[] };
      return v.projects.find((p) => p.team?.id === tm)!.id;
    }, team);
    // Only alices is granted the room; quiet is Alice's too, with no grant.
    for (const power of ["threads.read", "threads.post"]) {
      const status = await aliceChat.evaluate(
        async ([d, p, id]) => (await fetch("/api/grants", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ daemon: d, power: p, scope: { kind: "project", id } }) })).status,
        [machine, power, project] as const,
      );
      expect(status).toBe(200);
    }
    seed(general, "from the ux session", "UX Improvements");
    seed(general, "from billing", "Billing");
  });

  test("@ lists the granted machine with its icon, owner, dot and sessions; the other isn't there", async () => {
    test.setTimeout(60_000);
    await aliceChat.bringToFront();
    await aliceChat.goto(`${app}/chat/${general}`);
    await compose(aliceChat).click();
    await compose(aliceChat).pressSequentially("hello @");
    const pick = aliceChat.locator(".mention-pick");
    const row = pick.locator("[data-mention-machine='alices']");
    await expect(row).toBeVisible({ timeout: 15_000 });
    await expect(row.locator(".machine-icon")).toBeVisible();
    await expect(row).toContainText("alices");
    await expect(row).toContainText("yours");
    await expect(row.locator("[data-presence]")).toHaveAttribute("data-presence", "online", { timeout: 20_000 });
    await expect(pick.locator("[data-mention-session='alices/ux-improvements']")).toBeVisible();
    await expect(pick.locator("[data-mention-session='alices/billing']")).toBeVisible();
    // A machine with no grant over the room isn't offered.
    await expect(pick.locator("[data-mention-machine='quiet']")).toHaveCount(0);
    // Typing filters machines and sessions alike.
    await compose(aliceChat).pressSequentially("alices/ux");
    await expect(pick.locator("[data-mention-session]")).toHaveCount(1);
    await expect(pick.locator("[data-mention-session='alices/ux-improvements']")).toBeVisible();
  });

  test("picking a session inserts @machine/session; the message reaches the machine and shows a chip", async () => {
    test.setTimeout(60_000);
    const pick = aliceChat.locator(".mention-pick");
    await pick.locator("[data-mention-session='alices/ux-improvements']").dispatchEvent("mousedown");
    await expect(compose(aliceChat)).toHaveValue("hello @alices/ux-improvements ");
    await compose(aliceChat).pressSequentially("what is the board for");
    const posted = aliceChat.waitForResponse((r) => r.request().method() === "POST" && r.url().endsWith(`/api/chat/rooms/${general}/messages`));
    await compose(aliceChat).press("Enter");
    const body = (await (await posted).json()) as { pushed: { account: string }[]; mentions: string[] };
    expect(body.mentions).toContain(`machine:${machine}`);
    expect(body.pushed.map((p) => p.account)).toContain(`machine:${machine}`);
    const chip = aliceChat.locator(".thread-msg .mention-machine", { hasText: "@alices/ux-improvements" });
    await expect(chip).toBeVisible();
    await expect(chip.locator(".machine-icon")).toBeVisible();
  });

  test("on a phone the picker fits the screen", async ({ browser }) => {
    test.setTimeout(60_000);
    const phone = await browser.newContext({ ...pixel7, ...context, baseURL: t.base });
    await phone.addCookies(await alice.context().cookies());
    const page = await phone.newPage();
    await page.goto(`${app}/chat/${general}`);
    await compose(page).click();
    await compose(page).pressSequentially("@");
    const pick = page.locator(".mention-pick");
    await expect(pick.locator("[data-mention-machine='alices']")).toBeVisible({ timeout: 15_000 });
    await expect(pick.locator("[data-mention-session]")).toHaveCount(2);
    const box = (await pick.boundingBox())!;
    const width = await page.evaluate(() => window.innerWidth);
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(width);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await phone.close();
  });
}
