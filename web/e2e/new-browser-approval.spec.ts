// A new browser checks who approved it (#761): it shows a code, and it pins
// the account's root only on the approving device's answer to that code, or
// once its person has checked the account's fingerprint. Only a browser
// whose own key is the root, or one the app's sign-in signed (control.spec),
// has nothing to check. The browsers here reach a real control through
// routes, which stand in for the answers of other versions and timings.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Browser, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { ANY, controlPort, listen } from "./ports";

test.afterAll(closeContexts);
test.describe.configure({ mode: "serial" });

let base = "";
let control: ChildProcess;
let gh: Server;
let dir = "";
let laptop: Page;
let root = "";

test.use({ baseURL: async ({}, use) => use(base) });

test.beforeAll(async () => {
  gh = createServer((req, res) => {
    const u = new URL(req.url!, "http://github");
    if (u.pathname === "/login/oauth/authorize") {
      const back = new URL(u.searchParams.get("redirect_uri")!);
      back.searchParams.set("code", "c0de");
      back.searchParams.set("state", u.searchParams.get("state")!);
      res.writeHead(302, { location: back.href }).end();
    } else if (u.pathname === "/login/oauth/access_token") {
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ access_token: "gho_test" }));
    } else if (u.pathname === "/user") {
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ id: 7, login: "stranger" }));
    } else res.writeHead(404).end();
  });
  const github = `http://127.0.0.1:${await listen(gh)}`;
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-approval-"));
  const db = join(dir, "control.db");
  control = spawn(
    "../target/debug/arugula-control",
    [
      ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db],
      ...["--github-client-id", "id", "--github-client-secret", "s", "--static-dir", "dist"],
      ...["--github-url", github, "--github-api", github],
    ],
    { stdio: "ignore" },
  );
  base = `http://127.0.0.1:${await controlPort(db, control)}`;
});

test.afterAll(() => {
  control?.kill("SIGKILL");
  gh?.close();
  if (dir) rmSync(dir, { recursive: true, force: true });
});

/** A browser with nothing kept, whose control doesn't say `device_secret`.
 * Its routes see every request (a service worker would answer some). */
async function browserWithoutTheFlag(browser: Browser): Promise<Page> {
  const context = await browser.newContext({ baseURL: base, serviceWorkers: "block" });
  await context.route("**/control.json", async (r) => {
    const res = await r.fetch();
    const { device_secret, ...said } = (await res.json()) as { device_secret?: number };
    expect(device_secret).toBe(1);
    await r.fulfill({ response: res, json: said });
  });
  return context.newPage();
}

const signIn = async (page: Page) => {
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
};

const booted = (page: Page) => page.waitForFunction(() => window.__arugula?.control?.phase === "ready", null, { timeout: 20_000 });
const keyId = (page: Page) => page.evaluate(() => window.__arugula.control!.keys.id);
const pinned = (page: Page) => page.evaluate(() => window.__arugula.control!.enrollment?.root ?? null);
const phase = (page: Page) => page.evaluate(() => window.__arugula.control!.phase);
/** The page is waiting to be approved; its key id. */
async function waiting(page: Page): Promise<string> {
  await page.waitForFunction(() => window.__arugula?.control?.phase === "waiting", null, { timeout: 20_000 });
  return keyId(page);
}
/** The approval reached the page and it did what it does next. */
const settled = (page: Page) => page.waitForFunction(() => ["ready", "unvouched", "error"].includes(window.__arugula?.control?.phase ?? ""), null, { timeout: 20_000 });

/** The code a waiting page shows, `LOOKUP.SECRET`. */
async function deviceCode(page: Page): Promise<string> {
  const code = await page.locator("[data-device-code]").first().getAttribute("data-device-code", { timeout: 20_000 });
  expect(code).toMatch(/^[A-Z0-9]{5}-[A-Z0-9]{5}\.[A-Z0-9]{4}(-[A-Z0-9]{4}){3}$/);
  return code!;
}

/** The laptop approves with the code typed in, as a person does. */
async function approveWith(code: string, id: string) {
  await expect(laptop.locator(`[data-pending="${id}"]`)).toBeVisible({ timeout: 20_000 });
  await laptop.locator("[data-device-whole]").fill(code);
  await laptop.locator("[data-approve]").click();
}

/** The laptop approves as a page from before codes does: it signs the
 * certificate and sends no answer. */
