// M103: the machine's web client asks its owner once whether to send usage
// counts and crash reports, with the card in the corner. Not under a test
// runner unless a spec says so (this one does, `__arugulaTelemetry`), and
// never to anyone but the owner. Put away unanswered, it asks again on the
// next load; answered, never again. Yes and No are the daemon's
// (`telemetry.get`), the switch in the machine's settings flips it, and with
// it on, the views the page opens reach the day's counts (`telemetry.note`).
//
// The daemon is one of the spec's own, with nothing keeping telemetry off
// (no CI, no DO_NOT_TRACK), and its reports go nowhere.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, open } from "./helpers";
import { ANY, daemonPort } from "./ports";

const OWNER = "me@example.com";
let base = "";
let state = "";
let daemon: ChildProcess | undefined;

test.use({ baseURL: async ({}, use) => use(base) });
test.describe.configure({ mode: "serial" });
test.afterAll(closeContexts);

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "arugula-e2e-telemetry-machine-"));
  const env: NodeJS.ProcessEnv = { ...process.env, ARUGULA_TELEMETRY_URL: "http://127.0.0.1:9" };
  for (const k of ["CI", "DO_NOT_TRACK", "ARUGULA_TELEMETRY", "NEXTEST_RUN_ID"]) delete env[k];
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", state, "--shell", "bash --norc --noprofile", "--no-manager-env"],
      ...["--owner", OWNER, "--tailscale-socket", "/nonexistent/tailscaled.sock"],
    ],
    { stdio: "ignore", env },
  );
  base = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  if (state) rmSync(state, { recursive: true, force: true });
});

type Info = { on: boolean; why: string; via?: string; preview: { events: { event: string; views?: Record<string, number> }[] } };
const info = async (): Promise<Info> => (await (await fetch(`${base}/api/telemetry`)).json()) as Info;
const card = (p: Page) => p.getByRole("dialog", { name: "Help improve Arugula?" });
const optIn = (p: Page) => p.addInitScript(() => Object.assign(window, { __arugulaTelemetry: true }));

/** The page, connected and drawn, then a moment for the card to come. */
async function load(page: Page) {
  await open(page);
  await expect(page.locator(".ws-head [data-account-menu]")).toBeVisible();
  await page.waitForTimeout(500);
}

test("under the test runner it doesn't ask", async ({ page }) => {
  await load(page);
  await expect(card(page)).toHaveCount(0);
  expect((await info()).why).toBe("not_asked");
});

test("a guest is never asked, and can't read or answer it", async ({ browser }) => {
  const ctx = await browser.newContext({ storageState: { cookies: [], origins: [] }, extraHTTPHeaders: { "tailscale-user-login": "friend@example.com" } });
  const page = await ctx.newPage();
  await optIn(page);
  await page.goto("/");
  await page.waitForTimeout(1500);
  await expect(card(page)).toHaveCount(0);
  const as = { "tailscale-user-login": "friend@example.com" };
  expect((await page.request.get("/api/telemetry", { headers: as })).status()).toBe(403);
  expect((await page.request.put("/api/telemetry", { headers: as, data: { on: true } })).status()).toBe(403);
  await ctx.close();
  expect((await info()).why).toBe("not_asked");
});

test("the owner is asked once: put away it asks again, Yes turns it on, and views are counted", async ({ page }) => {
  test.setTimeout(60_000);
  // Nobody has answered (whatever a run before this one did).
  rmSync(join(state, "telemetry.json"), { force: true });
  await optIn(page);
  await load(page);
  await expect(card(page)).toBeVisible();
  await expect(card(page)).toContainText("from this machine: versions, which features are used");
  // What's sent: the preview, before a yes.
  await card(page).getByText("What's sent").click();
  await expect(card(page).locator("[data-telemetry-preview] pre")).toContainText('"preview_only": true');

  // Put away: still not asked, and asked again on the next load.
  await card(page).getByRole("button", { name: "Not now" }).click();
  await expect(card(page)).toHaveCount(0);
  expect((await info()).why).toBe("not_asked");
  await load(page);
  await expect(card(page)).toBeVisible();

  // Yes: on, from the web client, and never asked again.
  await card(page).getByRole("button", { name: "Yes, send them" }).click();
  await expect(card(page)).toHaveCount(0);
  await expect.poll(async () => (await info()).on).toBe(true);
  expect((await info()).via).toBe("web");
  await load(page);
  await expect(card(page)).toHaveCount(0);

  // Counting, then the swarm, opened; the page's count goes as it goes
  // away (pagehide on the live page, not a race with an unload).
  await expect(page.locator("html[data-usage-counts]")).toHaveCount(1);
  await page.evaluate(() => (location.hash = "#swarm"));
  await expect(page.locator(".swarm")).toBeVisible();
  await page.evaluate(() => dispatchEvent(new PageTransitionEvent("pagehide")));
  const views = async () => (await info()).preview.events.find((e) => e.event === "daily")?.views ?? {};
  await expect.poll(views).toEqual({ swarm: 1 });
});

test("the settings switch flips it, and No turns it off for good", async ({ page }) => {
  await optIn(page);
  await load(page);
  await expect(card(page)).toHaveCount(0);
  const settings = async () => {
    await page.locator(".ws-head [data-account-menu]").click();
    await page.getByRole("menuitem", { name: "Usage and crash reports…" }).click();
    return page.getByRole("dialog", { name: "Usage and crash reports" });
  };
  let dialog = await settings();
  const sw = dialog.getByRole("switch", { name: "Usage and crash reports from this machine" });
  await expect(sw).toBeChecked();
  await sw.uncheck();
  await expect.poll(async () => (await info()).why).toBe("no");
  await expect(sw).not.toBeChecked();
  await sw.check();
  await expect.poll(async () => (await info()).on).toBe(true);
  await dialog.getByText("What's sent").click();
  await expect(dialog.locator("[data-telemetry-preview] pre")).toContainText('"install"');
  await dialog.getByRole("button", { name: "Done", exact: true }).click();

  // Asked again only where nobody has answered: the daemon forgets.
  rmSync(join(state, "telemetry.json"));
  await load(page);
  await expect(card(page)).toBeVisible();
  await card(page).getByRole("button", { name: "No thanks" }).click();
  await expect(card(page)).toHaveCount(0);
  await expect.poll(async () => (await info()).why).toBe("no");
  await load(page);
  await expect(card(page)).toHaveCount(0);
  dialog = await settings();
  await expect(dialog.getByRole("switch", { name: "Usage and crash reports from this machine" })).not.toBeChecked();
});
