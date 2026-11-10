// #646: the strip at the top of a thread. One agent is one line, several are a
// card each, on a phone they fold into one line, and a thread with no agents
// shows nothing. The machine here is a real daemon that has no session of the
// name in the thread, so its agent reads as unknown; a machine control doesn't
// know reads as offline. What a daemon answers is the Rust tests' (#645). The
// harness can't post as a machine's agent (control takes that only from a
// signed daemon call), so posts and statuses are written the way post_thread
// and a mention leave them, as in chat-mention-status.spec.ts.

import { spawnSync } from "node:child_process";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, live, show } from "./team-fixture";

const t = new TeamControl("chat-thread-agents");
let alice: Page;
let chat: Page;
let app = "";
let general = "";
let machine = "";
let account = "";

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

/** A reply by a machine's session, as post_thread leaves it. */
function agentSays(root: number, on: string, session: string, text: string) {
  sql(`UPDATE chat_rooms SET last_seq = last_seq + 1 WHERE id = '${general}';
    INSERT INTO chat_messages (room, seq, author, at, text, mentions, agent, agent_session, parent)
    VALUES ('${general}', (SELECT last_seq FROM chat_rooms WHERE id = '${general}'), '${account}', ${Date.now()}, '${text}', '[]', '${on}', '${session}', ${root});`);
}

/** A person's post, and its number. */
async function post(text: string, parent?: number): Promise<number> {
  return chat.evaluate(
    async ([room, body, p]) => {
      const r = await fetch(`/api/chat/rooms/${room}/messages`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: body, parent: p }),
      });
      return ((await r.json()) as { seq: number }).seq;
    },
    [general, text, parent ?? null] as const,
  );
}

const openThread = async (root: number) => {
  await chat.goto(`${app}/chat/${general}?thread=${root}`);
  await expect(chat.locator(`[data-thread-panel="${root}"]`)).toBeVisible({ timeout: 15_000 });
};

test("a team with chat on, a machine granted the room, and #general", async ({ browser }) => {
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
  chat = opened;
  await chat.locator(`[data-room-new="${team}/channels"]`).click();
  await chat.locator("[data-new-channel-name]").fill("general");
  await chat.locator("[data-new-channel-name]").press("Enter");
  await expect(chat.locator("[data-room-view] .chat-title")).toContainText("general");
  general = (await chat.locator("[data-room-view]").getAttribute("data-room-view"))!;
  machine = await alice.evaluate(() => window.__arugula.fleet.host("alices")!.id!);
  account = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "alice")!.account);
  const project = await chat.evaluate(async (tm) => {
    const v = (await (await fetch("/api/projects")).json()) as { projects: { id: string; team?: { id: string } }[] };
    return v.projects.find((p) => p.team?.id === tm)!.id;
  }, team);
  for (const power of ["threads.read", "threads.post"]) {
    const status = await chat.evaluate(
      async ([d, p, id]) =>
        (await fetch("/api/grants", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ daemon: d, power: p, scope: { kind: "project", id } }) })).status,
      [machine, power, project] as const,
    );
    expect(status).toBe(200);
  }
});

test("no agents, no strip; one agent is a line; several are cards; a phone folds them", async () => {
  test.setTimeout(120_000);
  await chat.bringToFront();
  const root = await post("Reconnect drops after the laptop sleeps");
  await openThread(root);
  const strip = chat.locator("[data-thread-agents]");
  await expect(chat.locator(`[data-thread-panel="${root}"]`)).toContainText("No replies yet");
  await expect(strip).toHaveCount(0);

  // One agent: a line, with its name, whose it is and what it's doing.
  agentSays(root, machine, "views", "On it.");
  const me = await post("Is the token check in scope?", root);
  // A reply to the agent is told to its machine, whose daemon has no session
  // to give it to: it waits.
  await openThread(root);
  const one = chat.locator('[data-thread-agents="one"]');
  await expect(one).toContainText("views", { timeout: 15_000 });
  await expect(one).toContainText("Unknown");
  await expect(one).toContainText("1 waiting");
  await expect(chat.locator("[data-thread-agent]")).toHaveCount(1);

  // Its details say who can stop it and what's shared.
  await one.locator("[data-thread-agent]").click();
  const detail = chat.locator("[data-thread-agent-detail]");
  await expect(detail).toContainText("Your agent on alices. Only you can stop it.");
  await expect(detail).not.toContainText("Never its output");

  // It took the message in: its mark is under it, and nothing waits.
  sql(`UPDATE chat_mention_status SET state = 'working', session = 'views' WHERE room = '${general}' AND seq = ${me};`);
  await openThread(root);
  await expect(chat.locator(`[data-thread-panel="${root}"] [data-thread-seen]`)).toContainText("views", { timeout: 15_000 });
  await expect(one).not.toContainText("waiting");

  // A second agent, on a machine that doesn't answer: a card each, the first
  // the lead, the second offline, and nothing about it made up.
  agentSays(root, "m_gone", "wake", "Looks related.");
  await openThread(root);
  const many = chat.locator('[data-thread-agents="many"]');
  await expect(many.locator("[data-thread-agent]")).toHaveCount(2, { timeout: 15_000 });
  const cards = many.locator("[data-thread-agent]");
  await expect(cards.nth(0)).toContainText("LEAD");
  await expect(cards.nth(1)).toContainText("wake");
  await expect(cards.nth(1)).toContainText("Offline");
  await expect(cards.nth(1)).not.toContainText("LEAD");
  await expect(many.locator(".thread-agent-where")).toHaveCount(0);

  // On a phone they fold into one line, and open again.
  await chat.setViewportSize({ width: 390, height: 780 });
  await openThread(root);
  const sum = chat.locator("[data-thread-agents-sum]");
  await expect(sum).toContainText("2 agents", { timeout: 15_000 });
  await expect(many.locator("[data-thread-agent]")).toHaveCount(0);
  await sum.click();
  await expect(many.locator("[data-thread-agent]")).toHaveCount(2);
});

