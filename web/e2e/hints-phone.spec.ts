// The phone offer (hints/phone.ts, Arugula issue
// iss_a2ad28a0f67e61e8fe081d8121e37c3e): on a machine with no account, a
// pane that waited 5 minutes or more while the page was idle brings a
// toast when they come back, "This waited N min for you. Get this on your
// phone", whose Sign in is the one-click cloud join, then a QR code for the
// phone. Once a day at most, and never on a joined machine, where
// push-device's toast keeps quiet too. Time is the page's clock, moved on
// (Playwright's `clock`): the pane's wait is the daemon's own. The cloud's
// answers are stood in for, as welcome.spec.ts does.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { ANY, daemonPort } from "./ports";

test.afterAll(closeContexts);

let base = "";
let daemon: ChildProcess;
let dir: string;
let claude: string;
const MIN = 60_000;

// The page's day is the offer's key: its clock sits at midday whatever
// the hour here, so minutes moved on never cross midnight.
const off = 12 - new Date().getUTCHours();
test.use({ baseURL: async ({}, use) => use(base), timezoneId: off === 0 ? "Etc/GMT" : `Etc/GMT${off > 0 ? "-" : "+"}${Math.abs(off)}` });
test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-hints-phone-"));
  claude = mkdtempSync(join(tmpdir(), "arugula-e2e-hints-phone-claude-"));
  daemon = spawn(
    "../target/debug/arugulad",
    ["--listen", ANY, "--state-dir", dir, "--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
    { stdio: "ignore", env: { ...process.env, CLAUDE_CONFIG_DIR: claude } },
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
  for (const d of [dir, claude]) rmSync(d, { recursive: true, force: true });
});

/** A first run's page, asking for hints, with the ones any page gets
 * (a command's mark) seen. `joined`: `/api/host` says this machine is in
 * an account. */
async function openFresh(page: Page, joined = false) {
  await page.addInitScript(() => {
    Object.assign(window, { __arugulaHints: true });
    if (!localStorage.getItem("arugula.seen")) localStorage.setItem("arugula.seen", JSON.stringify({ "seen:command-mark": 1 }));
  });
  if (joined)
    await page.route(/\/api\/host$/, async (r) => {
      const real = (await (await r.fetch()).json()) as Record<string, unknown>;
      return r.fulfill({ json: { ...real, control_state: { state: "joined", url: "https://control.arugula.io", kind: "account", connected: true } } });
    });
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state?.panes.length ?? 0)).toBeGreaterThan(0);
}

/** A pane waits on the person from now: the daemon's clock. */
async function waits(page: Page): Promise<number> {
  const pane = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  const r = await fetch(`${base}/api/panes/${pane}/attention`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ state: "needs_input" }) });
  expect(r.ok).toBe(true);
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.attention, pane)).toBe("needs_input");
  return pane;
}

/** Nobody touches the page until `at`, then the pointer moves: they're back. */
let moves = 0;
async function backAt(page: Page, at: number) {
  await page.clock.setFixedTime(at);
  await page.mouse.move(20 + (++moves % 2) * 10, 200);
}

const offer = (page: Page) => page.locator('.hint-toast[data-hint="phone-away"]');

test("a pane waited while the page was idle: the offer on return wraps at 360px, and Sign in joins, then the phone's QR code", async ({ page, context }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await openFresh(page);
  const t = Date.now();
  await waits(page);
  // Back within a minute: nothing.
  await backAt(page, t + MIN);
  await page.waitForTimeout(500);
  await expect(page.locator("[data-hint]")).toHaveCount(0);

  // Six minutes since anything moved: the offer.
  await backAt(page, t + 7 * MIN);
  await expect(offer(page)).toContainText(/This waited [67] min for you\. Get this on your phone/);
  await expect(offer(page).locator("[data-hint-act]")).toHaveText("Sign in");
  // No push-device beside it, now or once it's gone.
  await expect(page.locator('[data-hint="push-device"]')).toHaveCount(0);

  // At a phone's width it wraps inside the window, not squished to half of it.
  const box = (await offer(page).boundingBox())!;
  const copy = (await offer(page).locator(".hint-copy").boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(360);
  expect(box.width).toBeGreaterThan(300);
  expect(copy.height).toBeGreaterThan(20);

  // Sign in: the cloud join, in a tab of its own.
  const url = "https://control.arugula.io";
  const code = "ABCDE-FGHJK.MNPQ-RSTV-WXYZ-0123";
  const pending = { code, approve: `${url}/#join=${code}`, expires_ms: t + 60 * MIN };
  let state: "none" | "pending" | "joined" = "none";
  let asked: unknown = null;
  const control = () => ({ url, ...(state === "pending" ? { pending } : state === "joined" ? { joined: url } : {}) });
  await page.route(/\/api\/setup(\/control)?(\?.*)?$/, async (r) => {
    const u = new URL(r.request().url());
    if (u.pathname === "/api/setup/control") {
      asked = r.request().postDataJSON();
      state = "pending";
      return r.fulfill({ json: { pending } });
    }
    return r.fulfill({ json: { control: control() } });
  });
  await context.route(`${url}/**`, (r) => r.fulfill({ body: "Arugula cloud" }));
  const tab = context.waitForEvent("page");
  await offer(page).locator("[data-hint-act]").click();
  await expect.poll(() => asked).toEqual({ url });
  await expect.poll(async () => (await tab).url()).toBe(pending.approve);
  const signing = page.locator("[data-phone-signin]");
  await expect(signing).toContainText("Signing in");
  await expect(signing).toContainText("Finish in your browser. This page carries on by itself.");
  await expect(offer(page)).toHaveCount(0);

  // Added there: the phone's QR code, for the app, and what the phone can do.
  state = "joined";
  const qr = page.locator("[data-phone-qr]");
  await expect(qr).toContainText("Open Arugula on your phone", { timeout: 10_000 });
  await expect(qr).toContainText("you can answer and type from your phone");
  await expect(qr.getByRole("img", { name: "QR code for https://app.arugula.io" })).toBeVisible();
  await expect(qr).toContainText("app.arugula.io");
  await qr.locator("[data-phone-done]").click();
  await expect(qr).toHaveCount(0);
});

test("once a day: dismissed, the same day's next wait says nothing; the next day's does", async ({ page }) => {
  await openFresh(page);
  const t = Date.now();
  await waits(page);
  await backAt(page, t + 6 * MIN);
  await expect(offer(page)).toBeVisible();
  await offer(page).locator("[data-hint-dismiss]").click();
  await expect(offer(page)).toHaveCount(0);

  // Away again, the same day.
  await backAt(page, t + 12 * MIN);
  await page.waitForTimeout(1000);
  await expect(page.locator("[data-hint]")).toHaveCount(0);

  // The next day, still waiting.
  await backAt(page, t + 24 * 60 * MIN);
  await expect(offer(page)).toBeVisible();
});

test("never on a joined machine", async ({ page }) => {
  await openFresh(page, true);
  const t = Date.now();
  await waits(page);
  await backAt(page, t + 7 * MIN);
  await page.waitForTimeout(1500);
  await expect(offer(page)).toHaveCount(0);
});
