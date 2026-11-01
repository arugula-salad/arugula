// #468: a deploy waiting for your approval is findable without its push.
// The platform page's Inbox counts it in the bar and lists it first, and
// control's phone view shows it as a row under the bar that opens the
// deploy on the platform page. And control's notification settings send
// a test notification to every subscription the account has, and say how
// many there are.
//
// The rig: control on `localhost` (a machine joins it there), the
// platform page on `app.localhost`, the platform's fake driver.

import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, show } from "./team-fixture";

process.env.ARUGULA_PLATFORM_FAKE_DRIVER = "1";

const t = new TeamControl("deploys-waiting", { localhost: true });

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const api = (p: Page, method: string, path: string, body?: unknown) =>
  p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, headers: { "content-type": "application/json" }, body: b ? JSON.stringify(b) : undefined });
      return { status: r.status, body: await r.json().catch(() => null) };
    },
    [method, path, body] as const,
  );

test("a waiting deploy is counted on the Inbox and shown on control's phone view; a test notification reaches every subscription", async ({ browser }) => {
  test.setTimeout(180_000);
  // A stand-in push service: what control posts lands here.
  const got: string[] = [];
  const service = createServer((req, res) => {
    req.on("data", () => {});
    req.on("end", () => {
      got.push(req.url ?? "");
      res.writeHead(201).end();
    });
  });
  await new Promise<void>((r) => service.listen(0, "127.0.0.1", r));
  const pushHost = `127.0.0.1:${(service.address() as AddressInfo).port}`;
  await t.start(["--platform-host", "app.localhost", "--hosted-accounts", "*", "--push-host", pushHost]);
  const app = t.base.replace("//localhost", "//app.localhost");
  const alice = await t.person(browser, "alice", { serviceWorkers: "block" });
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));

  await alice.goto(app);
  await expect(alice.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];
  const env = await api(alice, "POST", `/api/projects/${project}/environments`, { name: "production", driver: "fake" });
  expect(env.status, JSON.stringify(env.body)).toBe(200);
  // Nothing waits: no count for deploys.
  await expect(alice.locator("[data-inbox-deploys]")).toHaveCount(0);

  const d = await api(alice, "POST", `/api/projects/${project}/deploys`, { environment: env.body.id, source: { kind: "commit", ref: "a".repeat(40), where: "acme/app" } });
  expect(d.status, JSON.stringify(d.body)).toBe(200);
  const r = await api(alice, "POST", `/api/deploys/${d.body.id}/request`);
  expect(r.status, JSON.stringify(r.body)).toBe(200);

  // The platform page: the bar's Inbox counts it, and the inbox lists it first.
  await alice.goto(`${app}/inbox`);
  const count = alice.locator("[data-inbox-count][data-inbox-deploys='1']");
  await expect(count).toBeVisible();
  await expect(count).toHaveAttribute("title", /1 deploy waits for your approval/);
  await expect(alice.locator("[data-inbox-group]").first()).toHaveAttribute("data-inbox-group", "deploys");

  // Control's phone view: a row at the top, which opens the deploy there.
  // With no machine yet, on the screen that says how to add one.
  await alice.setViewportSize({ width: 390, height: 760 });
  await home(alice);
  const note = alice.locator("[data-deploys-waiting='1']");
  const said = "Acme: Deploy to production waits for your approval";
  await expect(note).toContainText(said);
  // With one, under the phone's bar.
  await t.addMachine(alice, "box");
  await home(alice);
  await show(alice, "box");
  await expect(alice.locator(".phone-bar + [data-deploys-waiting='1']")).toContainText(said);
  const [opened] = await Promise.all([alice.context().waitForEvent("page"), note.click()]);
  await expect(opened).toHaveURL(`${app}/deploys/${d.body.id}`);
  await opened.close();

  // A test notification: to every subscription the account has, counted.
  // This device's, at the stand-in service, signed by its key.
  const endpoint = `http://${pushHost}/push/alice`;
  await alice.evaluate(async (endpoint) => {
    const b64 = (b: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(b))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    const pair = (await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"])) as CryptoKeyPair;
    const p256dh = b64(await crypto.subtle.exportKey("raw", pair.publicKey));
    await window.__arugula.control!.subscribePush({ endpoint, p256dh, auth: b64(crypto.getRandomValues(new Uint8Array(16)).buffer) });
  }, endpoint);
  const check = await api(alice, "POST", "/api/push/check", { endpoint });
  expect(check.body).toEqual({ subscriptions: 1, usable: 1, this_device: "usable" });
  const tried = await api(alice, "POST", "/api/push/test", { endpoint: null });
  expect(tried.body).toMatchObject({ subscriptions: 1, usable: 1, sent: 1, gone: 0, failed: 0, this_device: null });
  await expect.poll(() => got).toContain("/push/alice");
  service.close();
});
