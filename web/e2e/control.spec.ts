// M17/M18: Arugula control in a browser. A stranger with no tailnet signs
// in (a fake GitHub), the browser becomes the account's first device, two
// machines join by code, and both are listed and usable: one directly,
// one only through the relay. A second browser (the phone) can't reach
// anything until the first approves it, and loses access when removed.

import { spawn, type ChildProcess } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Browser, type Page } from "@playwright/test";
import { closeContexts, controlPanel, pasteFile, ready, run, text, uploadedPath } from "./helpers";
import { ANY, controlPort, daemonPort, listen } from "./ports";
import { labs } from "./labs";

test.afterAll(closeContexts);

let base = "";
const procs: ChildProcess[] = [];
const dirs: string[] = [];
let gh: Server;
/** Each machine's state dir, by name. */
const states: Record<string, string> = {};

test.describe.configure({ mode: "serial" });
test.use({ baseURL: async ({}, use) => use(base) });

function temp(what: string) {
  const d = mkdtempSync(join(tmpdir(), `arugula-e2e-control-${what}-`));
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

async function signIn(page: Page) {
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
  await expect(page.locator(".control-center, .control-page, .app")).toBeVisible();
}

/** What an Arugula from before #760 says it understands: no
 * `join-secret`, so no secret in its link, and the account is compared. */
const BEFORE_SECRETS = { ARUGULA_FEATURES: "presigned-invites,owner-moves" };

/** `arugulad join`, approved from `page`; then the daemon runs. `older`
 * plays an Arugula from before #760 at the join, against this control and
 * page: its link has only the code, the page shows the code and the
 * account to compare, and it asks. */
async function addMachine(page: Page, name: string, direct: boolean, older = false) {
  const state = temp(name);
  states[name] = state;
  const joining = spawn("../target/debug/arugulad", ["join", base, "--name", name, "--state-dir", state], {
    stdio: ["pipe", "pipe", "ignore"],
    env: { ...process.env, ...(older ? BEFORE_SECRETS : {}) },
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
  const code = link.split("#join=")[1];
  const exited = new Promise<number | null>((r) => joining.on("exit", r));
  await page.goto(link);
  if (older) {
    expect(code).toMatch(/^[A-Z0-9]{5}-[A-Z0-9]{5}$/);
    await expect(page.locator("[data-join-code]")).toHaveText(code);
    // The machine asks whether the account is the one this browser shows.
    const account = await page.locator("[data-join-account]").getAttribute("data-join-account");
    await page.locator("[data-approve-join]").click();
    joining.stdin!.end(`${account}\n`);
  } else {
    // #760: the code and the machine's secret after it, which control
    // never sees. Nothing to compare, and the machine finishes by itself.
    expect(code).toMatch(/^[A-Z0-9]{5}-[A-Z0-9]{5}\.[A-Z0-9]{4}(-[A-Z0-9]{4}){3}$/);
    await expect(page.locator("[data-join-sealed]")).toBeVisible();
    await expect(page.locator("[data-join-code]")).toHaveCount(0);
    await expect(page.locator("[data-join-account]")).toHaveCount(0);
    await page.locator("[data-approve-join]").click();
  }
  expect(await exited).toBe(0);
  procs.push(
    spawn(
      "../target/debug/arugulad",
      [
        ...["--listen", ANY, "--name", name, "--state-dir", labs(state)],
        ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
        ...(direct ? ["--direct-url", "http://127.0.0.1:0"] : []),
      ],
      { stdio: "ignore" },
    ),
  );
}

/** The page has booted and is signed in and enrolled. */
async function booted(page: Page) {
  await page.waitForFunction(() => window.__arugula?.control?.phase === "ready", null, { timeout: 20_000 });
}

const hostNames = async (page: Page) => (await booted(page), page.evaluate(() => window.__arugula.hosts.names));
const connected = (page: Page) => page.evaluate(() => window.__arugula.client.connected);

async function showHost(page: Page, name: string) {
  await page.evaluate((n) => window.__arugula.hosts.select(n), name);
  await expect.poll(() => connected(page), { timeout: 20_000 }).toBe(true);
}

async function shell(page: Page, marker: string) {
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state?.panes.length ?? 0)).toBeGreaterThan(0);
  const pane = await page.evaluate(() => window.__arugula.client.active()!);
  await ready(page, pane);
  await run(page, pane, `echo ${marker}-$((6*7))`, `${marker}-42`);
  expect(await text(page, pane)).toContain(`${marker}-42`);
}

let laptop: Page;
let phone: Page;

let recoveryCodes: string[] = [];

test("a stranger signs up and becomes the first device", async ({ browser }) => {
  laptop = await (await browser.newContext({ permissions: ["clipboard-read", "clipboard-write"] })).newPage();
  await signIn(laptop);
  // Recovery codes, once, each with Copy, and both to copy or download (#99).
  await expect(laptop.locator("[data-recovery-code]")).toHaveCount(2);
  recoveryCodes = await laptop.locator("[data-recovery-code]").allTextContents();
  const clipboard = () => laptop.evaluate(() => navigator.clipboard.readText());
  // (The Add a machine screen waits under the codes, with its own Copy.)
  await laptop.locator(".prompt .copy-text [data-copy]").first().click();
  await expect(laptop.locator(".prompt .copy-text [data-copy]").first()).toHaveText("Copied");
  expect(await clipboard()).toBe(recoveryCodes[0]);
  await laptop.getByRole("button", { name: "Copy both" }).click();
  expect(await clipboard()).toBe(recoveryCodes.join("\n") + "\n");
  const saved = laptop.waitForEvent("download");
  await laptop.locator("[data-download-codes]").click();
  expect((await saved).suggestedFilename()).toBe("arugula-recovery-codes.txt");
  // Continue only once they're stored (#106).
  await expect(laptop.locator("[data-saved-codes]")).toBeDisabled();
  await laptop.locator("[data-stored-codes]").check();
  await laptop.locator("[data-saved-codes]").click();
  // No machine yet (#98): install, join (by full path) and approve, as
  // numbered steps, each command with Copy.
  await expect(laptop.getByRole("heading", { name: "Add a machine" })).toBeVisible();
  await expect(laptop.locator(".control-steps > li")).toHaveCount(3);
  await expect(laptop.locator("[data-install]")).toHaveText("curl -fsSL https://arugula.io/install.sh | sh");
  await expect(laptop.locator("[data-install-windows]")).toHaveText("irm https://arugula.io/install.ps1 | iex");
  const join = `~/.local/bin/arugulad join ${base}`;
  await expect(laptop.locator("[data-join-cmd]")).toHaveText(join);
  await expect(laptop.locator("[data-join-cmd-windows]")).toHaveText(`arugulad join ${base}`);
  await expect(laptop.locator(".control-steps")).toContainText("Codes last 15 minutes.");
  await expect(laptop.locator(".control-add")).toContainText("a device (this browser, your phone) reaches them");
  const copyJoin = laptop.locator("[data-join-cmd] + [data-copy]");
  await copyJoin.click();
  expect(await clipboard()).toBe(join);
  // With the clipboard blocked, Copy selects the command instead.
  await laptop.evaluate(() => {
    navigator.clipboard.writeText = () => Promise.reject(new DOMException("blocked", "NotAllowedError"));
    document.execCommand = () => false;
  });
  await copyJoin.click();
  await expect(copyJoin).toHaveText("Selected");
  expect(await laptop.evaluate(() => getSelection()!.toString())).toBe(join);
});

test("with no machine yet, the account's menu is there (#97)", async () => {
  await expect(laptop.locator("[data-signed-in-as]")).toHaveText("stranger");
  // Devices: this browser, and a passkey could be added.
  const menu = async (item: string) => {
    await laptop.locator("[data-account-bar] [data-account-menu]").click();
    await laptop.getByRole("menuitem", { name: item }).click();
  };
  await menu("Devices and machines…");
  await expect(laptop.getByRole("heading", { name: "Devices and machines" })).toBeVisible();
  await expect(laptop.locator(".control-devices li")).toHaveCount(1);
  await laptop.locator(".prompt").getByRole("button", { name: "Done" }).click();
  // A team.
  await menu("Teams…");
  await laptop.getByLabel("Team name").fill("Solo");
  await laptop.getByRole("button", { name: "Make a team" }).click();
  await expect(laptop.locator("[data-team] h3")).toHaveText("Solo");
  await laptop.locator(".prompt").getByRole("button", { name: "Done" }).click();
  // The plan, where billing is on (faked here: no Stripe in tests), is the
  // platform page's (platform.spec.ts): with none, the menu doesn't offer
  // it (M155).
  await laptop.route("**/api/billing", (r) =>
    r.fulfill({
      json: { billing: true, plan: "personal", relay: { bytes: 0, allowance: 1e9, warning: false, slowed: false }, sandbox_minutes: 0, teams: [] },
    }),
  );
  await laptop.evaluate(() => window.__arugula.control!.refresh());
  await laptop.locator("[data-account-bar] [data-account-menu]").click();
  await expect(laptop.getByRole("menuitem", { name: "Teams…" })).toBeVisible();
  await expect(laptop.getByRole("menuitem", { name: /^Plan and usage/ })).toHaveCount(0);
  await laptop.keyboard.press("Escape");
  await laptop.unroute("**/api/billing");
  // Owning one, step 2 mentions --team.
  await expect(laptop.locator(".control-steps")).toContainText("--team");
  // Signing out, and the sign-in page says why a #join= link came here
  // (#103); signing back in keeps this browser's place.
  await menu("Sign out");
  await expect(laptop.locator("[data-signin=github]")).toBeVisible();
  // Signing out goes to "/" itself: let that load land first, or it can
  // overtake the link's.
  await laptop.goto("/");
  await laptop.goto("/#join=ABCDE-FGHIJ");
  await expect(laptop.locator("[data-why=join]")).toHaveText("Sign in to approve this machine.");
  await laptop.goto("/");
  await signIn(laptop);
  await booted(laptop);
  await expect(laptop.locator("[data-signed-in-as]")).toHaveText("stranger");
});

test("two machines join by code; one direct, one only through the relay", async () => {
  await addMachine(laptop, "box", true);
  // #760: an older daemon still joins, through the compare path.
  await addMachine(laptop, "mac", false, true);
  await laptop.goto("/");
  await expect.poll(() => hostNames(laptop), { timeout: 20_000 }).toEqual(["box", "mac"]);
  await expect
    .poll(() => laptop.evaluate(() => window.__arugula.control!.daemons.every((d) => d.online)), { timeout: 20_000 })
    .toBe(true);

  await showHost(laptop, "box");
  await shell(laptop, "box");
  expect(await laptop.evaluate(() => window.__arugula.client.path)).toBe("direct");

  await expect(laptop.locator(".ws-head [data-connection]")).toHaveAttribute("data-connection", "direct");

  await showHost(laptop, "mac");
  await shell(laptop, "mac");
  expect(await laptop.evaluate(() => window.__arugula.client.path)).toBe("relayed");
  await expect(laptop.locator(".ws-head [data-connection]")).toHaveAttribute("data-connection", "relayed");

  // M70: a pasted image goes through the relay in chunks (2.5 MB: three),
  // inside the end-to-end channel, and lands on the machine whole.
  const pane = await laptop.evaluate(() => window.__arugula.client.active()!);
  const png = Buffer.from(Array.from({ length: 2_500_000 }, (_, i) => (i * 7) % 256));
  await pasteFile(laptop, pane, png, "relayed.png", "image/png");
  expect(readFileSync(await uploadedPath(laptop, pane))).toEqual(png);
  // The path waits on the prompt line: clear it for what's typed next.
  await laptop.evaluate((p) => window.__arugula.client.input(p, new TextEncoder().encode("\x15")), pane);
});

test("a relayed page that goes leaves the machine's clients (presence) too", async () => {
  const viewers = () => laptop.evaluate(() => window.__arugula.client.state?.presence?.length ?? 0);
  const before = await viewers();
  // Each page that came and went stayed a client of the mux until the
  // daemon restarted: hundreds, slowing every keystroke.
  for (let i = 0; i < 3; i++) {
    const other = await laptop.context().newPage();
    await other.goto("/");
    await booted(other);
    await showHost(other, "mac");
    await expect.poll(viewers, { timeout: 20_000 }).toBe(before + 1);
    await other.close();
    await expect.poll(viewers, { timeout: 20_000 }).toBe(before);
  }
});

/** #761: the code a waiting page shows, `LOOKUP.SECRET`. */
async function deviceCode(page: Page): Promise<string> {
  const code = await page.locator("[data-device-code]").getAttribute("data-device-code", { timeout: 20_000 });
  expect(code).toMatch(/^[A-Z0-9]{5}-[A-Z0-9]{5}\.[A-Z0-9]{4}(-[A-Z0-9]{4}){3}$/);
  return code!;
}

async function phoneContext(browser: Browser) {
  return browser.newContext({ viewport: { width: 390, height: 760 }, isMobile: true, hasTouch: true });
}

test("a phone needs the laptop's approval", async ({ browser }) => {
  phone = await (await phoneContext(browser)).newPage();
  await signIn(phone);
  await expect(phone.getByText("Approve this browser")).toBeVisible();
  // It says where to approve it (#105), and that a machine's join is
  // another approval that doesn't need this one (#326).
  await expect(phone.locator("[data-control-url]")).toHaveText(base);
  await expect(phone.locator("[data-waiting-machine]")).toContainText("doesn't need this one");
  await expect(phone.locator("[data-sign-out]")).toBeVisible();
  // #761: it shows a code, as a QR code and as text, and its fingerprint
  // only for an older approving page, a click away.
  const code = await deviceCode(phone);
  await expect(phone.locator("svg[data-qr]")).toHaveAttribute("data-qr", `${base}/#device=${code}`);
  await expect(phone.locator("[data-fingerprint]")).toBeHidden();
  const fp = await phone.locator("[data-fingerprint]").getAttribute("data-fingerprint");
  // The laptop is asked, and approves with the code: nothing to compare.
  await expect(laptop.locator(`[data-pending="${fp}"][data-device-sealed]`)).toBeVisible({ timeout: 20_000 });
  await expect(laptop.locator(".prompt .fingerprint")).toHaveCount(0);
  await expect(laptop.locator("[data-approve]")).toBeDisabled();
  // Its first half is shown, fixed: only the rest is typed.
  const [lookup, rest] = code.split(".");
  await expect(laptop.locator("[data-device-rest] .control-rest-known")).toHaveText(`${lookup}.`);
  // Another device's whole code, pasted, isn't this one's.
  await laptop.locator("[data-device-whole]").fill("ABCDE-FGHJK.ABCD-EFGH-JKMN-PQRS");
  await expect(laptop.locator("[data-device-rest-error]")).toBeVisible();
  // It shows once and whole: no fixed half before it, no second dot.
  await expect(laptop.locator("[data-device-whole]")).toHaveValue("ABCDE-FGHJK.ABCD-EFGH-JKMN-PQRS");
  await expect(laptop.locator("[data-device-rest] .control-rest-known")).toBeHidden();
  await expect(laptop.locator("[data-approve]")).toBeDisabled();
  // Half of the rest: still waiting for it.
  await laptop.locator("[data-device-whole]").fill(rest.slice(0, 9).toLowerCase());
  await expect(laptop.locator("[data-approve]")).toBeDisabled();
  await laptop.locator("[data-device-whole]").fill(rest.toLowerCase().replace(/-/g, ""));
  await expect(laptop.locator("[data-device-whole]")).toHaveValue(rest);
  await laptop.locator("[data-approve]").click();
  await expect.poll(() => hostNames(phone), { timeout: 20_000 }).toEqual(["box", "mac"]);
  // It pinned the laptop's root, without asking.
  expect(await phone.evaluate(() => window.__arugula.control!.enrollment!.root)).toBe(await laptop.evaluate(() => window.__arugula.control!.enrollment!.root));
  await showHost(phone, "mac");
  await shell(phone, "phone");
});

test("a browser scanned from a phone: its QR code opens the approval with the code in it (#761)", async ({ browser }) => {
  const other = await (await browser.newContext()).newPage();
  await signIn(other);
  const code = await deviceCode(other);
  const link = (await other.locator("svg[data-qr]").getAttribute("data-qr"))!;
  const id = await other.evaluate(() => window.__arugula.control!.keys.id);
  // What a phone's camera opens: the link, on a device of the account.
  await laptop.goto(link);
  await expect(laptop.locator(`[data-pending="${id}"][data-device-sealed]`)).toBeVisible({ timeout: 20_000 });
  await expect(laptop.locator("[data-device-whole]")).toHaveCount(0);
  await laptop.locator("[data-approve]").click();
  await booted(other);
  await expect(laptop).not.toHaveURL(/#device=/);
  // A link whose code nobody shows any more says so.
  await laptop.goto(`${base}/#device=${code}`);
  await expect(laptop.locator("[data-device-gone]")).toBeVisible({ timeout: 20_000 });
  await laptop.goto(base);
  await booted(laptop);
  await laptop.evaluate((d) => window.__arugula.control!.revoke(d), id);
  await other.context().close();
});

test("a new browser refuses an approval that doesn't answer its code (#761)", async ({ browser }) => {
  const other = await (await browser.newContext()).newPage();
  await signIn(other);
  const code = await deviceCode(other);
  const id = await other.evaluate(() => window.__arugula.control!.keys.id);
  await expect(laptop.locator(`[data-pending="${id}"]`)).toBeVisible({ timeout: 20_000 });
  // Its lookup with another secret: what control would have to guess, or
  // a slip typing it. The laptop approves; the browser doesn't believe it.
  const wrong = `${code.split(".")[0]}.ABCD-EFGH-JKMN-PQRS`;
  await laptop.locator("[data-device-whole]").fill(wrong);
  await laptop.locator("[data-approve]").click();
  await expect(other.locator('[data-unvouched="wrong"]')).toBeVisible({ timeout: 20_000 });
  expect(await other.evaluate(() => window.__arugula.control!.enrollment)).toBeUndefined();
  // Reloaded, it still doesn't: the code was its one guess.
  await other.reload();
  await expect(other.locator('[data-unvouched="wrong"]')).toBeVisible({ timeout: 20_000 });
  await expect(other.locator("[data-enroll-again]")).toBeVisible();
  await laptop.evaluate((d) => window.__arugula.control!.revoke(d), id);
  await other.context().close();
});

test("an approving page from before codes still approves; the new browser checks the account instead (#761)", async ({ browser }) => {
  const other = await (await browser.newContext()).newPage();
  await signIn(other);
  await deviceCode(other);
  const id = await other.evaluate(() => window.__arugula.control!.keys.id);
  // Its fingerprint is there for that page to compare.
  await other.locator("[data-waiting-older] summary").click();
  await expect(other.locator(`[data-fingerprint="${id}"]`)).toBeVisible();
  await expect(laptop.locator(`[data-pending="${id}"]`)).toBeVisible({ timeout: 20_000 });
  // An older page signs the certificate and sends no MAC.
  await laptop.evaluate(async (d) => {
    const s = window.__arugula.control as unknown as { pending: { device: string }[]; pendingSealed: Set<string>; approve(c: unknown): Promise<void> };
    s.pendingSealed.delete(d);
    await s.approve(s.pending.find((c) => c.device === d));
  }, id);
  // The browser doesn't take control's word for the account: it shows the
  // account's fingerprint to check on the laptop, the old way.
  const root = await laptop.evaluate(() => window.__arugula.control!.enrollment!.root);
  await expect(other.locator('[data-unvouched="missing"]')).toBeVisible({ timeout: 20_000 });
  await expect(other.locator(`[data-unvouched-account="${root}"]`)).toBeVisible();
  await other.locator("[data-vouch]").click();
  await booted(other);
  expect(await other.evaluate(() => window.__arugula.control!.enrollment!.root)).toBe(root);
  await laptop.evaluate((d) => window.__arugula.control!.revoke(d), id);
  await other.context().close();
});

test("an older desktop app signs in through the browser with a code, then is approved as a device (M48)", async ({ browser }) => {
  // The app keeps a verifier, listens on a loopback port for its grant,
  // asks for a ticket, and opens its page in the person's browser. An app
  // from before #758 sends no keys and shows a code, and so does the page;
  // its window is then approved as a device on the prompt (the same
  // fallback a newer app gets when Allow can't sign its keys).
  const verifier = randomBytes(32).toString("hex");
  const challenge = createHash("sha256").update(verifier).digest("hex");
  let grant = "";
  const loop = createServer((req, res) => {
    const u = new URL(req.url!, "http://127.0.0.1");
    if (u.pathname === "/arugula-signin") grant = u.searchParams.get("grant") ?? "";
    res.writeHead(303, { location: `${base}/#app-done` }).end();
  });
  await new Promise<void>((r) => loop.listen(0, "127.0.0.1", r));
  const port = (loop.address() as { port: number }).port;
  // An app from before the loopback hand-over is told to update.
  const old = await fetch(`${base}/auth/app`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name: "Arugula app on test-mac" }),
  });
  expect(old.status).toBe(400);
  expect(((await old.json()) as { error: string }).error).toContain("update it");
  const ask = await fetch(`${base}/auth/app`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name: "Arugula app on test-mac", challenge, port }),
  });
  expect(ask.status).toBe(200);
  const t = (await ask.json()) as { ticket: string; code: string; url: string };
  expect(t.url).toBe(`${base}/#app=${t.ticket}`);
  const redeem = (body: unknown, origin = base) =>
    fetch(`${base}/auth/app/${t.ticket}/redeem`, { method: "POST", headers: { "content-type": "application/json", origin }, body: JSON.stringify(body) });
  // A redeem before the person allows it gets nothing; nor does a GET.
  expect((await redeem({ grant: "00", verifier })).status).toBe(404);
  expect((await fetch(`${base}/auth/app/${t.ticket}/redeem?grant=00&verifier=${verifier}`, { redirect: "manual" })).status).toBe(405);

  // In the browser (signed in), the same code, and Allow: the grant goes
  // to the app's port, and the browser comes back to control's page.
  await laptop.goto(t.url);
  await expect(laptop.locator("[data-app-login-name]")).toHaveText("Arugula app on test-mac");
  await expect(laptop.locator("[data-app-login-code]")).toHaveText(t.code);
  await expect(laptop.locator("[data-app-login-elsewhere]")).toHaveCount(0);
  await laptop.locator("[data-app-login-allow]").click();
  await expect(laptop.locator("[data-app-login-done]")).toBeVisible();
  expect(grant).toMatch(/^[0-9a-f]{64}$/);
  loop.close();
  // The grant alone (someone who saw the link) or from another site does nothing.
  expect((await redeem({ grant, verifier: "0".repeat(64) })).status).toBe(404);
  expect((await redeem({ grant, verifier }, "https://elsewhere.example")).status).toBe(403);
  // In a browser rather than the app's window, #app-redeem doesn't sign in.
  const other = await (await browser.newContext()).newPage();
  await other.goto(`${base}/#app-redeem=${t.ticket}.${grant}.${verifier}`);
  await expect(other.locator("[data-signin]").first()).toBeVisible();
  await other.context().close();

  // The app's window redeems it: signed in, then a new device to approve.
  const app = await (await browser.newContext()).newPage();
  await app.addInitScript(() => Object.assign(window, { __arugulaApp: { name: "Arugula app on test-mac" } }));
  await app.goto(`${base}/#app-redeem=${t.ticket}.${grant}.${verifier}`);
  // It says it's the app being approved, by name (#326), and asks as an
  // app device.
  await expect(app.getByText("Approve this app as a device")).toBeVisible();
  await expect(app.locator("[data-waiting-browser]")).toContainText("the Arugula app on test-mac");
  const code = await deviceCode(app);
  const fp = await app.locator("[data-fingerprint]").getAttribute("data-fingerprint");
  await expect(laptop.locator(`[data-pending="${fp}"]`)).toBeVisible({ timeout: 20_000 });
  await expect(laptop.locator(".prompt")).toContainText("Arugula app on test-mac");
  // #761: the app's window shows a code too; typed on the laptop.
  await laptop.locator("[data-device-whole]").fill(code);
  await laptop.locator("[data-approve]").click();
  await expect.poll(() => hostNames(app), { timeout: 20_000 }).toEqual(["box", "mac"]);
  await showHost(app, "mac");
  await shell(app, "app");

  // Single use.
  expect((await redeem({ grant, verifier })).status).toBe(404);

  // Removing the app's device in control cuts it off at once.
  const id = await app.evaluate(() => window.__arugula.control!.keys.id);
  await laptop.evaluate((d) => window.__arugula.control!.revoke(d), id);
  await expect.poll(() => connected(app), { timeout: 5_000, intervals: [200] }).toBe(false);
  await app.context().close();
});

