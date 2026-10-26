// #1473: a guest who opens a follow invite starts following the inviter's
// window: the invite names the person (`owner`), and the guest's page picks
// the window from presence. A friend (a second principal, a header on
// loopback as `tailscale serve` adds it) opens the invite's address, which
// is what a notification opens (`&follow=`), while the owner is on another
// tab than the pane's.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, open, ready, tab } from "./helpers";
import { ANY, daemonPort } from "./ports";
import { labs } from "./labs";

test.afterAll(closeContexts);

let base = "";
const OWNER = "me@example.com";
const FRIEND = "friend@example.com";
let daemon: ChildProcess;
let dir: string;

test.use({ baseURL: async ({}, use) => use(base) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-follow-invite-"));
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
      if ((await fetch(`${base}/api/host`)).ok) return;
    } catch {
      // not yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  rmSync(dir, { recursive: true, force: true });
});

const tabId = async (p: Page) => (await tab(p)).id;

test("opening a follow invite follows the owner's tab", async ({ browser }) => {
  const owner = await (await browser.newContext({ viewport: { width: 1100, height: 650 } })).newPage();
  await open(owner);
  const pane = await active(owner);
  await ready(owner, pane);
  const first = await tabId(owner);
  const session = await owner.evaluate(() => window.__arugula.client.session!);
  // The owner is on a second tab; the invite's pane is on the first.
  await owner.evaluate((s) => window.__arugula.client.intent({ op: "new_tab", session: s, from_pane: null }), session);
  await expect.poll(() => tabId(owner)).not.toBe(first);
  const second = await tabId(owner);

  const sent = await fetch(`${base}/api/invite`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ session, who: `tailnet:${FRIEND}`, pane, follow: true }),
  });
  expect(sent.ok).toBe(true);

  const friend = await (
    await browser.newContext({ viewport: { width: 760, height: 480 }, extraHTTPHeaders: { "tailscale-user-login": FRIEND } })
  ).newPage();
  await friend.goto(`/#pane=${pane}&follow=owner`);
  await expect.poll(() => friend.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  // On the owner's tab, not the pane's.
  await expect.poll(() => tabId(friend)).toBe(second);

  // And along when the owner switches.
  await owner.evaluate((t) => window.__arugula.client.selectTab(t), first);
  await expect.poll(() => tabId(friend)).toBe(first);
  await owner.evaluate((t) => window.__arugula.client.selectTab(t), second);
  await expect.poll(() => tabId(friend)).toBe(second);
});
