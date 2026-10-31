// #438 (shared by chat-mention-chips.spec.ts and its WebKit twin): a mention
// is drawn as missed (dashed, "This reached no one") only when it reached
// nobody. Alice mentions Bob in #general: Alice, Bob and Carol, a third
// party, all see a normal chip, and Bob's is highlighted as his own. A
// mention of someone who isn't in the room is dashed for everyone.

import { expect, test, type Browser, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

export function mentionChipsTests(what: string, context: { serviceWorkers?: "block" } = {}) {
  const t = new TeamControl(what);
  let alice: Page;
  let bob: Page;
  let carol: Page;
  let app = "";
  let general = "";

  test.describe.configure({ mode: "serial" });
  test.afterAll(async ({ browser }) => {
    await closeContexts({ browser });
    t.stop();
  });

  async function join(browser: Browser, login: string, team: string): Promise<Page> {
    const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
    const p = await t.person(browser, login, context);
    await p.goto(link);
    await p.locator("[data-accept-invite]").click();
    await expect(p.locator("[data-invite-pending]")).toBeVisible();
    await alice.evaluate(() => window.__arugula.control!.refresh());
    await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
    await alice.locator("[data-admit-yes]").click();
    return p;
  }

  const account = (login: string) =>
    alice.evaluate((l) => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === l)!.account, login);

  test("a team with Alice, Bob and Carol, and #general with all three", async ({ browser }) => {
    test.setTimeout(300_000);
    await t.start(["--platform-host", "app.localhost"]);
    app = t.base.replace("//127.0.0.1", "//app.localhost");
    alice = await t.person(browser, "alice", context);
    await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
    const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
    await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
    bob = await join(browser, "bob", team);
    await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
    carol = await join(browser, "carol", team);
    await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(3);
    await home(alice);
    await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
    // Control's page has `window.__arugula`; the platform page below doesn't.
    const members = [await account("bob"), await account("carol")];
    await alice.goto(`${app}/chat`);
    await alice.locator(`[data-room-new="${team}/channels"]`).click();
    await alice.locator("[data-new-channel-name]").fill("general");
    await alice.locator("[data-new-channel-name]").press("Enter");
    await expect(alice.locator("[data-room-view] .chat-title")).toContainText("general");
    general = (await alice.locator("[data-room-view]").getAttribute("data-room-view"))!;
    const status = await alice.evaluate(
      async ([room, accounts]) => (await fetch(`/api/chat/rooms/${room}/members`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ accounts }) })).status,
      [general, members] as const,
    );
    expect(status).toBe(200);
  });

  test("Alice mentions Bob and a stranger: Alice, Bob and Carol see Bob's chip normal, the stranger's dashed", async () => {
    test.setTimeout(90_000);
    await alice.bringToFront();
    const box = alice.locator("[data-room-view] textarea").first();
    await box.fill("@bob and @ghost please look");
    await box.press("Enter");
    for (const [who, page] of [["alice", alice], ["bob", bob], ["carol", carol]] as const) {
      await page.bringToFront();
      await page.goto(`${app}/chat/${general}`);
      const msg = page.locator(".thread-msg", { hasText: "please look" });
      await expect(msg, who).toBeVisible({ timeout: 20_000 });
      // The mention of Bob reached him: no dashed chip, whoever looks.
      await expect(msg.locator(".mention-missed", { hasText: "@bob" }), who).toHaveCount(0);
      await expect(msg.locator(".mention", { hasText: "@bob" }), who).toHaveCount(1);
      // Only Bob sees it as his own.
      await expect(msg.locator(".mention.me", { hasText: "@bob" }), who).toHaveCount(who === "bob" ? 1 : 0);
      // @ghost is in no one's room: dashed for everyone.
      await expect(msg.locator(".mention-missed", { hasText: "@ghost" }), who).toHaveCount(1);
    }
  });
}