async function approveFromOlderPage(id: string) {
  await expect(laptop.locator(`[data-pending="${id}"]`)).toBeVisible({ timeout: 20_000 });
  await laptop.evaluate(async (d) => {
    const s = window.__arugula.control as unknown as { pending: { device: string }[]; pendingSealed: Set<string>; approve(c: unknown): Promise<void> };
    s.pendingSealed.delete(d);
    await s.approve(s.pending.find((c) => c.device === d));
  }, id);
}

/** Done with this browser: it goes from the account, so the laptop's
 * prompt is clear for the next. */
async function done(page: Page, id: string) {
  await page.context().close();
  await laptop.evaluate(async (d) => {
    const s = window.__arugula.control as unknown as { pending: { device: string }[]; reject(c: unknown): Promise<void>; revoke(d: string): Promise<void> };
    const waiting = s.pending.find((c) => c.device === d);
    await (waiting ? s.reject(waiting) : s.revoke(d));
  }, id);
  await expect(laptop.locator(`[data-pending="${id}"]`)).toHaveCount(0, { timeout: 20_000 });
}

/** The account reads as having no devices yet. */
const noDevicesYet = (page: Page) =>
  page.route("**/api/me", async (r) => {
    const res = await r.fetch();
    await r.fulfill({ response: res, json: { ...(await res.json()), root: null } });
  });

/** What this page asks to store as recovery codes. */
function recoveryCodesSent(page: Page): string[] {
  const sent: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "POST" && new URL(r.url()).pathname === "/api/recovery") sent.push(r.url());
  });
  return sent;
}

test("the first device is its own root: nothing to check, and its codes to keep", async ({ browser }) => {
  laptop = await browserWithoutTheFlag(browser);
  await signIn(laptop);
  await expect(laptop.locator("[data-recovery-code]")).toHaveCount(2, { timeout: 20_000 });
  await laptop.locator("[data-stored-codes]").check();
  await laptop.locator("[data-saved-codes]").click();
  await booted(laptop);
  root = await keyId(laptop);
  expect(await pinned(laptop)).toBe(root);
  await expect(laptop.locator("[data-unvouched]")).toHaveCount(0);
  // The code it had ready for an approval that never came is gone.
  expect(await laptop.evaluate(() => [window.__arugula.control!.deviceCode, localStorage.getItem("arugula.control.device-secret")])).toEqual([null, null]);
});

test("a control that doesn't say device_secret: the browser shows its code, and the approval answers it", async ({ browser }) => {
  const other = await browserWithoutTheFlag(browser);
  await signIn(other);
  const code = await deviceCode(other);
  const id = await keyId(other);
  await approveWith(code, id);
  await booted(other);
  await expect(other.locator("[data-unvouched]")).toHaveCount(0);
  expect(await pinned(other)).toBe(root);
  await done(other, id);
});

test("a control that doesn't say device_secret: an answer that's wrong is refused", async ({ browser }) => {
  const other = await browserWithoutTheFlag(browser);
  await signIn(other);
  const code = await deviceCode(other);
  const id = await keyId(other);
  await approveWith(`${code.split(".")[0]}.ABCD-EFGH-JKMN-PQRS`, id);
  await expect(other.locator('[data-unvouched="wrong"]')).toBeVisible({ timeout: 20_000 });
  expect(await pinned(other)).toBeNull();
  await done(other, id);
});

test("a control that doesn't say device_secret: approved with no answer, the browser asks its person to check the account", async ({ browser }) => {
  const other = await browserWithoutTheFlag(browser);
  await signIn(other);
  const id = await waiting(other);
  await approveFromOlderPage(id);
  await settled(other);
  // Nothing is pinned until the person says so.
  expect(await pinned(other)).toBeNull();
  expect(await phase(other)).toBe("unvouched");
  await expect(other.locator('[data-unvouched="missing"]')).toBeVisible();
  await expect(other.locator(`[data-unvouched-account="${root}"]`)).toBeVisible();
  await other.locator("[data-vouch]").click();
  await booted(other);
  expect(await pinned(other)).toBe(root);
  await done(other, id);
});