test("the desktop app signs in through the browser with no code, and Allow makes it a device (#758)", async ({ browser }) => {
  // What crates/desktop/dist/signin.html makes: the window's device keys.
  const subtle = globalThis.crypto.subtle;
  const hexOf = (b: ArrayBuffer) => Buffer.from(b).toString("hex");
  const noise = (await subtle.generateKey({ name: "X25519" }, true, ["deriveBits"])) as CryptoKeyPair;
  const sign = (await subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"])) as CryptoKeyPair;
  const keys = {
    noise: hexOf(await subtle.exportKey("raw", noise.publicKey)),
    sign: hexOf(await subtle.exportKey("raw", sign.publicKey)),
    secretNoise: hexOf(await subtle.exportKey("pkcs8", noise.privateKey)),
    secretSign: hexOf(await subtle.exportKey("pkcs8", sign.privateKey)),
  };
  const verifier = randomBytes(32).toString("hex");
  const challenge = createHash("sha256").update(verifier).digest("hex");
  let grant = "";
  let cert = "";
  const loop = createServer((req, res) => {
    const u = new URL(req.url!, "http://127.0.0.1");
    if (u.pathname === "/arugula-signin") {
      grant = u.searchParams.get("grant") ?? "";
      cert = u.searchParams.get("cert") ?? "";
    }
    res.writeHead(303, { location: `${base}/#app-done` }).end();
  });
  await new Promise<void>((r) => loop.listen(0, "127.0.0.1", r));
  const port = (loop.address() as { port: number }).port;
  const ask = await fetch(`${base}/auth/app`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name: "Arugula app on test-air", challenge, port, features: ["no-code"], device: { noise: keys.noise, sign: keys.sign } }),
  });
  expect(ask.status).toBe(200);
  const t = (await ask.json()) as { ticket: string; code?: string; url: string };
  expect(t.code).toBeUndefined();

  // The browser (a device) shows no code, and says Allow makes it a device.
  await laptop.goto(t.url);
  await expect(laptop.locator("[data-app-login-name]")).toHaveText("Arugula app on test-air");
  await expect(laptop.locator("[data-app-login-code]")).toHaveCount(0);
  await expect(laptop.locator("[data-app-login-device]")).toBeVisible();
  await laptop.locator("[data-app-login-allow]").click();
  await expect(laptop.locator("[data-app-login-done]")).toContainText("one of your devices now");
  loop.close();
  expect(grant).toMatch(/^[0-9a-f]{64}$/);
  // The certificate went to the app, not to control: signed by the laptop,
  // for the app's keys, with the root the laptop pinned.
  const handed = JSON.parse(Buffer.from(cert, "base64url").toString()) as { cert: { noise: string; sign: string; approver: string }; root: string };
  expect(handed.cert.noise).toBe(keys.noise);
  expect(handed.cert.sign).toBe(keys.sign);
  const laptopId = await laptop.evaluate(() => window.__arugula.control!.keys.id);
  expect(handed.cert.approver).toBe(laptopId);
  // An app device, not a browser: whatever listens on the port gets it.
  expect((handed.cert as { kind?: string }).kind).toBe("app");

  // The app's window takes its keys and the certificate: a device at once,
  // with no "Approve this app as a device" screen.
  const app = await (await browser.newContext()).newPage();
  await app.addInitScript(() => Object.assign(window, { __arugulaApp: { name: "Arugula app on test-air" } }));
  await app.goto(`${base}/#app-redeem=${t.ticket}.${grant}.${verifier}.${keys.secretNoise}.${keys.secretSign}.${cert}`);
  await expect.poll(() => hostNames(app), { timeout: 20_000 }).toEqual(["box", "mac"]);
  await expect(app.getByText("Approve this app as a device")).toHaveCount(0);
  expect(await app.evaluate(() => window.__arugula.control!.keys.signPub)).toBe(keys.sign);
  expect(await app.evaluate(() => window.__arugula.control!.enrollment!.cert.kind)).toBe("app");
  // It approves machines, not browsers or phones; the laptop approves both.
  expect(await app.evaluate(() => [window.__arugula.control!.canApprove("daemon"), window.__arugula.control!.canApprove("browser")])).toEqual([true, false]);
  expect(await laptop.evaluate(() => window.__arugula.control!.canApprove("browser"))).toBe(true);
  await showHost(app, "mac");
  await shell(app, "air");
  const id = await app.evaluate(() => window.__arugula.control!.keys.id);
  await laptop.evaluate((d) => window.__arugula.control!.revoke(d), id);
  await app.context().close();
});

