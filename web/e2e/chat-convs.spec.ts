// Conversations (#855): threads of their own, kept on their home machine,
// with members of their own. On the chat page they're *Channels*, after
// Activity and above the machines. The owner makes one (*New
// conversation*), posts, and adds a second person (a tailnet user a session
// here is shared with, as a watcher), seeing what being a member means; the
// second person, in their own browser, finds it under Channels with what's
// unread, reads it, sees the same sentence on its members list, and posts;
// each sees the other's message come in. Leaving takes it off their list.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, open } from "./helpers";
import { ANY, daemonPort } from "./ports";
import { labs } from "./labs";

const OWNER = "me@example.com";
const FRIEND = "friend@example.com";
const SEE = "Members can have the lead type into and read every linked pane. Invite only people you'd hand those terminals to.";

let base = "";
let daemon: ChildProcess;
let dir: string;

test.describe.configure({ mode: "serial" });
test.use({ baseURL: async ({}, use) => use(base) });
test.afterAll(closeContexts);

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-convs-"));
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", labs(dir), "--owner", OWNER],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
    ],
    { stdio: "ignore" },
  );
  base = `http://127.0.0.1:${await daemonPort(dir, daemon)}`;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${base}/api/host`)).ok) break;
    } catch {
      // not yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  // The friend reaches this machine: the session is shared with them, to
  // watch. In the conversation they'll post all the same.
  const panes = (await (await fetch(`${base}/api/panes`)).json()) as { session: number }[];
  const r = await fetch(`${base}/api/acl`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ session: panes[0].session, principal: `tailnet:${FRIEND}`, role: "viewer" }),
  });
  expect(r.ok).toBe(true);
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  rmSync(dir, { recursive: true, force: true });
});

let owner: Page;
let friend: Page;
let id = "";

test("Channels sits after Activity, above the machines, and makes a conversation", async ({ browser }) => {
  owner = await (await browser.newContext()).newPage();
  await open(owner);
  await owner.locator("[data-open-chat]").click();
  const chat = owner.locator("[data-chat]");
  const list = chat.locator(".chat-list");
  await expect(list.locator("[data-chat-channels]")).toContainText("No conversations yet.");
  // In order: Activity, Channels, then the machine's sessions.
  const order = await list.evaluate((el) =>
    [...el.querySelectorAll("[data-chat-activity], [data-chat-channels], [data-chat-machine]")].map((x) =>
      x.hasAttribute("data-chat-activity") ? "activity" : x.hasAttribute("data-chat-channels") ? "channels" : "machine",
    ),
  );
  expect(order).toEqual(["activity", "channels", "machine"]);

  await list.locator("[data-conv-new]").click();
  const dialog = owner.locator("[data-conv-dialog]");
  await dialog.locator("[data-conv-title]").fill("calc tests");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(dialog).toHaveCount(0);

  // It opens, and it's a channel.
  const row = list.locator("[data-chat-channels] .chat-row");
  await expect(row).toHaveText(/calc tests/);
  id = await owner.evaluate(() => window.__arugula.client.state!.convs![0].id);
  expect(id).toMatch(/^c-[0-9a-f]{32}$/);
  await expect(row).toHaveAttribute("data-chat-thread", `conv-${id}`);
  expect(await owner.evaluate(() => location.hash)).toMatch(new RegExp(`^#chat=[^/]*/conv-${id}$`));
  await expect(chat.locator(".chat-thread .chat-title")).toContainText("calc tests");
  await expect(chat.locator("[data-chat-topic]")).toHaveText("just you");
});

