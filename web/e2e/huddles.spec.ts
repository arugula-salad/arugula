// M63: huddles. The owner starts one on a session from the bar; a friend
// who drives and a watcher see it and join. Audio goes peer to peer
// (Chrome's fake microphone beeps), each person sees who's talking and who
// muted, someone whose share is revoked drops out within a second, and a
// daemon that goes away ends the huddle for everyone in it.
//
// These people come over the tailnet, with no device keys: they're marked
// unverified. Signed fingerprints are checked in huddles-control.spec.ts.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Browser, type Page } from "@playwright/test";
import { ANY, daemonPort } from "./ports";

let base = "";
const OWNER = "me@example.com";
const FRIEND = "friend@example.com";
const WATCHER = "watcher@example.com";
let daemon: ChildProcess;
let dir: string;

test.describe.configure({ mode: "serial" });
test.use({
  baseURL: async ({}, use) => use(base),
  launchOptions: { args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", "--autoplay-policy=no-user-gesture-required"] },
});

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "illogical-e2e-huddles-"));
  daemon = spawn(
    "../target/debug/illogicald",
    [
      ...["--listen", ANY, "--state-dir", dir, "--owner", OWNER],
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

test.afterAll(async () => {
  // Pages left open would keep reconnecting to this daemon's port.
  for (const p of [owner, friend, watcher]) await p?.context().close();
  daemon?.kill("SIGKILL");
  rmSync(dir, { recursive: true, force: true });
});

const api = (path: string, body?: unknown) =>
  fetch(base + path, {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

async function person(browser: Browser, who?: string): Promise<Page> {
  const ctx = await browser.newContext({
    permissions: ["microphone"],
    ...(who ? { extraHTTPHeaders: { "tailscale-user-login": who } } : {}),
  });
  const page = await ctx.newPage();
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.__illogical?.client.connected)).toBe(true);
  await expect.poll(() => page.evaluate(() => window.__illogical.client.hasCalls())).toBe(true);
  return page;
}

/** Client ids in the huddle, as this page has it. */
const members = (page: Page, session: number) =>
  page.evaluate((s) => window.__illogical.client.call(s)?.members.map((m) => m.client) ?? [], session);

/** How this page's connection to each other member is doing. */
const peers = (page: Page) =>
  page.evaluate(() => {
    const h = window.__illogical.huddle;
    const me = window.__illogical.client.clientId;
    return (h?.call()?.members ?? []).filter((m) => m.client !== me).map((m) => [h!.peer(m.client)?.state, h!.peer(m.client)?.trust]);
  });

let owner: Page;
let friend: Page;
let watcher: Page;
let session = 0;

test("the owner starts a huddle; a friend sees it and joins, and they hear each other", async ({ browser }) => {
  owner = await person(browser);
  session = await owner.evaluate(() => window.__illogical.client.state!.sessions[0].id);
  await api("/api/acl", { session, principal: `tailnet:${FRIEND}`, role: "editor" });
  await api("/api/acl", { session, principal: `tailnet:${WATCHER}`, role: "viewer" });
  friend = await person(browser, FRIEND);
  watcher = await person(browser, WATCHER);

  await owner.locator("header.bar .huddle-button").click();
  await expect(owner.locator(".huddle-bar .huddle-member")).toHaveCount(1);
  // The others see a huddle going on.
  await expect(friend.locator("header.bar .huddle-button.live .huddle-count")).toHaveText("1");
  await expect(watcher.locator("header.bar .huddle-button.live")).toBeVisible();

  await friend.locator("header.bar .huddle-button").click();
  await expect(owner.locator(".huddle-bar .huddle-member")).toHaveCount(2);
  await expect(friend.locator(".huddle-bar .huddle-member")).toHaveCount(2);
  // Connected, and over the tailnet with no device keys: unverified.
  await expect.poll(() => peers(owner), { timeout: 15_000 }).toEqual([["connected", "unverified"]]);
  await expect.poll(() => peers(friend), { timeout: 15_000 }).toEqual([["connected", "unverified"]]);

  // Chrome's fake microphone beeps: each sees the other talking.
  const friendId = await friend.evaluate(() => window.__illogical.client.clientId);
  const ownerId = await owner.evaluate(() => window.__illogical.client.clientId);
  await expect(owner.locator(`.huddle-member[data-member="${friendId}"][data-speaking]`)).toBeVisible({ timeout: 10_000 });
  await expect(friend.locator(`.huddle-member[data-member="${ownerId}"][data-speaking]`)).toBeVisible({ timeout: 10_000 });
  // Audio really arrives: bytes on the inbound RTP stream.
  const bytes = () =>
    owner.evaluate(async (id) => {
      const pc = (window.__illogical.huddle as unknown as { peers: Map<number, { pc: RTCPeerConnection }> }).peers.get(id)!.pc;
      let n = 0;
      (await pc.getStats()).forEach((s) => {
        if (s.type === "inbound-rtp" && s.kind === "audio") n = s.bytesReceived;
      });
      return n;
    }, friendId);
  await expect.poll(bytes, { timeout: 10_000 }).toBeGreaterThan(1000);
});

test("a watcher joins too, and a mute shows for everyone", async () => {
  await watcher.locator("header.bar .huddle-button").click();
  for (const p of [owner, friend, watcher]) await expect(p.locator(".huddle-bar .huddle-member")).toHaveCount(3);
  await expect.poll(() => peers(watcher), { timeout: 15_000 }).toEqual([
    ["connected", "unverified"],
    ["connected", "unverified"],
  ]);

  await friend.locator("[data-huddle-mute]").click();
  const friendId = await friend.evaluate(() => window.__illogical.client.clientId);
  await expect(owner.locator(`.huddle-member[data-member="${friendId}"] .huddle-muted`)).toBeVisible();
  await expect(watcher.locator(`.huddle-member[data-member="${friendId}"] .huddle-muted`)).toBeVisible();
  // Muted: the friend's own mic track is off.
  expect(await friend.evaluate(() => window.__illogical.huddle!.muted)).toBe(true);
  await friend.keyboard.press("Control+Shift+Space");
  await expect(owner.locator(`.huddle-member[data-member="${friendId}"] .huddle-muted`)).toHaveCount(0);
});

test("a watcher whose share is revoked drops out within a second", async () => {
  const at = Date.now();
  await api("/api/acl", { session, principal: `tailnet:${WATCHER}`, role: null });
  await expect.poll(() => members(owner, session), { timeout: 1000 }).toHaveLength(2);
  expect(Date.now() - at).toBeLessThan(1000);
  // The watcher hangs up on their side too.
  await expect(watcher.locator(".huddle-bar.ended")).toBeVisible();
  expect(await watcher.evaluate(() => window.__illogical.huddle?.live())).toBe(false);
  await expect.poll(() => peers(owner)).toEqual([["connected", "unverified"]]);
});

test("leaving, and the last one out ends it", async () => {
  await friend.locator("[data-huddle-leave]").click();
  await expect(friend.locator(".huddle-bar.ended")).toContainText("You left");
  await expect(owner.locator(".huddle-bar .huddle-member")).toHaveCount(1);
  // The session menu offers it again.
  await friend.locator(".huddle-bar.ended button", { hasText: "Join again" }).click();
  await expect(owner.locator(".huddle-bar .huddle-member")).toHaveCount(2);
  await expect.poll(() => peers(owner), { timeout: 15_000 }).toEqual([["connected", "unverified"]]);
});

test("the machine going away ends the huddle for everyone in it", async () => {
  daemon.kill("SIGKILL");
  await expect(owner.locator(".huddle-bar.ended")).toContainText("dropped", { timeout: 10_000 });
  await expect(friend.locator(".huddle-bar.ended")).toContainText("dropped", { timeout: 10_000 });
});