test("the To row chooses who a reply wakes", async () => {
  test.setTimeout(120_000);
  await chat.setViewportSize({ width: 1180, height: 720 });
  const root = await post("Who has this?");
  await openThread(root);
  await expect(chat.locator("[data-thread-to]")).toHaveCount(0);

  agentSays(root, machine, "views", "I do.");
  agentSays(root, "m_gone", "wake", "Me too.");
  await openThread(root);
  const panel = chat.locator(`[data-thread-panel="${root}"]`);
  const box = panel.locator("textarea");
  const lead = chat.locator(`[data-thread-to-agent="${machine}/views"]`);
  const other = chat.locator('[data-thread-to-agent="m_gone/wake"]');
  const nobody = chat.locator("[data-thread-to-nobody]");
  const line = panel.locator(`.mention-line[data-machine="${machine}"]`);

  // The lead by default, and its status line says it was told.
  await expect(lead).toHaveAttribute("aria-pressed", "true", { timeout: 15_000 });
  await expect(lead).toContainText("lead");
  await expect(other).toContainText("offline");
  await box.fill("over to you");
  await box.press("Enter");
  await expect(line).toHaveCount(1, { timeout: 15_000 });

  // Nobody: posted, and nobody told. It's for that message: the lead is back.
  await nobody.click();
  await expect(lead).toHaveAttribute("aria-pressed", "false");
  await box.fill("just talking");
  await box.press("Enter");
  await expect(panel.getByText("just talking")).toBeVisible();
  await expect(line).toHaveCount(1);
  await expect(lead).toHaveAttribute("aria-pressed", "true");

  // Naming one in the text picks it, for that message, and the chips say so.
  await box.fill("@wake what do you think?");
  await expect(other).toHaveAttribute("aria-pressed", "true");
  await expect(lead).toHaveAttribute("aria-pressed", "false");
  await expect(lead).toBeDisabled();
  await box.fill("");
  await expect(lead).toHaveAttribute("aria-pressed", "true");
});

test("naming a machine that isn't in the thread yet replaces the lead", async () => {
  test.setTimeout(120_000);
  const root = await post("Who has this one?");
  agentSays(root, "m_gone", "wake", "I do.");
  await openThread(root);
  const panel = chat.locator(`[data-thread-panel="${root}"]`);
  const box = panel.locator("textarea");
  const lead = chat.locator('[data-thread-to-agent="m_gone/wake"]');
  const named = chat.locator("[data-thread-to-named]");
  const nobody = chat.locator("[data-thread-to-nobody]");
  await expect(lead).toHaveAttribute("aria-pressed", "true", { timeout: 15_000 });
  await expect(named).toHaveCount(0);

  // The row says whom it wakes: the machine named, not the lead and not Nobody.
  await box.fill("@alices/views have a look");
  await expect(named).toHaveText("views on alices");
  await expect(named).toHaveAttribute("aria-pressed", "true");
  await expect(lead).toHaveAttribute("aria-pressed", "false");
  await expect(lead).toBeDisabled();
  await expect(nobody).toHaveAttribute("aria-pressed", "false");

  // In code it names nobody, and the lead is back.
  await box.fill("`@alices` is the machine");
  await expect(named).toHaveCount(0);
  await expect(lead).toHaveAttribute("aria-pressed", "true");

  // What's sent chooses nobody, and control tells the machine from the text.
  await box.fill("@alices have a look");
  await expect(named).toHaveText("alices");
  const [sent] = await Promise.all([chat.waitForRequest((r) => r.method() === "POST" && r.url().endsWith(`/api/chat/rooms/${general}/messages`)), box.press("Enter")]);
  expect((sent.postDataJSON() as { to: unknown[] }).to).toEqual([]);
  await expect(panel.locator(`.mention-line[data-machine="${machine}"]`)).toHaveCount(1, { timeout: 15_000 });
  await expect(named).toHaveCount(0);
});