test("the owner posts, and adds someone after reading what members can do", async () => {
  const chat = owner.locator("[data-chat]");
  await chat.locator(".chat-thread textarea").fill("tests fail on main");
  await chat.locator(".chat-thread textarea").press("Enter");
  await expect(chat.locator(".chat-thread .thread-list")).toContainText("tests fail on main");

  // The members list says it, and so does adding someone.
  await chat.locator("[data-conv-people]").click();
  const details = chat.locator("[data-chat-details-panel]");
  await expect(details.locator("[data-conv-warning]")).toHaveText(SEE);
  await details.locator("[data-conv-add]").click();
  const dialog = owner.locator("[data-conv-dialog]");
  await expect(dialog.locator("[data-conv-warning]")).toHaveText(SEE);

  // Someone who can't reach the machine is refused, and says why.
  await dialog.locator("[data-conv-who]").fill("@nobody");
  await dialog.getByRole("button", { name: "Add" }).click();
  await expect(dialog.locator(".thread-error")).toContainText("can reach this machine");
  await dialog.locator("[data-conv-who]").fill("@friend");
  await dialog.locator("[data-conv-history]").check();
  await dialog.getByRole("button", { name: "Add" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(details.locator(`[data-conv-member="tailnet:${FRIEND}"]`)).toContainText("since the start");
  await expect(chat.locator("[data-chat-topic]")).toHaveText("2 people");
});

test("the second person finds it under Channels, reads the warning, and both post", async ({ browser }) => {
  friend = await (await browser.newContext({ extraHTTPHeaders: { "tailscale-user-login": FRIEND } })).newPage();
  await open(friend);
  await friend.locator("[data-open-chat]").click();
  const chat = friend.locator("[data-chat]");
  const row = chat.locator(`[data-chat-channels] [data-chat-thread="conv-${id}"]`);
  // Added with history: the owner's message is unread for them.
  await expect(row.locator(".chat-count")).toHaveText("1");
  await row.click();
  await expect(chat.locator(".chat-thread .thread-list")).toContainText("tests fail on main");
  await chat.locator("[data-conv-people]").click();
  await expect(chat.locator("[data-chat-details-panel] [data-conv-warning]")).toHaveText(SEE);
  // A member doesn't add people or delete it; they may leave.
  await expect(chat.locator("[data-conv-add]")).toHaveCount(0);
  await expect(chat.locator(`[data-conv-remove="tailnet:${FRIEND}"]`)).toHaveText("Leave");

  // A watcher on the session, yet they post here.
  await chat.locator(".chat-thread textarea").fill("on it, @me");
  await chat.locator(".chat-thread textarea").press("Enter");
  await expect(chat.locator(".chat-thread .thread-list")).toContainText("on it");
  await expect(owner.locator("[data-chat] .chat-thread .thread-list")).toContainText("on it");

  await owner.locator("[data-chat] .chat-thread textarea").fill("thanks");
  await owner.locator("[data-chat] .chat-thread textarea").press("Enter");
  await expect(chat.locator(".chat-thread .thread-list")).toContainText("thanks");
});

test("leaving takes it off their list; the owner's stays", async () => {
  const chat = friend.locator("[data-chat]");
  await chat.locator(`[data-conv-remove="tailnet:${FRIEND}"]`).click();
  await expect(chat.locator(`[data-chat-thread="conv-${id}"]`)).toHaveCount(0);
  await expect(owner.locator(`[data-conv-member="tailnet:${FRIEND}"]`)).toHaveCount(0);
  await expect(owner.locator(`[data-chat-channels] [data-chat-thread="conv-${id}"]`)).toBeVisible();
});

test("a conversation's notification opens it on the chat page", async ({ browser }) => {
  // What the service worker opens for a mention in it, or being added.
  const page = await (await browser.newContext()).newPage();
  await page.goto(`/#chat=conv-${id}`);
  const chat = page.locator("[data-chat]");
  await expect(chat.locator(`[data-conv="${id}"] .chat-title`)).toContainText("calc tests");
  await expect(chat.locator(".chat-thread .thread-list")).toContainText("tests fail on main");
  expect(await page.evaluate(() => location.hash)).toMatch(new RegExp(`^#chat=[^/]*/conv-${id}$`));
});
