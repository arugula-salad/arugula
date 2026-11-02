// #502: after `@machine` the sender sees where the mention got to, one quiet
// line under the message. The machine here is a real daemon with no session
// in the room, so its answer is "nobody took it" (the inbox); the other
// routes, an old daemon and an offline machine are control's Rust tests.
// The harness can't post as the machine's agent (control takes that only
// from a signed daemon call), so the agent's reply is written the way
// post_thread leaves it, and the status the reply would set is written
// beside it; the page reads both when it loads.

import { spawnSync } from "node:child_process";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, live, show } from "./team-fixture";

const t = new TeamControl("chat-mention-status");
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

const compose = (p: Page) => p.locator("[data-chat-control] textarea, [data-room-view] textarea").first();

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
      async ([d, p, id]) => (await fetch("/api/grants", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ daemon: d, power: p, scope: { kind: "project", id } }) })).status,
      [machine, power, project] as const,
    );
    expect(status).toBe(200);
  }
});

test("@machine shows a status line under the message, then the agent's reply makes it an answered link", async () => {
  test.setTimeout(120_000);
  await chat.bringToFront();
  await chat.goto(`${app}/chat/${general}`);
  await compose(chat).click();
  await compose(chat).pressSequentially("@alices what is the board for");
  await compose(chat).press("Escape");
  const posted = chat.waitForResponse((r) => r.request().method() === "POST" && r.url().endsWith(`/api/chat/rooms/${general}/messages`));
  await compose(chat).press("Enter");
  const { seq } = (await (await posted).json()) as { seq: number };

  // The daemon has no session to give it to: it waits in the machine's inbox.
  const line = chat.locator(`.mention-line[data-machine="${machine}"]`);
  await expect(line).toBeVisible({ timeout: 15_000 });
  await expect(line).toHaveAttribute("data-mention-status", "waiting");
  await expect(line).toContainText("Waiting on alices: no session took it");
  await expect(line.locator(".machine-icon")).toBeVisible();

  // The agent answers in the thread: the line becomes a link to it.
  const at = Date.now();
  sql(`UPDATE chat_rooms SET last_seq = last_seq + 1 WHERE id = '${general}';
    INSERT INTO chat_messages (room, seq, author, at, text, mentions, agent, parent)
    VALUES ('${general}', (SELECT last_seq FROM chat_rooms WHERE id = '${general}'), '${account}', ${at}, 'the board tracks the launch', '[]', '${machine}', ${seq});
    UPDATE chat_mention_status SET state = 'answered', reply = (SELECT last_seq FROM chat_rooms WHERE id = '${general}')
    WHERE room = '${general}' AND seq = ${seq};`);
  await chat.reload();
  const answered = chat.locator(`.mention-line[data-machine="${machine}"]`);
  await expect(answered).toHaveAttribute("data-mention-status", "answered", { timeout: 15_000 });
  await expect(answered.locator("button.mention-link")).toContainText("alices answered");
  await answered.locator("button.mention-link").click();
  await expect(chat.getByText("the board tracks the launch")).toBeVisible();
});
