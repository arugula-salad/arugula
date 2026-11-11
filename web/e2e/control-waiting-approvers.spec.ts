// Arugula #77: a waiting page names the devices that can approve it, with
// when each was last used. Alice's account has her laptop (a browser, its
// first device) and the desktop app (Arugula app on test-air, approved from
// the laptop as #758 does). A new browser of hers, on "Approve this
// browser", lists the laptop, and names the app apart: the app approves
// machines and apps, never a browser. A browser that came to approve a
// machine, on "Approve the machine on a device you use", lists both. With
// every device gone, the page says nothing can approve it and points to a
// recovery code. A local control (team-fixture.ts).

import { spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { createServer } from "node:http";
import { expect, test, type Browser, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("approvers");
const APP = "Arugula app on test-air";

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

/** A picture of the page for the PR, when E2E_SHOTS names a folder. */
const shot = async (p: Page, name: string) => {
  if (!process.env.E2E_SHOTS) return;
  // Tall enough for the whole screen: it scrolls inside, not the page.
  await p.setViewportSize({ width: 900, height: 1800 });
  await p.screenshot({ path: `${process.env.E2E_SHOTS}/${name}` });
};

let laptop: Page;
let laptopId = "";
let laptopName = "";
let appId = "";

/** The desktop app made a device from `page` (#758): what
 * crates/desktop/dist/signin.html sends, Allow on the laptop, and the
 * app's window taking its keys, so control has seen it used. */
async function desktopApp(browser: Browser, page: Page): Promise<string> {
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
    res.writeHead(303, { location: `${t.base}/#app-done` }).end();
  });
  await new Promise<void>((r) => loop.listen(0, "127.0.0.1", r));
  const port = (loop.address() as { port: number }).port;
  const ask = await fetch(`${t.base}/auth/app`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name: APP, challenge, port, features: ["no-code"], device: { noise: keys.noise, sign: keys.sign } }),
  });
  expect(ask.status).toBe(200);
  const tk = (await ask.json()) as { ticket: string; url: string };
  await page.goto(tk.url);
  await page.locator("[data-app-login-allow]").click();
  await expect(page.locator("[data-app-login-done]")).toContainText("one of your devices now");
  loop.close();
  const app = await (await browser.newContext({ baseURL: t.base })).newPage();
  await app.addInitScript((name) => Object.assign(window, { __arugulaApp: { name } }), APP);
  await app.goto(`/#app-redeem=${tk.ticket}.${grant}.${verifier}.${keys.secretNoise}.${keys.secretSign}.${cert}`);
  await app.waitForFunction(() => window.__arugula?.control?.phase === "ready", null, { timeout: 20_000 });
  const id = await app.evaluate(() => window.__arugula.control!.keys.id);
  await app.context().close();
  return id;
}

/** A new browser of Alice's, signed in and waiting to be approved. */
async function newBrowser(browser: Browser, url = "/"): Promise<Page> {
  const ctx = await browser.newContext({ baseURL: t.base });
  await ctx.addCookies([{ name: "as", value: "alice", url: t.github }]);
  const page = await ctx.newPage();
  await page.goto(url);
  await page.locator("[data-signin=github]").click();
  return page;
}

test("Approve this browser names the laptop that can, and the app that can't", async ({ browser }) => {
  test.setTimeout(120_000);
  await t.start();
  laptop = await t.person(browser, "alice");
  laptopId = await laptop.evaluate(() => window.__arugula.control!.keys.id);
  laptopName = await laptop.evaluate(() => window.__arugula.control!.enrollment!.cert.name);
  appId = await desktopApp(browser, laptop);
  await laptop.goto("/");

  const fresh = await newBrowser(browser);
  await expect(fresh.getByRole("heading", { name: "Approve this browser" })).toBeVisible({ timeout: 20_000 });
  const aside = fresh.locator('[data-approvers-for="browser"]');
  await expect(aside.locator("[data-approvers]")).toBeVisible({ timeout: 15_000 });
  await shot(fresh, "waiting-browser.png");
  // The laptop, by name, with when control last saw it used.
  const row = aside.locator(`[data-approver="${laptopId}"]`);
  await expect(row).toContainText(laptopName);
  await expect(row).toContainText("last used");
  await expect(row.locator("[data-used]")).toHaveCount(1);
  await expect(aside.locator("[data-approver]")).toHaveCount(1);
  await expect(aside).toContainText("Your device that can approve it:");
  // The app is named, and why it won't do.
  await expect(aside.locator(`[data-approver-cannot="${appId}"]`)).toHaveText(APP);
  await expect(aside.locator("[data-approvers-cannot]")).toContainText("can't approve a browser");
  await expect(aside.locator("[data-approvers-none]")).toHaveCount(0);

  // With every device gone (as control lists them), it says so plainly,
  // and points to the recovery code.
  await fresh.route("**/api/devices", async (route) => {
    if (route.request().method() !== "GET") return route.continue();
    const r = await route.fetch();
    const body = (await r.json()) as { certs: { device: string; account: string }[]; revocations: unknown[] };
    body.revocations = body.certs.map((c) => ({ v: 1, account: c.account, device: c.device, at: Date.now(), by: c.device, sig: "" }));
    await route.fulfill({ response: r, json: body });
  });
  await fresh.reload();
  const none = fresh.locator('[data-approvers-for="browser"] [data-approvers-none]');
  await expect(none).toContainText("Your account has no other device that can approve it", { timeout: 20_000 });
  await expect(none).toContainText("Use a recovery code below");
  await shot(fresh, "waiting-none.png");
  await fresh.context().close();
});

test("Approve the machine names both the laptop and the app", async ({ browser }) => {
  test.setTimeout(120_000);
  const joining = spawn("../target/debug/arugulad", ["join", t.base, "--name", "rejoiner", "--state-dir", t.temp("rejoiner")], { stdio: ["pipe", "pipe", "ignore"] });
  try {
    const link = await new Promise<string>((res) => {
      let out = "";
      joining.stdout!.on("data", (d) => {
        out += d;
        const m = out.match(/(http\S+#join=[A-Z0-9.-]+)/);
        if (m) res(m[1]);
      });
    });
    const code = link.split("#join=")[1].split(".")[0];
    const visitor = await newBrowser(browser, link);
    await expect(visitor.locator(`[data-waiting-join="${code}"]`)).toBeVisible({ timeout: 20_000 });
    const aside = visitor.locator('[data-approvers-for="daemon"]');
    await expect(aside.locator("[data-approvers]")).toBeVisible({ timeout: 15_000 });
    await shot(visitor, "waiting-machine.png");
    await expect(aside).toContainText("Your devices that can approve it, last used first:");
    await expect(aside.locator(`[data-approver="${appId}"]`)).toContainText(APP);
    await expect(aside.locator(`[data-approver="${appId}"]`)).toContainText("last used");
    await expect(aside.locator(`[data-approver="${laptopId}"]`)).toContainText(laptopName);
    await expect(aside.locator("[data-approver]")).toHaveCount(2);
    await expect(aside.locator("[data-approvers-cannot]")).toHaveCount(0);
    // Not the browser's own reason for asking again.
    await expect(visitor.locator("[data-why-again]")).toHaveCount(0);
    await visitor.context().close();
  } finally {
    joining.kill("SIGKILL");
  }
});