test("a desktop app's keys swapped by control aren't signed for anyone else: the app asks to be approved (#758)", async ({ browser }) => {
  // The certificate the browser hands over is for the keys control showed
  // it; the app's window checks them against its own and, if they differ,
  // asks to be approved like any new device.
  const subtle = globalThis.crypto.subtle;
  const hexOf = (b: ArrayBuffer) => Buffer.from(b).toString("hex");
  const pair = async () => {
    const noise = (await subtle.generateKey({ name: "X25519" }, true, ["deriveBits"])) as CryptoKeyPair;
    const sign = (await subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"])) as CryptoKeyPair;
    return {
      noise: hexOf(await subtle.exportKey("raw", noise.publicKey)),
      sign: hexOf(await subtle.exportKey("raw", sign.publicKey)),
      secretNoise: hexOf(await subtle.exportKey("pkcs8", noise.privateKey)),
      secretSign: hexOf(await subtle.exportKey("pkcs8", sign.privateKey)),
    };
  };
  const [mine, shown] = [await pair(), await pair()];
  const verifier = randomBytes(32).toString("hex");
  const challenge = createHash("sha256").update(verifier).digest("hex");
  let grant = "";
  let cert = "";
  const loop = createServer((req, res) => {
    const u = new URL(req.url!, "http://127.0.0.1");
    if (u.pathname === "/arugula-signin") {
      grant = u.searchParams.get("grant") ?? "";
      cert = u.searchParams.get("cert") ?? "";
    }
    res.writeHead(303, { location: `${base}/#app-done` }).end();
  });
  await new Promise<void>((r) => loop.listen(0, "127.0.0.1", r));
  const port = (loop.address() as { port: number }).port;
  // As if control had put other keys in the ticket than the app sent.
  const ask = await fetch(`${base}/auth/app`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name: "Arugula app on test-swap", challenge, port, features: ["no-code"], device: { noise: shown.noise, sign: shown.sign } }),
  });
  const t = (await ask.json()) as { ticket: string; url: string };
  await laptop.goto(t.url);
  await laptop.locator("[data-app-login-allow]").click();
  await expect(laptop.locator("[data-app-login-done]")).toBeVisible();
  loop.close();
  expect(cert).not.toBe("");

  const app = await (await browser.newContext()).newPage();
  await app.addInitScript(() => Object.assign(window, { __arugulaApp: { name: "Arugula app on test-swap" } }));
  await app.goto(`${base}/#app-redeem=${t.ticket}.${grant}.${verifier}.${mine.secretNoise}.${mine.secretSign}.${cert}`);
  await expect(app.getByText("Approve this app as a device")).toBeVisible({ timeout: 20_000 });
  expect(await app.evaluate(() => window.__arugula.control!.keys.signPub)).not.toBe(shown.sign);
  // Nothing was enrolled for the keys control showed.
  const devices = await laptop.evaluate(() => [...window.__arugula.control!.trusted.values()].map((c) => c.sign));
  expect(devices).not.toContain(shown.sign);
  const fp = await app.locator("[data-fingerprint]").getAttribute("data-fingerprint");
  await laptop.goto(base);
  await expect(laptop.locator(`[data-pending="${fp}"]`)).toBeVisible({ timeout: 20_000 });
  await laptop.locator("[data-reject]").click().catch(() => {});
  await app.context().close();
});

