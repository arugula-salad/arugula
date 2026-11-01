// DK5 (#386): a deploy is approved where the push opens it, on the platform
// page, in one screen. The card checks the operation's bytes, asks the
// platform whether the reader may decide it (`can-decide`), and asks for
// control's passkey in place (Chrome's virtual authenticator stands in for
// Touch ID). Where this page can't use control's passkeys, it offers
// control's `#deploy=` dialog and says why.
//
// The rig: control on `arugula.localhost`, the platform page on
// `app.arugula.localhost`. There the passkey's RP (control's host) is a
// parent of the page's host, so Chrome takes it without related origins
// (`localhost` itself isn't a registrable domain, so control on plain
// `localhost` gets the SecurityError path). In production control's host
// isn't a parent (control.arugula.io and app.arugula.io): Chrome reads
// control's `/.well-known/webauthn` first, over https. That fetch is the one
// part this spec doesn't run; S46's harness ran it against Chrome, and the
// platform's check of the assertion (its origin is this page's, its RP
// control's host) is the same either way.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

// The fake driver is the platform's only when it's asked for (testnet's switch).
process.env.ARUGULA_PLATFORM_FAKE_DRIVER = "1";

const t = new TeamControl("deploy-approve", { host: "arugula.localhost" });

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

/** A request to the platform's API, from its page (the person's session). */
const api = (p: Page, method: string, path: string, body?: unknown) =>
  p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, headers: { "content-type": "application/json" }, body: b ? JSON.stringify(b) : undefined });
      return { status: r.status, body: await r.json().catch(() => null) };
    },
    [method, path, body] as const,
  );

/** A deploy of `sha` to the environment, planned and asked for: its id. */
async function asked(p: Page, project: string, env: string, sha: string): Promise<string> {
  const d = await api(p, "POST", `/api/projects/${project}/deploys`, { environment: env, source: { kind: "commit", ref: sha.repeat(40 / sha.length), where: "acme/app" } });
  expect(d.status, JSON.stringify(d.body)).toBe(200);
  const r = await api(p, "POST", `/api/deploys/${d.body.id}/request`);
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return d.body.id;
}

