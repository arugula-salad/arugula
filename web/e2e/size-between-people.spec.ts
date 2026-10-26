// #1502: switching to a tab or typing in it takes the size from nobody or
// from the same person's window, never from another person's. Only the
// "Use this window's size" button does. A friend (a second principal, a
// header on loopback as `tailscale serve` adds it) edits the owner's pane.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { active, closeContexts, open, ready, run, tab, text, type } from "./helpers";
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
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-size-people-"));
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

const me = (p: Page) => p.evaluate(() => window.__arugula.client.clientId);

test("another person's switch and typing keep the size; asking takes it", async ({ browser }) => {
  const owner = await (await browser.newContext({ viewport: { width: 1100, height: 650 } })).newPage();
  await open(owner);
  const pane = await active(owner);
  const session = await owner.evaluate(() => window.__arugula.client.state!.sessions[0].id);
  await ready(owner, pane);
  const res = await fetch(`${base}/api/acl`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ session, principal: `tailnet:${FRIEND}`, role: "editor" }),
  });
  expect(res.ok).toBe(true);
  const sized = await tab(owner);
  expect(sized.owner).toBe(await me(owner));

  // Every size the tab has from now on (each one resizes the PTY).
  await owner.evaluate((id) => {
    const c = window.__arugula.client;
    const w = window as unknown as { sizes: string[] };
    w.sizes = [];
    let last = "";
    c.subscribe(() => {
      const t = c.state?.tabs.find((t) => t.id === id);
      const now = t ? `${t.cols}x${t.rows}` : "";
      if (now && now !== last) w.sizes.push(now);
      last = now;
    });
  }, sized.id);
  const sizes = () => owner.evaluate(() => (window as unknown as { sizes: string[] }).sizes);

  // The friend opens the tab (a switch) and types: neither takes the size.
  const friend = await (
    await browser.newContext({ viewport: { width: 760, height: 480 }, extraHTTPHeaders: { "tailscale-user-login": FRIEND } })
  ).newPage();
  await open(friend);
  await ready(friend, pane);
  expect(await friend.evaluate(() => window.__arugula.client.role())).toBe("editor");
  // It runs on the owner's machine: they trust the friend with it (M14), and
  // the friend takes control (M13). Typing is allowed; only the size isn't.
  await owner.evaluate(
    ([p, to]) => window.__arugula.client.paneOp(p, { op: "grant_trust", to, minutes: 30 }),
    [pane, `tailnet:${FRIEND}`] as const,
  );
  await expect.poll(() => friend.evaluate((p) => window.__arugula.client.mayType(p), pane)).toBe(true);
  await friend.evaluate((p) => window.__arugula.client.paneOp(p, { op: "take_control" }), pane);
  await run(friend, pane, "echo friend-$((3+4))", "friend-7");
  await expect.poll(() => text(owner, pane)).toContain("friend-7");
  await friend.waitForTimeout(3500);
  await type(friend, pane, "echo again-$((5+5))\n");
  await expect.poll(() => text(owner, pane)).toContain("again-10");
  expect(await sizes(), "the pane was resized by another person").toEqual([`${sized.cols}x${sized.rows}`]);
  expect((await tab(owner)).owner).toBe(await me(owner));

  // The button takes it.
  await friend.locator("[data-sized-elsewhere]").getByRole("button", { name: "Use this window's size" }).click();
  await expect.poll(async () => (await tab(owner)).owner).toBe(await me(friend));
  await expect.poll(async () => (await sizes()).length).toBe(2);
  const [was, now] = await sizes();
  expect(was).toBe(`${sized.cols}x${sized.rows}`);
  expect(now).not.toBe(was);
  await expect.poll(async () => {
    const t = await tab(friend);
    return `${t.cols}x${t.rows}`;
  }).toBe(now);
});