test("with every device lost, a recovery code lets a new browser in, once", async ({ browser }) => {
  const fresh = await (await browser.newContext()).newPage();
  await signIn(fresh);
  await expect(fresh.getByText("Approve this browser")).toBeVisible();
  await fresh.locator("[data-use-recovery]").click();
  await fresh.getByLabel("Recovery code").fill(recoveryCodes[0]);
  await fresh.getByRole("button", { name: "Use it" }).click();
  await expect.poll(() => hostNames(fresh), { timeout: 20_000 }).toEqual(["box", "mac"]);
  // The code is spent: another browser can't use it again.
  const again = await (await browser.newContext()).newPage();
  await signIn(again);
  await again.locator("[data-use-recovery]").click();
  await again.getByLabel("Recovery code").fill(recoveryCodes[0]);
  await again.getByRole("button", { name: "Use it" }).click();
  await expect(again.locator(".control-error")).toContainText("isn't one of this account's recovery codes");
  // The laptop (still enrolled) turns the second browser down, and the
  // browser hears which device did (#105). It can ask again.
  const laptopName = await laptop.evaluate(() => window.__arugula.control!.enrollment!.cert.name);
  await laptop.locator("[data-reject]").click();
  await expect(again.locator("[data-turned-down]")).toContainText(`${laptopName} turned this browser down`, { timeout: 10_000 });
  await again.locator("[data-try-again]").click();
  await expect(again.getByText("Approve this browser")).toBeVisible();
  await expect(laptop.locator("[data-pending]")).toBeVisible({ timeout: 20_000 });
  await laptop.locator("[data-reject]").click();
  await expect(again.locator("[data-turned-down]")).toBeVisible({ timeout: 10_000 });
});

