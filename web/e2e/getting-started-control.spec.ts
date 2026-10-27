// Getting started on control's page: it opens by itself once per account,
// the first time the account's machine shows there, and is remembered in
// the account's prefs (`/api/me/prefs`, `seen:getting-started`), not the
// browser's: a browser with its storage cleared isn't greeted again.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import type { Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fakeGithub } from "../fixtures/fakes.ts";
import { closeContexts } from "./helpers";
import { ANY, controlPort } from "./ports";

test.afterAll(closeContexts);

let base = "";
const procs: ChildProcess[] = [];
const dirs: string[] = [];
let gh: Server;

test.use({ baseURL: async ({}, use) => use(base) });

function temp(what: string) {
  const d = mkdtempSync(join(tmpdir(), `arugula-e2e-gscontrol-${what}-`));
  dirs.push(d);
  return d;
}

async function up(url: string) {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(url)).status < 500) return;
    } catch {
      // not yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`${url} didn't come up`);
}

test.beforeAll(async () => {
  gh = fakeGithub(0);
  await new Promise((r) => gh.once("listening", r));
  const github = `http://127.0.0.1:${(gh.address() as AddressInfo).port}`;
  const db = join(temp("db"), "control.db");
  procs.push(
    spawn(
      "../target/debug/arugula-control",
      [
        ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db],
        ...["--github-client-id", "id", "--github-client-secret", "s", "--static-dir", "dist"],
        ...["--github-url", github, "--github-api", github],
      ],
      { stdio: "ignore" },
    ),
  );
  base = `http://127.0.0.1:${await controlPort(db, procs.at(-1))}`;
  await up(`${base}/control.json`);
});

test.afterAll(() => {
  for (const p of procs) p.kill("SIGKILL");
  gh?.close();
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
});

test("opens once per account on control's page, remembered by the account", async ({ browser }) => {
  test.setTimeout(60_000);
  const ctx = await browser.newContext();
  // A person, not automation: the page greets them (welcome.spec.ts).
  await ctx.addInitScript(() => Object.defineProperty(Navigator.prototype, "webdriver", { get: () => false }));
  const page = await ctx.newPage();
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();

  // A machine of this account, reached only through control.
  const state = temp("mac");
  const joining = spawn("../target/debug/arugulad", ["join", base, "--name", "mac", "--state-dir", state], {
    stdio: ["pipe", "pipe", "ignore"],
  });
  procs.push(joining);
  const link = await new Promise<string>((res) => {
    let out = "";
    joining.stdout!.on("data", (d) => {
      out += d;
      const m = out.match(/(http\S+#join=[A-Z0-9.-]+)/);
      if (m) res(m[1]);
    });
  });
  const exited = new Promise<number | null>((r) => joining.on("exit", r));
  await page.goto(link);
  await page.locator("[data-approve-join]").click();
  expect(await exited).toBe(0);
  procs.push(
    spawn(
      "../target/debug/arugulad",
      [
        ...["--listen", ANY, "--name", "mac", "--state-dir", state],
        ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
      ],
      { stdio: "ignore" },
    ),
  );
  await expect
    .poll(async () => ((await (await ctx.request.get("/api/directory")).json()) as { daemons: { online: boolean }[] }).daemons.map((d) => d.online), {
      timeout: 20_000,
    })
    .toEqual([true]);
  const prefs = async () => (await (await ctx.request.get("/api/me/prefs")).json()) as Record<string, unknown>;

  // The machine shows: Getting started greets the account, at its start.
  await page.goto("/");
  const panel = page.getByRole("dialog", { name: "Getting started" });
  await expect(panel).toBeVisible({ timeout: 20_000 });
  await expect(panel.locator("[data-start-progress]")).toHaveText("Step 1 / 5 · Welcome");
  await expect.poll(async () => (await prefs())["seen:getting-started"]).toBeTruthy();
  await panel.getByRole("button", { name: "Close" }).click();
  await expect(panel).toBeHidden();

  // Not again: not on a reload, nor with this browser's storage cleared.
  for (const clear of [false, true]) {
    if (clear) await page.evaluate(() => localStorage.clear());
    await page.goto("/");
    await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.e2e && window.__arugula.client.connected), { timeout: 20_000 }).toBe(true);
    await expect(page.locator(".session-button")).toBeVisible();
    await page.waitForTimeout(1000);
    await expect(panel).toBeHidden();
  }
  await ctx.close();
});
