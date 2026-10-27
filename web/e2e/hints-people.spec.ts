// A hint from facts (DECISIONS.md "Hints come from facts, once per
// account", hints.spec.ts has the rest): a second person in the session.
// The owner's page says once, as a toast, that clicking their avatar
// follows them and Share › Pair mode types together; Follow follows them.
// Hints keep quiet under a test runner unless a spec asks: this one does.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { ANY, daemonPort } from "./ports";

test.afterAll(closeContexts);

let base = "";
const OWNER = "me@example.com";
const FRIEND = "friend@example.com";
let daemon: ChildProcess;
let dir: string;

test.use({ baseURL: async ({}, use) => use(base) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-hints-people-"));
  daemon = spawn(
    "../target/debug/arugulad",
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

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  rmSync(dir, { recursive: true, force: true });
});

async function openAs(page: Page) {
  await page.addInitScript(() => Object.assign(window, { __arugulaHints: true }));
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state !== null)).toBe(true);
}

test("a second person in the session: follow them, once", async ({ browser }) => {
  const owner = await (await browser.newContext()).newPage();
  await openAs(owner);
  const toast = owner.locator('.hint-toast[data-hint="second-person"]');
  await owner.waitForTimeout(1000);
  await expect(toast).toHaveCount(0);

  const session = await owner.evaluate(() => window.__arugula.client.state!.sessions[0].id);
  await fetch(`${base}/api/acl`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ session, principal: `tailnet:${FRIEND}`, role: "editor" }) });
  const friend = await (await browser.newContext({ extraHTTPHeaders: { "tailscale-user-login": FRIEND } })).newPage();
  await openAs(friend);

  await expect(toast).toContainText("Click their avatar to follow. Share › Pair mode to type together");
  const them = await owner.evaluate(() => window.__arugula.client.others()[0].client);
  await toast.locator("[data-hint-act]").click();
  await expect.poll(() => owner.evaluate(() => window.__arugula.client.following)).toBe(them);
  await expect(owner.locator("[data-hint]")).toHaveCount(0);

  // Said: the friend leaving and coming back says nothing.
  await owner.evaluate(() => window.__arugula.client.follow(null));
  await friend.reload();
  await owner.waitForTimeout(2000);
  await expect(owner.locator("[data-hint]")).toHaveCount(0);
});