test("add a phone or browser: the control URL as a QR code and a link", async () => {
  await controlPanel(laptop, "add-device");
  await expect(laptop.getByRole("heading", { name: "Add a phone or browser" })).toBeVisible();
  await expect(laptop.locator("svg[data-qr]")).toHaveAttribute("data-qr", base);
  expect((await laptop.locator("svg[data-qr] path").getAttribute("d"))!.length).toBeGreaterThan(100);
  await expect(laptop.locator(".prompt [data-control-url]")).toHaveText(base);
  await expect(laptop.locator(".control-steps li")).toHaveCount(3);
  await laptop.getByRole("button", { name: "Done" }).click();
});

test("devices and machines, grouped; new recovery codes retire the old", async ({ browser }) => {
  await controlPanel(laptop, "devices");
  await expect(laptop.locator("[data-account]")).toHaveText("stranger");
  const machines = laptop.locator("[data-machines] li");
  await expect(machines).toHaveCount(2);
  await expect(machines.filter({ hasText: /^box\b/ }).locator("[data-status]")).toContainText("online · direct");
  await expect(machines.filter({ hasText: /^mac\b/ }).locator("[data-status]")).toContainText("online · relayed");
  // Removing a machine asks in a dialog that says what happens to it. A
  // double-click on Remove only opens it (#328 lost a Mac that way).
  const mac = await machines.filter({ hasText: /^mac\b/ }).getAttribute("data-device");
  await machines.filter({ hasText: /^mac\b/ }).locator("[data-remove]").dblclick();
  await new Promise((r) => setTimeout(r, 1000));
  expect(await laptop.evaluate((d) => window.__arugula.control!.trusted.has(d), mac!)).toBe(true);
  const ask = laptop.locator("[data-confirm-dialog]");
  await expect(ask).toContainText("Remove mac?");
  await expect(ask.locator("[data-remove-explain]")).toContainText("keeps running on it, reachable only locally");
  await ask.locator("[data-cancel-remove]").click();
  await expect(ask).toHaveCount(0);
  await expect(machines).toHaveCount(2);
  // The laptop, the phone and the browser the recovery code let in.
  await expect(laptop.locator("[data-browsers] li")).toHaveCount(3);
  await expect(laptop.locator("[data-browsers] li [data-this-browser]")).toHaveCount(1);
  // One code was spent: Sign-in and account says so.
  await laptop.getByRole("button", { name: "Done" }).click();
  await controlPanel(laptop, "account");
  await expect(laptop.locator("[data-recovery-left]")).toHaveAttribute("data-recovery-left", "1");
  await laptop.locator("[data-new-codes]").click();
  await laptop.locator("[data-new-codes]").click();
  await expect(laptop.locator("[data-recovery-code]")).toHaveCount(2);
  const fresh = await laptop.locator("[data-recovery-code]").allTextContents();
  expect(fresh).not.toContain(recoveryCodes[1]);
  await laptop.locator("[data-stored-codes]").check();
  await laptop.locator("[data-saved-codes]").click();
  await expect(laptop.locator("[data-recovery-left]")).toHaveAttribute("data-recovery-left", "2");
  await laptop.getByRole("button", { name: "Done" }).click();

  // The old code (never used) no longer works; a new one does.
  const other = await (await browser.newContext()).newPage();
  await signIn(other);
  await other.locator("[data-use-recovery]").click();
  await other.getByLabel("Recovery code").fill(recoveryCodes[1]);
  await other.getByRole("button", { name: "Use it" }).click();
  await expect(other.locator(".control-error")).toContainText("isn't one of this account's recovery codes");
  await other.getByLabel("Recovery code").fill(fresh[0]);
  await other.getByRole("button", { name: "Use it" }).click();
  await expect.poll(() => hostNames(other), { timeout: 20_000 }).toEqual(["box", "mac"]);
});

test("removing the phone cuts it off, once confirmed in a dialog", async () => {
  const id = await phone.evaluate(() => window.__arugula.control!.keys.id);
  await controlPanel(laptop, "devices");
  // A double-click on Remove opens the dialog and removes nothing.
  await laptop.locator(`[data-browsers] [data-remove="${id}"]`).dblclick();
  await new Promise((r) => setTimeout(r, 1000));
  expect(await laptop.evaluate((d) => window.__arugula.control!.trusted.has(d), id)).toBe(true);
  expect(await connected(phone)).toBe(true);
  const ask = laptop.locator("[data-confirm-dialog]");
  await expect(ask).toContainText("loses access to your machines at once");
  await ask.locator("[data-confirm-remove]").click();
  await expect(ask).toHaveCount(0);
  await expect(laptop.locator(`[data-browsers] [data-remove="${id}"]`)).toHaveCount(0);
  await laptop.getByRole("button", { name: "Done" }).click();
  // Control nudges the daemons, which refresh, close its channel and
  // refuse it from then on.
  await expect.poll(() => connected(phone), { timeout: 5_000, intervals: [200] }).toBe(false);
  await new Promise((r) => setTimeout(r, 3000));
  expect(await connected(phone)).toBe(false);
});

