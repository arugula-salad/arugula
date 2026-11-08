// #632: an agent's message in chat. Its avatar carries the agent icon and its
// name is its tab, `machine/tab`, in teal. On the reader's own machine the name
// opens that pane; on a machine they don't reach it is plain text and its
// tooltip says whose agent it is. A reaction chip counts people first, then a
// divider and the agent icon with the agents' count. Posts and reactions are
// written the way post_thread and an agent's react leave them (the harness
// can't sign as a machine, as in chat-thread-agents.spec.ts).

import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const t = new TeamControl("chat-agent-marks");
let alice: Page;
let chatPage: Page;
let app = "";
let general = "";
let machine = "";
let account = "";
let terminal = 0;
let views = 0;
let chatBlock = 0;

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

function sql(text: string) {
  const r = spawnSync("sqlite3", ["-cmd", ".timeout 5000", t.db, text], { encoding: "utf8" });
  expect(r.status, r.stderr).toBe(0);
  return r.stdout.trim();
}

/** A top-level post by a machine's session, as post_thread leaves it. */
function agentSays(on: string, session: string, text: string): number {
  sql(`UPDATE chat_rooms SET last_seq = last_seq + 1 WHERE id = '${general}';
    INSERT INTO chat_messages (room, seq, author, at, text, mentions, agent, agent_session)
    VALUES ('${general}', (SELECT last_seq FROM chat_rooms WHERE id = '${general}'), '${account}', ${Date.now()}, '${text}', '[]', '${on}', '${session}');`);
  return Number(sql(`SELECT last_seq FROM chat_rooms WHERE id = '${general}'`));
}

function react(seq: number, emoji: string, agent: string) {
  sql(`INSERT INTO chat_reactions (room, seq, emoji, account, agent, at) VALUES ('${general}', ${seq}, '${emoji}', '${account}', '${agent}', ${Date.now()});`);
}

/** Rows written to the database don't come on the live stream: reload to read them. */
async function reloaded() {
  // PROOF_PALETTE: the proof shots in Latte.
  if (process.env.PROOF_PALETTE) await alice.evaluate((palette) => localStorage.setItem("arugula-appearance", JSON.stringify({ palette, finish: "flat", corners: "soft" })), process.env.PROOF_PALETTE);
  await alice.reload();
  // The name's click left the reader on the pane it opened: back to the chat's.
  await expect.poll(() => alice.evaluate((p) => !!window.__arugula?.client?.info(p), terminal), { timeout: 30_000 }).toBe(true);
  await alice.evaluate((p) => window.__arugula.client.setActive(p), terminal);
  await expect(alice.locator(`[data-chat-block="${chatBlock}"]`)).toBeVisible({ timeout: 30_000 });
}

/** Where a proof screenshot goes, when PROOF_DIR is set. */
async function shot(page: Page, name: string, on: ReturnType<Page["locator"]>) {
  const dir = process.env.PROOF_DIR;
  if (!dir) return;
  mkdirSync(dir, { recursive: true });
  await on.screenshot({ path: `${dir}/${name}${process.env.PROOF_PALETTE ? `-${process.env.PROOF_PALETTE}` : ""}.png` });
}

test("setup: #general, a pane called views, and a team-chat block beside it", async ({ browser }) => {
  test.setTimeout(300_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await t.addMachine(alice, "alices");
  await home(alice);
  await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
  await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "alices");
  const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-chat]").click()]);
  chatPage = opened;
  await chatPage.locator(`[data-room-new="${team}/channels"]`).click();
  await chatPage.locator("[data-new-channel-name]").fill("general");
  await chatPage.locator("[data-new-channel-name]").press("Enter");
  await expect(chatPage.locator("[data-room-view] .chat-title")).toContainText("general");
  general = (await chatPage.locator("[data-room-view]").getAttribute("data-room-view"))!;
  machine = await alice.evaluate(() => window.__arugula.fleet.host("alices")!.id!);
  account = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "alice")!.account);

  // The tab the agent's name will open: a pane titled "views", and another
  // that's active, so the click has somewhere to go.
  const other = await call(alice, "POST", "/api/run", { command: "sleep 600" });
  terminal = (other.body as { pane: number }).pane;
  const titled = await call(alice, "POST", "/api/run", { command: "printf '\\033]0;views\\007'; sleep 600" });
  views = (titled.body as { pane: number }).pane;
  const block = await call(alice, "POST", "/api/blocks", { type: "team_chat", config: { room: general, pinned: true }, split: terminal, local: true });
  expect(block.status, JSON.stringify(block.body)).toBe(200);
  chatBlock = (block.body as { block: number }).block;
  await alice.bringToFront();
  await alice.evaluate((p) => window.__arugula.client.setActive(p), terminal);
  await expect(alice.locator(`[data-chat-block="${chatBlock}"]`)).toBeVisible({ timeout: 15_000 });
});