test("the push's page approves a deploy in place with a passkey, and says why when it can't", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.arugula.localhost", "--hosted-accounts", "*"]);
  const app = t.base.replace("//arugula.localhost", "//app.arugula.localhost");
  const alice = await t.person(browser, "alice", { serviceWorkers: "block" });
  const cdp = await alice.context().newCDPSession(alice);
  await cdp.send("WebAuthn.enable");
  await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true, hasUserVerification: true, isUserVerified: true },
  });
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));

  await alice.goto(app);
  await expect(alice.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];
  const env = await api(alice, "POST", `/api/projects/${project}/environments`, { name: "production", driver: "fake" });
  expect(env.status, JSON.stringify(env.body)).toBe(200);

  // No passkey yet: the card says so instead of offering a prompt that fails.
  const first = await asked(alice, project, env.body.id, "a");
  await alice.goto(`${app}/deploys/${first}`);
  await expect(alice.locator('[data-deploy-cannot="no_passkey"]')).toContainText("your account has none");
  await expect(alice.locator("[data-deploy-approve]")).toHaveCount(0);

  // A passkey, made on control's page (its RP is control's host).
  await alice.goto(t.base);
  await alice.waitForFunction(() => window.__arugula?.control?.phase === "ready");
  await alice.evaluate(() => window.__arugula.control!.addPasskey());
  await expect.poll(() => alice.evaluate(() => fetch("/api/me/passkeys").then((r) => r.json()).then((v) => v.passkeys.length))).toBe(1);

  // The push opens the deploy here; the card shows what's signed, and
  // Approve with passkey approves it on this page.
  await alice.goto(`${app}/deploys/${first}`);
  const card = alice.locator(`[data-deploy-card="${first}"]`);
  await expect(card.locator('[data-deploy-decide="ready"]')).toBeVisible();
  const op = await api(alice, "GET", `/api/deploys/${first}`);
  await expect(card.locator("[data-deploy-op-signed]")).toHaveText(op.body.operation.body);
  await expect(card.locator("[data-deploy-op-hash]")).toHaveText(op.body.operation.hash);
  // The checked plan shows open beside the buttons, in the inbox's card too.
  await expect(card.locator("[data-deploy-plan-signed]")).toHaveText(op.body.plan_text);
  await alice.goto(`${app}/inbox`);
  const inboxCard = alice.locator(`[data-inbox-group=deploys] [data-deploy-card="${first}"]`);
  await expect(inboxCard.locator('[data-deploy-decide="ready"] [data-deploy-plan-signed]')).toHaveText(op.body.plan_text);

  // Another deploy on the same page (the router keeps the page): nothing
  // checked for the first one stands while the other loads and is checked.
  const staging = await api(alice, "POST", `/api/projects/${project}/environments`, { name: "staging", driver: "fake" });
  const other = await asked(alice, project, staging.body.id, "d");
  const otherOp = (await api(alice, "GET", `/api/deploys/${other}`)).body.operation.hash;
  await alice.goto(`${app}/deploys/${first}`);
  await expect(card.locator('[data-deploy-decide="ready"]')).toBeVisible();
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  await alice.route(`**/api/deploys/${other}/can-decide`, async (r) => {
    await held;
    await r.continue();
  });
  await alice.evaluate((p) => {
    history.pushState(null, "", p);
    dispatchEvent(new PopStateEvent("popstate"));
  }, `/deploys/${other}`);
  const otherCard = alice.locator(`[data-deploy-card="${other}"]`);
  await expect(otherCard.locator('[data-deploy-decide="checking"]')).toBeVisible();
  await expect(alice.locator("[data-deploy-approve], [data-deploy-deny]")).toHaveCount(0);
  await expect(card).toHaveCount(0);
  release();
  await expect(otherCard.locator("[data-deploy-op-hash]")).toHaveText(otherOp);
  await alice.unroute(`**/api/deploys/${other}/can-decide`);
  await alice.goto(`${app}/deploys/${first}`);
  await expect(card.locator('[data-deploy-decide="ready"]')).toBeVisible();

  await card.locator("[data-deploy-approve]").click();
  await expect(card.locator("[data-deploy-done]")).toContainText("Approved");
  await expect(alice.locator("[data-deploy-state-chip]").first()).toHaveAttribute("data-deploy-state-chip", "approved");
  const after = await api(alice, "GET", `/api/deploys/${first}`);
  expect(after.body.state).toBe("approved");
  expect(after.body.approvals[0]).toMatchObject({ v: 2, decision: "once" });
  // Its assertion names this page's origin, with control's host as the RP.
  const client = JSON.parse(Buffer.from(after.body.approvals[0].passkey.client_data_json, "base64url").toString());
  expect(client).toMatchObject({ type: "webauthn.get", origin: new URL(app).origin });

  // Decided: the page says so, with no button.
  await alice.reload();
  await expect(alice.locator("[data-deploy-decide]")).toHaveCount(0);

  // Deny works the same way.
  const second = await asked(alice, project, env.body.id, "b");
  await alice.goto(`${app}/deploys/${second}`);
  await alice.locator(`[data-deploy-card="${second}"] [data-deploy-deny]`).click();
  await expect(alice.locator(`[data-deploy-card="${second}"] [data-deploy-done]`)).toContainText("Denied");
  expect((await api(alice, "GET", `/api/deploys/${second}`)).body.state).toBe("denied");

  // A window with no WebAuthn (the Linux app's): Approve on Control, saying why.
  const third = await asked(alice, project, env.body.id, "c");
  const bare = await alice.context().newPage();
  await bare.addInitScript(() => {
    delete (window as { PublicKeyCredential?: unknown }).PublicKeyCredential;
  });
  await bare.goto(`${app}/deploys/${third}`);
  const round = bare.locator("[data-deploy-on-control]");
  await expect(round).toHaveText("Approve on Control");
  await expect(round).toHaveAttribute("href", `${t.base}/#deploy=${third}`);
  await expect(bare.locator("[data-deploy-why-elsewhere]")).toContainText("no passkeys");
  await bare.close();

  // A browser whose passkey manager refuses this origin at once (no related
  // origins): the same, after it tried.
  await alice.addInitScript(() => {
    navigator.credentials.get = () => Promise.reject(new DOMException("not a related origin", "SecurityError"));
  });
  await alice.goto(`${app}/deploys/${third}`);
  await alice.locator(`[data-deploy-card="${third}"] [data-deploy-approve]`).click();
  await expect(alice.locator('[data-deploy-decide="elsewhere"] [data-deploy-on-control]')).toBeVisible();
  await expect(alice.locator("[data-deploy-why-elsewhere]")).toContainText("can't use Control's passkeys");
  expect((await api(alice, "GET", `/api/deploys/${third}`)).body.state).toBe("pending");
});