test("with no sessions on the machine shown, control's page still has the host and account menus", async () => {
  await showHost(laptop, "box");
  const none = () => laptop.evaluate(() => window.__arugula.client.state?.sessions.length ?? -1);
  await expect.poll(none).toBeGreaterThan(0);
  const closeAll = () =>
    laptop.evaluate(() => {
      const c = window.__arugula.client;
      for (const s of c.state!.sessions) c.intent({ op: "close_session", session: s.id });
    });
  await closeAll();
  await expect.poll(none).toBe(0);
  // It says which machine this is and where to switch.
  await expect(laptop.locator("[data-no-sessions-host]")).toHaveText("Not the machine you meant? Switch from the machine's name at the top left.");
  await expect(laptop.locator("[data-no-sessions] .empty-title")).toHaveText("No sessions on box.");
  await expect(laptop.locator(".ws-head .host-button")).toHaveText(/box/);
  // The account's items are the account menu's, on the right, not the
  // host menu's.
  await laptop.locator(".host-button").click();
  await expect(laptop.getByRole("menuitem", { name: "Add a machine…" })).toBeVisible();
  await expect(laptop.getByRole("menuitem", { name: "Teams…" })).toHaveCount(0);
  await laptop.keyboard.press("Escape");
  await laptop.locator(".ws-head [data-account-menu]").click();
  await expect(laptop.locator(".ws-head [data-signed-in-as]")).toHaveText("stranger");
  await expect(laptop.getByRole("menuitem", { name: "Teams…" })).toBeVisible();
  await expect(laptop.getByRole("menuitem", { name: "Sign out" })).toBeVisible();
  await laptop.getByRole("menuitem", { name: "Devices and machines…" }).click();
  await expect(laptop.getByRole("heading", { name: "Devices and machines" })).toBeVisible();
  await laptop.locator(".prompt").getByRole("button", { name: "Done" }).click();
  // The other machine is reached from there, with no session made.
  await laptop.locator(".host-button").click();
  await laptop.getByRole("menuitem", { name: /^\s*mac\s/ }).click();
  await expect.poll(() => laptop.evaluate(() => window.__arugula.hosts.current), { timeout: 20_000 }).toBe("mac");
  await showHost(laptop, "box");
  // Leave box with a session, as the next tests found it.
  await expect(laptop.locator("[data-no-sessions-host]")).toBeVisible();
  await laptop.getByRole("button", { name: "New session" }).click();
  await expect.poll(none).toBe(1);
});

test("a browser the account removed says so before offering Approve, and enrolls again (#327)", async () => {
  // The phone was removed above. Opened again (here on a machine's join
  // link), it checks its own key first: no Approve button that would fail.
  await phone.goto(`${base}/#join=AAAAA-AAAAA`);
  await expect(phone.locator('[data-untrusted="removed"]')).toBeVisible({ timeout: 20_000 });
  await expect(phone.locator("[data-approve-join]")).toHaveCount(0);
  await expect(phone.locator("[data-sign-out-forget]")).toBeVisible();
  // Forgotten, it asks as a new device, with the recovery form open.
  await phone.locator("[data-enroll-again]").click();
  // Still on the machine's link, it leads with that (#326).
  await expect(phone.locator('[data-waiting-join="AAAAA-AAAAA"]')).toBeVisible();
  await expect(phone.locator("[data-recovery-input]")).toBeVisible();
  // Not wanted back: the laptop turns it down, so nothing waits on it.
  await expect(laptop.locator("[data-pending]")).toBeVisible({ timeout: 20_000 });
  await laptop.locator("[data-reject]").click();
  await expect(phone.locator("[data-turned-down]")).toBeVisible({ timeout: 10_000 });
});

test("Getting started joins as soon as a device approves: nothing to compare (#760)", async ({ browser }) => {
  const state = temp("sealed");
  procs.push(
    spawn(
      "../target/debug/arugulad",
      [
        ...["--listen", ANY, "--name", "sealed", "--state-dir", labs(state), "--control", base],
        ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
      ],
      { stdio: "ignore" },
    ),
  );
  const local = `http://127.0.0.1:${await daemonPort(state, procs.at(-1))}`;
  const page = await (await browser.newContext()).newPage();
  await page.goto(local);
  await expect(page.locator(".session-button")).toBeVisible();
  const start = page.getByRole("dialog", { name: "Getting started" });
  if (!(await start.isVisible())) {
    await page.locator(".ws-head [data-account-menu]").click();
    await page.getByRole("menuitem", { name: "Getting started" }).click();
  }
  await start.locator('[data-start-seg="cloud"]').click();
  await start.locator("[data-start-connect]").click();
  await expect(start.locator("[data-start-approve]")).toBeVisible({ timeout: 20_000 });
  // The link carries the secret after the code; the panel shows both, for
  // typing where the link can't be opened.
  const link = (await start.locator("[data-start-approve]").getAttribute("href"))!;
  const whole = link.split("#join=")[1];
  expect(whole).toMatch(/^[A-Z0-9]{5}-[A-Z0-9]{5}\.[A-Z0-9]{4}(-[A-Z0-9]{4}){3}$/);
  await expect(start.locator("[data-start-pending] [data-start-code]")).toHaveText(whole);
  await laptop.goto(link);
  await expect(laptop.locator("[data-join-sealed]")).toBeVisible();
  await expect(laptop.locator("[data-join-code]")).toHaveCount(0);
  await expect(laptop.locator("[data-join-account]")).toHaveCount(0);
  await laptop.locator("[data-approve-join]").click();
  // Straight to joined: no fingerprint to check.
  await expect(start.locator("[data-start-joined]")).toHaveText("Joined to your account", { timeout: 20_000 });
  await expect(start.locator("[data-start-confirm]")).toHaveCount(0);
  await page.context().close();
  await laptop.goto("/");
  await expect.poll(() => hostNames(laptop), { timeout: 20_000 }).toContain("sealed");
});