test("an agent on your machine: icon on the avatar, machine/tab in teal, a click opens the pane", async () => {
  test.setTimeout(120_000);
  await expect.poll(() => alice.evaluate((p) => !!window.__arugula.client.info(p)?.title?.includes("views"), views), { timeout: 20_000 }).toBe(true);
  const own = agentSays(machine, "views", "Looking at it.");
  const theirs = agentSays("m_elsewhere", "wake", "Looks related.");
  await reloaded();
  const block = alice.locator(`[data-chat-block="${chatBlock}"]`);
  const mine = block.locator(`[data-msg="${own}"]`);
  await expect(mine).toBeVisible({ timeout: 20_000 });

  // The avatar's mark, not an AGENT badge or a "· session" span.
  await expect(mine.locator("[data-agent-mark] .machine-icon")).toBeVisible();
  await expect(mine.locator(".chip, .thread-session")).toHaveCount(0);
  const name = mine.locator("[data-agent-name]");
  await expect(name).toHaveText("alices/views");
  await expect(name).toHaveAttribute("data-opens", "1");
  await expect(name).toHaveAttribute("title", /alice's agent, on alices/);
  await shot(alice, "message-own", mine);

  // Another's agent, on a machine this reader doesn't reach: not a link.
  const other = block.locator(`[data-msg="${theirs}"]`);
  await other.scrollIntoViewIfNeeded();
  await expect(other).toBeVisible();
  const plain = other.locator("[data-agent-name]");
  await expect(plain).toHaveText("a machine that left/wake");
  await expect(plain).not.toHaveAttribute("data-opens", "1");
  await expect(other.locator("button[data-agent-name]")).toHaveCount(0);
  await expect(plain).toHaveAttribute("title", /agent, on a machine that left/);
  await shot(alice, "message-theirs", other);

  // Clicking it opens that pane.
  await name.click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.client.activePane.get(window.__arugula.client.tab!)), { timeout: 10_000 }).toBe(views);

});

test("a reaction counts people, then agents; agents alone show the icon and count", async () => {
  test.setTimeout(60_000);
  const seq = agentSays(machine, "views", "React to this.");
  react(seq, "👀", "");
  react(seq, "👀", machine);
  react(seq, "🎉", machine);
  await reloaded();
  const block = alice.locator(`[data-chat-block="${chatBlock}"]`);
  const msg = block.locator(`[data-msg="${seq}"]`);
  const both = msg.locator('[data-reaction="👀"]');
  await both.scrollIntoViewIfNeeded({ timeout: 20_000 });
  await expect(both).toBeVisible();
  await expect(both.locator(".reaction-count")).toHaveText("1");
  await expect(both.locator(".reaction-sep")).toHaveCount(1);
  await expect(both.locator(".reaction-agents")).toHaveText("1");
  await expect(both.locator(".reaction-agents .machine-icon")).toBeVisible();
  const agents = msg.locator('[data-reaction="🎉"]');
  await expect(agents.locator(".reaction-count")).toHaveCount(0);
  await expect(agents.locator(".reaction-sep")).toHaveCount(0);
  await expect(agents.locator(".reaction-agents")).toHaveText("1");
  await shot(alice, "reaction", msg.locator(".reactions"));

  // Clicking still toggles your own, against the people's count: Alice's is
  // the one person, so it goes and the agent's stays; again, and it's back.
  await expect(both).toHaveClass(/mine/);
  await both.click();
  await expect(both.locator(".reaction-count")).toHaveCount(0, { timeout: 10_000 });
  await expect(both.locator(".reaction-agents")).toHaveText("1");
  await both.click();
  await expect(both.locator(".reaction-count")).toHaveText("1", { timeout: 10_000 });
  await expect(both.locator(".reaction-agents")).toHaveText("1");
});