test("a control from before codes: approved with no answer, the browser asks its person to check the account", async ({ browser }) => {
  const other = await browserWithoutTheFlag(browser);
  await other.route("**/api/devices", async (r) => {
    if (r.request().method() !== "POST") return r.fallback();
    const { features: _, ...asked } = r.request().postDataJSON() as { features?: string };
    await r.fallback({ postData: JSON.stringify(asked) });
  });
  await signIn(other);
  const id = await waiting(other);
  await approveFromOlderPage(id);
  await settled(other);
  expect(await pinned(other)).toBeNull();
  await expect(other.locator('[data-unvouched="missing"]')).toBeVisible();
  await other.locator("[data-vouch]").click();
  await booted(other);
  expect(await pinned(other)).toBe(root);
  await done(other, id);
});

test("two browsers at once on a new account: the one that waits checks its approval like any other", async ({ browser }) => {
  // It read the account before the other became its first device, so it
  // asked as the first, and control keeps it waiting.
  const other = await browserWithoutTheFlag(browser);
  await noDevicesYet(other);
  const recovery = recoveryCodesSent(other);
  await signIn(other);
  const code = await deviceCode(other);
  const id = await keyId(other);
  await approveWith(code, id);
  await booted(other);
  await expect(other.locator("[data-unvouched]")).toHaveCount(0);
  expect(await pinned(other)).toBe(root);
  // The account's codes are the first device's: this one makes none.
  expect(recovery).toEqual([]);
  await expect(other.locator("[data-recovery-code]")).toHaveCount(0);
  await done(other, id);
});

test("approved as the account's first device, a browser takes only its own key as the root", async ({ browser }) => {
  const other = await browserWithoutTheFlag(browser);
  await noDevicesYet(other);
  const recovery = recoveryCodesSent(other);
  let id = "";
  await other.route("**/api/devices", async (r) => {
    if (r.request().method() !== "POST") return r.fallback();
    const { cert } = r.request().postDataJSON() as { cert: { device: string } };
    id = cert.device;
    // Its certificate from the laptop is in place when the answer comes.
    await r.fetch();
    await approveFromOlderPage(id);
    await r.fulfill({ json: { approved: true, cert, root: id } });
  });
  // Recovery codes are a first device's to keep.
  await other.route("**/api/recovery", (r) => r.fulfill({ json: {} }));
  await signIn(other);
  await other.waitForFunction(() => window.__arugula?.control?.phase === "error", null, { timeout: 30_000 });
  expect(await other.evaluate(() => window.__arugula.control!.error)).toContain("first device");
  expect(await pinned(other)).toBeNull();
  expect(recovery).toEqual([]);
  await done(other, id);
});

test("approved as the account's first device, a browser's own certificate still has to check out", async ({ browser }) => {
  const other = await browserWithoutTheFlag(browser);
  await noDevicesYet(other);
  const recovery = recoveryCodesSent(other);
  let id = "";
  let mine: { device: string } | undefined;
  await other.route("**/api/devices", async (r) => {
    if (r.request().method() === "POST") {
      mine = (r.request().postDataJSON() as { cert: { device: string } }).cert;
      id = mine.device;
      await r.fetch();
      return r.fulfill({ json: { approved: true, cert: mine, root: id } });
    }
    // Its own key as the root, with a signature that isn't good.
    const res = await r.fetch();
    const all = (await res.json()) as { trust: { account: string; root: string } };
    await r.fulfill({ response: res, json: { ...all, trust: { ...all.trust, root: id }, certs: [{ ...mine, sig: "00".repeat(64) }] } });
  });
  await other.route("**/api/recovery", (r) => r.fulfill({ json: {} }));
  await signIn(other);
  await other.waitForFunction(() => window.__arugula?.control?.phase === "error", null, { timeout: 30_000 });
  expect(await other.evaluate(() => window.__arugula.control!.error)).toContain("doesn't check out");
  expect(await pinned(other)).toBeNull();
  // Codes wait until its certificate has checked out.
  expect(recovery).toEqual([]);
  await expect(laptop.locator(`[data-pending="${id}"]`)).toBeVisible({ timeout: 20_000 });
  await done(other, id);
});

test("a control that says device_secret: the same code and the same answer", async ({ browser }) => {
  const other = await (await browser.newContext({ baseURL: base, serviceWorkers: "block" })).newPage();
  await signIn(other);
  const code = await deviceCode(other);
  const id = await keyId(other);
  await approveWith(code, id);
  await booted(other);
  await expect(other.locator("[data-unvouched]")).toHaveCount(0);
  expect(await pinned(other)).toBe(root);
  await done(other, id);
});