test("an older daemon: Getting started asks to check the account's fingerprint before the machine trusts it", async ({ browser }) => {
  const state = temp("starter");
  procs.push(
    spawn(
      "../target/debug/arugulad",
      [
        ...["--listen", ANY, "--name", "starter", "--state-dir", labs(state), "--control", base],
        ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
      ],
      // From before #760: no join secret, so the account is compared.
      { stdio: "ignore", env: { ...process.env, ...BEFORE_SECRETS } },
    ),
  );
  const local = `http://127.0.0.1:${await daemonPort(state, procs.at(-1))}`;
  const page = await (await browser.newContext()).newPage();
  await page.goto(local);
  await expect(page.locator(".session-button")).toBeVisible();
  const start = page.getByRole("dialog", { name: "Getting started" });
  const cloud = async () => {
    if (!(await start.isVisible())) {
      await page.locator(".ws-head [data-account-menu]").click();
      await page.getByRole("menuitem", { name: "Getting started" }).click();
    }
    await start.locator('[data-start-seg="cloud"]').click();
  };
  // The daemon's --control (#207): the button asks this test's control
  // for a code. Approves it on the laptop from the link the panel shows,
  // and returns the account the laptop showed.
  expect((await (await page.request.get(`${local}/api/setup?part=control`)).json()).control.url).toBe(base);
  const approve = async () => {
    await cloud();
    const connect = start.locator("[data-start-connect]");
    await expect(connect).toHaveText(`Connect to ${new URL(base).host}`);
    await connect.click();
    await expect(start.locator("[data-start-approve]")).toBeVisible({ timeout: 20_000 });
    const link = (await start.locator("[data-start-approve]").getAttribute("href"))!;
    expect(link.startsWith(`${base}/#join=`)).toBe(true);
    await laptop.goto(link);
    const account = (await laptop.locator("[data-join-account]").getAttribute("data-join-account"))!;
    await laptop.locator("[data-approve-join]").click();
    return account;
  };
  const shown = () => start.locator("[data-start-account]").getAttribute("data-start-account").then((a) => a?.replaceAll("-", ""));

  // Approved, but not saved until the person says the account is theirs.
  // "They don't": nothing is pinned.
  const account = await approve();
  await expect.poll(shown, { timeout: 20_000 }).toBe(account);
  await expect(start.locator("[data-start-confirm]")).toContainText("Is this your account?");
  await start.locator("[data-start-different]").click();
  await expect(start.locator("[data-start-error]")).toContainText("Not joined");
  expect((await (await page.request.get(`${local}/api/setup?part=control`)).json()).control.joined).toBeUndefined();

  // Again (as a new key: the turned-down one is dropped), and "They
  // match": joined.
  expect(await approve()).toBe(account);
  await expect.poll(shown, { timeout: 20_000 }).toBe(account);
  await start.locator("[data-start-same]").click();
  await expect(start.locator("[data-start-joined]")).toHaveText("Joined to your account", { timeout: 20_000 });
  await page.context().close();
  // The laptop sees it in the account.
  await laptop.goto("/");
  await expect.poll(() => hostNames(laptop), { timeout: 20_000 }).toContain("starter");
  // The page may switch machine as the new one arrives, mounting the
  // overlay afresh: ask for the panel until it's there.
  await expect(async () => {
    await controlPanel(laptop, "devices");
    await expect(laptop.locator("[data-account-fingerprint]")).toHaveAttribute("data-account-fingerprint", account, { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await laptop.getByRole("button", { name: "Done" }).click();
});

test("a browser that isn't a device yet, here to approve a machine, leads with its code; one approval covers both (#326)", async ({ browser }) => {
  const state = temp("rejoiner");
  const joining = spawn("../target/debug/arugulad", ["join", base, "--name", "rejoiner", "--state-dir", state], { stdio: ["pipe", "pipe", "ignore"] });
  procs.push(joining);
  const link = await new Promise<string>((res) => {
    let out = "";
    joining.stdout!.on("data", (d) => {
      out += d;
      const m = out.match(/(http\S+#join=[A-Z0-9.-]+)/);
      if (m) res(m[1]);
    });
  });
  // #760: the code, and the machine's secret after it.
  const whole = link.split("#join=")[1];
  const code = whole.split(".")[0];
  const exited = new Promise<number | null>((r) => joining.on("exit", r));

  // The machine's approval link, in a browser that isn't one of the
  // account's devices: signed in, it can't approve, so it says where to.
  const visitor = await (await browser.newContext()).newPage();
  await visitor.goto(link);
  await expect(visitor.locator('[data-why="join"]')).toBeVisible();
  await visitor.locator("[data-signin=github]").click();
  await expect(visitor.locator(`[data-waiting-join="${code}"]`)).toBeVisible({ timeout: 20_000 });
  await expect(visitor.getByRole("heading", { name: "Approve the machine on a device you use" })).toBeVisible();
  // The link it hands on keeps the secret, and (#761) carries the
  // visitor's own code; control was told the machine's code.
  const mine = (await visitor.locator("[data-waiting-also]").getAttribute("data-device-code"))!;
  await expect(visitor.locator("[data-control-join-link]")).toHaveText(`${base}/#join=${whole}&device=${mine}`);
  await expect(visitor.locator("svg[data-qr]")).toHaveAttribute("data-qr", `${base}/#join=${whole}&device=${mine}`);
  await expect(visitor.locator("[data-waiting-also]")).toContainText("the machine joins either way");
  const fp = (await visitor.locator("[data-fingerprint]").getAttribute("data-fingerprint"))!;

  // The laptop gets one prompt with both, side by side.
  await laptop.goto("/");
  await expect(laptop.locator("[data-join-both]")).toBeVisible({ timeout: 20_000 });
  await expect(laptop.locator("[data-join-machine] [data-join-sealed]")).toBeVisible();
  await expect(laptop.locator(`[data-join-alongside="${fp}"] [data-pending="${fp}"][data-device-sealed]`)).toBeVisible();
  // #761: brought by the visitor, the laptop doesn't have its code either:
  // it's ticked once that's typed.
  await expect(laptop.locator(`[data-join-also="${fp}"]`)).not.toBeChecked();
  await laptop.locator("[data-device-rest] [data-device-whole]").fill(mine);
  await expect(laptop.locator(`[data-join-also="${fp}"]`)).toBeChecked();
  await expect(laptop.locator("[data-approve-join]")).toHaveText("Approve both");
  // Brought by the visitor, the laptop has only the code: it shows that
  // half, asks for the rest, and Approve waits for it.
  await expect(laptop.locator("[data-join-rest]")).toContainText(`${code}.`);
  await expect(laptop.locator("[data-approve-join]")).toBeDisabled();
  const secret = whole.split(".")[1];
  await laptop.locator("[data-join-whole]").fill(secret.slice(0, 9));
  await expect(laptop.locator("[data-approve-join]")).toBeDisabled();
  // A whole code pasted in is taken, but only this machine's.
  await laptop.locator("[data-join-whole]").fill(`${code === "AAAAA-AAAAA" ? "BBBBB-BBBBB" : "AAAAA-AAAAA"}.${secret}`);
  await expect(laptop.locator("[data-join-rest-error]")).toBeVisible();
  await expect(laptop.locator("[data-approve-join]")).toBeDisabled();
  await laptop.locator("[data-join-whole]").fill(whole.toLowerCase());
  await expect(laptop.locator("[data-join-whole]")).toHaveValue(secret);
  await expect(laptop.locator("[data-approve-join]")).toBeEnabled();
  await expect(laptop.locator("[data-join-account]")).toHaveCount(0);
  await laptop.locator("[data-approve-join]").click();
  expect(await exited).toBe(0);
  // One approval: the browser is in too, and nothing else waits.
  await booted(visitor);
  // Its link's code is spent: it says so rather than a bare "no such code".
  await expect(visitor.locator(`[data-join-done="${code}"]`)).toBeVisible({ timeout: 20_000 });
  await expect(laptop.locator("[data-pending]")).toHaveCount(0);
  await expect(laptop.locator(".prompt")).toHaveCount(0);
  await visitor.context().close();
});

test("the rest of a code: typed key by key, another machine's stays an error, and nothing carries over to the next code (#760)", async () => {
  // Two machines waiting; the laptop is given only their codes.
  const ask = async (name: string) => {
    const joining = spawn("../target/debug/arugulad", ["join", base, "--name", name, "--state-dir", temp(name)], { stdio: ["pipe", "pipe", "ignore"] });
    procs.push(joining);
    const whole = await new Promise<string>((res) => {
      let out = "";
      joining.stdout!.on("data", (d) => {
        out += d;
        const m = out.match(/#join=([A-Z0-9.-]+)\s/);
        if (m) res(m[1]);
      });
    });
    return { whole, code: whole.split(".")[0], secret: whole.split(".")[1] };
  };
  const a = await ask("rest-a");
  const b = await ask("rest-b");
  const field = laptop.locator("[data-join-whole]");
  await laptop.goto(`/#join=${a.code}`);
  await expect(laptop.locator("[data-join-rest]")).toContainText(`${a.code}.`);
  // b's whole code, a key at a time: once the dot comes, it's b's, and it
  // stays an error (and in the field) as the rest is typed.
  await field.pressSequentially(b.whole);
  await expect(laptop.locator("[data-join-rest-error]")).toContainText(b.code);
  // Shown tidied: b's code in its own shape, one dot, then the rest.
  await expect(field).toHaveValue(b.whole);
  await expect(laptop.locator("[data-approve-join]")).toBeDisabled();
  // On to b's code: what was typed for a is gone, error and all.
  await laptop.evaluate((h) => (location.hash = h), `#join=${b.code}`);
  await expect(laptop.locator("[data-join-rest]")).toContainText(`${b.code}.`);
  await expect(field).toHaveValue("");
  await expect(laptop.locator("[data-join-rest-error]")).toHaveCount(0);
  // Its own whole code typed by hand: the matching half is dropped.
  await field.pressSequentially(b.whole.toLowerCase());
  await expect(field).toHaveValue(b.secret);
  await expect(laptop.locator("[data-approve-join]")).toBeEnabled();
  // Neither is wanted here.
  await laptop.locator("[data-cancel-join]").click();
  await laptop.goto(`/#join=${a.code}`);
  await laptop.locator("[data-cancel-join]").click();
  await expect(laptop.locator(".prompt")).toHaveCount(0);
});

test("a join link kept over a sign-in comes back only to its own code, and never over a link with a secret (#760)", async () => {
  const KEY = "arugula:presigned-invite";
  const code = "ABCDE-FGHJK";
  const kept = `#join=${code}.AAAA-AAAA-AAAA-AAAA`;
  const keep = (at: number) => laptop.evaluate(([k, h, t]) => sessionStorage.setItem(k as string, JSON.stringify({ hash: h, at: t })), [KEY, kept, at] as const);
  const stored = () => laptop.evaluate((k) => sessionStorage.getItem(k), KEY);
  // Coming back from GitHub loads the page; a goto from "/" that changes
  // only the hash wouldn't. (Each keep waits for the page to boot, or its
  // own restoreInvite reads it first.)
  const back = async (url: string) => {
    await laptop.goto("about:blank");
    await laptop.goto(url);
  };
  await laptop.goto("/");
  await booted(laptop);
  // Back from signing in: the code alone in the URL, so the kept link
  // (with its secret) is put back, and read once.
  await keep(Date.now());
  await back(`/#join=${code}`);
  await expect.poll(() => laptop.evaluate(() => location.hash)).toBe(kept);
  expect(await stored()).toBeNull();
  // A retried join (same code, new secret) opened in the same tab: its
  // link wins, and the kept one is gone.
  const fresh = `#join=${code}.BBBB-BBBB-BBBB-BBBB`;
  await laptop.goto("/");
  await booted(laptop);
  await keep(Date.now());
  await back(`/${fresh}`);
  await laptop.waitForFunction(() => window.__arugula?.control?.phase === "ready");
  expect(await laptop.evaluate(() => location.hash)).toBe(fresh);
  expect(await stored()).toBeNull();
  // Kept longer than a join lasts: not put back, even for its code.
  await laptop.goto("/");
  await booted(laptop);
  await keep(Date.now() - 60 * 60 * 1000);
  await back(`/#join=${code}`);
  await laptop.waitForFunction(() => window.__arugula?.control?.phase === "ready");
  expect(await laptop.evaluate(() => location.hash)).toBe(`#join=${code}`);
  expect(await stored()).toBeNull();
  await laptop.goto("/");
});

test("a waiting browser that follows another machine's link brings that one instead (#326)", async ({ browser }) => {
  // Two machines asking; the visitor comes for the first, then follows the
  // second's link in the same tab: only the hash changes, nothing reloads.
  const asks = async (name: string) => {
    const joining = spawn("../target/debug/arugulad", ["join", base, "--name", name, "--state-dir", temp(name)], { stdio: ["pipe", "pipe", "ignore"] });
    procs.push(joining);
    return new Promise<string>((res) => {
      let out = "";
      joining.stdout!.on("data", (d) => {
        out += d;
        const m = out.match(/(http\S+#join=[A-Z0-9.-]+)/);
        if (m) res(m[1]);
      });
    });
  };
  const [first, second] = [await asks("firstbox"), await asks("secondbox")];
  const visitor = await (await browser.newContext()).newPage();
  await visitor.goto(first);
  await visitor.locator("[data-signin=github]").click();
  const firstCode = first.split("#join=")[1].split(".")[0];
  await expect(visitor.locator(`[data-waiting-join="${firstCode}"]`)).toBeVisible({ timeout: 20_000 });
  await laptop.goto("/");
  await expect(laptop.locator("[data-join-machine]")).toContainText("firstbox", { timeout: 20_000 });

  await visitor.goto(second);
  const secondCode = second.split("#join=")[1].split(".")[0];
  await expect(visitor.locator(`[data-waiting-join="${secondCode}"]`)).toBeVisible();
  // Control is told: the laptop now sees the second machine alongside.
  await expect(laptop.locator("[data-join-machine]")).toContainText("secondbox", { timeout: 30_000 });
  // Neither is wanted: turn the machine and the browser down.
  await laptop.locator("[data-cancel-join]").click();
  await expect(laptop.locator("[data-reject]")).toBeVisible({ timeout: 20_000 });
  await laptop.locator("[data-reject]").click();
  await visitor.context().close();
});

test("sessions: where you're signed in, and signing out everywhere (#173)", async () => {
  await controlPanel(laptop, "account");
  await expect(laptop.getByRole("heading", { name: "Sign-in and account" })).toBeVisible();
  // This browser, the phone, and the others that signed in above.
  await expect(laptop.locator("[data-session]")).not.toHaveCount(0);
  expect(await laptop.locator("[data-session]").count()).toBeGreaterThanOrEqual(2);
  // The system Chrome's own platform: where the tests run.
  await expect(laptop.locator("[data-sessions]")).toContainText(`Chrome on ${process.platform === "darwin" ? "macOS" : "Linux"}`);
  await expect(laptop.locator("[data-sessions]")).toContainText("(this browser)");
  // Sign one other out: it's gone from the list.
  const n = await laptop.locator("[data-session]").count();
  const other = laptop.locator("[data-end-session]").first();
  await other.click();
  await other.click();
  await expect(laptop.locator("[data-session]")).toHaveCount(n - 1);
  // Everywhere: this one too.
  await laptop.locator("[data-end-all]").click();
  await laptop.locator("[data-end-all]").click();
  await expect(laptop.locator("[data-signin=github]")).toBeVisible();
  expect((await phone.request.get(`${base}/api/me`)).status()).toBe(401);
});

test("deleting the account: type its login; its machines and team go (#173)", async () => {
  await signIn(laptop);
  await booted(laptop);
  const before = await laptop.evaluate(() => window.__arugula.control!.account);
  await controlPanel(laptop, "account");
  await laptop.locator("[data-delete-account]").click();
  await expect(laptop.getByRole("heading", { name: "Delete your account" })).toBeVisible();
  await expect(laptop.locator("[data-disband]")).toContainText("Solo");
  await expect(laptop.locator("[data-delete-what]")).toContainText(/your \d+ machines/);
  const go = laptop.locator("[data-delete-go]");
  await expect(go).toBeDisabled();
  await laptop.locator("[data-delete-confirm]").fill("someone");
  await expect(go).toBeDisabled();
  await laptop.locator("[data-delete-confirm]").fill("stranger");
  await go.click();
  await expect(laptop.locator("[data-signin=github]")).toBeVisible();
  // Its machine, asked to join again, says the account is gone rather than
  // that it's still in it (#208).
  const rejoin = spawn("../target/debug/arugulad", ["join", base, "--name", "box", "--state-dir", states.box], { stdio: ["ignore", "ignore", "pipe"] });
  let said = "";
  rejoin.stderr!.on("data", (d) => (said += d));
  expect(await new Promise((r) => rejoin.on("exit", r))).not.toBe(0);
  expect(said).toContain("control doesn't know it any more (this machine's account was deleted); run `arugulad leave`");
  // The same GitHub account signing in again starts afresh: a new
  // account, a new first device, no machines.
  await signIn(laptop);
  await expect(laptop.locator("[data-recovery-code]")).toHaveCount(2);
  const after = await laptop.evaluate(() => window.__arugula.control!.account);
  expect(after).not.toBe(before);
  const dir = await laptop.request.get(`${base}/api/directory`);
  expect((await dir.json()).daemons).toEqual([]);
});