test("at a phone's width the name ellipsizes and the badge leaves the initials clear", async () => {
  test.setTimeout(60_000);
  const seq = agentSays(machine, "a-session-with-a-very-long-tab-name-that-cannot-fit-on-a-phone", "Short answer.");
  await chatPage.setViewportSize({ width: 390, height: 800 });
  await chatPage.goto(`${app}/chat/${general}`);
  const msg = chatPage.locator(`[data-msg="${seq}"]`);
  await msg.scrollIntoViewIfNeeded({ timeout: 20_000 });
  const name = msg.locator("[data-agent-name]");
  await expect(name).toBeVisible();
  const box = (await name.boundingBox())!;
  expect(box.x + box.width, "the name stays on the screen").toBeLessThanOrEqual(390);
  expect(await name.evaluate((n) => n.scrollWidth > n.clientWidth), "and is cut with an ellipsis").toBe(true);
  // The badge sits on the avatar's corner, short of its middle where the letters are.
  const face = (await msg.locator(".msg-avatar").boundingBox())!;
  const badge = (await msg.locator("[data-agent-mark]").boundingBox())!;
  expect(badge.x + badge.width).toBeLessThan(face.x + face.width / 2);
  await shot(chatPage, "message-phone", msg);
});

/** What `--teal` paints, as the browser resolves a colour: a probe given `var(--teal)`. */
const teal = (page: Page) =>
  page.evaluate(() => {
    const probe = document.createElement("i");
    probe.style.cssText = "color:var(--teal);background:var(--teal)";
    document.body.append(probe);
    const c = getComputedStyle(probe);
    const out = { color: c.color, background: c.backgroundColor };
    probe.remove();
    return out;
  });

/** The name and the reaction's agent marks are painted `--teal` in Latte and Slate, as a link and as text. */
for (const palette of ["latte", "default"]) {
  test(`${palette}: the agent's name, link or text, and the chip's icon and divider are the palette's teal`, async () => {
    test.setTimeout(90_000);
    await alice.evaluate((p) => localStorage.setItem("arugula-appearance", JSON.stringify({ palette: p, finish: "flat", corners: "soft" })), palette);
    await reloaded();
    if (palette === "latte") await expect(alice.locator("html")).toHaveAttribute("data-palette", "latte");
    const block = alice.locator(`[data-chat-block="${chatBlock}"]`);
    const want = await teal(alice);
    const colour = (l: ReturnType<Page["locator"]>) => l.first().evaluate((n) => ({ color: getComputedStyle(n).color, background: getComputedStyle(n).backgroundColor }));
    // The link (your own machine's agent), and the text (one you can't open).
    expect((await colour(block.locator("button[data-agent-name]"))).color, "the name as a link").toBe(want.color);
    expect((await colour(block.locator("span[data-agent-name]"))).color, "the name as text").toBe(want.color);
    expect((await colour(block.locator(".reaction-agents"))).color, "the agents' count").toBe(want.color);
    expect((await colour(block.locator(".reaction-sep"))).background, "the divider").toBe(want.background);
    expect((await colour(block.locator("[data-agent-mark]"))).color, "the avatar's icon").toBe(want.color);
    const dir = process.env.PROOF_DIR;
    if (dir && palette === "latte") await block.locator('[data-agent-name]').first().locator("xpath=ancestor::*[@data-msg][1]").screenshot({ path: `${dir}/message-own-latte-teal.png` });
  });
}
