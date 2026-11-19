// aru-141 (#997): a browser that isn't one of the account's devices works
// on its machine with machine tokens. Signed in to control (a stand-in,
// routed on the page), with an account that already has a device, it
// doesn't ask to be approved: it lists the machine from the directory's
// Noise key, asks control for a token, and opens a channel to the real
// daemon with it. The daemon is joined to a stand-in control that publishes
// the issuer's key; the first token it's handed has run out, so the page
// asks for a fresh one and tries again. The daemon's own checks are
// crates/arugulad/tests/integration/machine_tokens.rs.

import { spawn, type ChildProcess } from "node:child_process";
import { createPrivateKey, generateKeyPairSync, randomUUID, sign } from "node:crypto";
import { createServer, type Server } from "node:http";
import { mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { ANY, daemonPort, listen } from "./ports";
import { labs } from "./labs";
import { controlPanel, ready, run } from "./helpers";
import { keyHeaders } from "./local-token";
import { certBody, deviceId, hex, joinCode, oneClickMac, unhex, type Cert } from "../src/shell/e2e/cert.ts";
import { generateKeys, signText } from "../src/shell/e2e/keys.ts";

// Control's routes are on the page, and a service worker would take them.
test.use({ serviceWorkers: "block" });

const b64 = (b: Uint8Array | string) => Buffer.from(b).toString("base64url");
const raw = (k: { export(o: { format: "jwk" }): { d?: string; x?: string } }, part: "d" | "x") => hex(new Uint8Array(Buffer.from(k.export({ format: "jwk" })[part]!, "base64url")));

let state = "";
let daemon: ChildProcess | undefined;
let control: Server | undefined;
let controlUrl = "";
let origin = "";
let machine = { id: "", noise: "" };
const issuer = generateKeyPairSync("ed25519");
let rootDevice = "";
/** The issuer's public keys, as control publishes them. */
const jwks = () => ({ keys: [{ kty: "OKP", crv: "Ed25519", kid: "k1", alg: "EdDSA", use: "sig", x: issuer.publicKey.export({ format: "jwk" }).x }] });

test.beforeAll(async () => {
  state = labs(mkdtempSync(join(tmpdir(), "arugula-e2e-tokens-")));
  // The machine's keys, made here so its id and certificate can be.
  const nk = generateKeyPairSync("x25519");
  const sk = generateKeyPairSync("ed25519");
  const noise = raw(nk.publicKey, "x");
  const signPub = raw(sk.publicKey, "x");
  writeFileSync(join(state, "daemon.key"), `illogical-device-key 1\nnoise ${raw(nk.privateKey, "d")} ${noise}\nsign ${raw(sk.privateKey, "d")}\n`, { mode: 0o600 });
  machine = { id: await deviceId(unhex(noise), unhex(signPub)), noise };

  // The account's first device, which approved the machine (and never this
  // browser).
  const root = await generateKeys(true);
  rootDevice = root.id;
  const signed = async (c: Omit<Cert, "sig">) => ({ ...c, sig: await signText(root, certBody({ ...c, sig: "" })) }) as Cert;
  const rootCert = await signed({ v: 1, account: "a1", device: root.id, kind: "browser", name: "laptop", noise: root.noisePub, sign: root.signPub, created: 1, approver: root.id });
  const cert = await signed({ v: 1, account: "a1", device: machine.id, kind: "daemon", name: "box", noise, sign: signPub, created: 2, approver: root.id });
  const certs = [rootCert, cert];

  // Control as the daemon sees it: its issuer's key, and the account's trust.
  const keys = jwks();
  control = createServer((req, res) => {
    const path = (req.url ?? "").split("?")[0];
    const body =
      path === "/control.json"
        ? { control: true, daemon_auth: 2 }
        : path === "/.well-known/jwks.json"
          ? keys
          : path === "/api/daemon/trust"
            ? { certs, revocations: [] }
            : path === "/api/daemon/push-subs"
              ? { subs: [] }
              : path === "/api/daemon/peers" || path === "/api/daemon/access"
                ? {}
                : null;
    res.writeHead(body ? 200 : 404, { "Content-Type": "application/json" });
    res.end(JSON.stringify(body ?? { error: "not here" }));
  });
  controlUrl = `http://127.0.0.1:${await listen(control)}`;
  writeFileSync(
    join(state, "control.json"),
    JSON.stringify({ url: controlUrl, trust: { account: "a1", root: root.id }, cert, certs, login: "me" }),
  );

  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--name", "box", "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/tailscaled.sock"],
    ],
    { stdio: ["ignore", openSync(join(state, "log"), "a"), openSync(join(state, "log"), "a")] },
  );
  origin = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
  // Joined (its log, if it can't say).
  await expect
    .poll(async () => {
      try {
        const h = (await (await fetch(`${origin}/api/host`, { headers: keyHeaders })).json()) as { control_state?: { state?: string } };
        return h.control_state?.state ?? JSON.stringify(h).slice(0, 300);
      } catch {
        return `log: ${readFileSync(join(state, "log"), "utf8").slice(-1500)}`;
      }
    }, { timeout: 20_000 })
    .toBe("joined");
});

// A killed daemon can still be writing its state until it has exited: wait
// for it before removing the directory (ENOTEMPTY otherwise).
test.afterAll(async () => {
  if (daemon && daemon.exitCode === null && daemon.signalCode === null) {
    const exited = new Promise((r) => daemon!.once("exit", r));
    daemon.kill("SIGKILL");
    await exited;
  }
  control?.close();
  if (state) rmSync(state, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
});

/** A machine token as the hub signs one; `ago` seconds old. */
function token(ago = 0): string {
  const now = Math.floor(Date.now() / 1000) - ago;
  const claims = { iss: controlUrl, sub: "a1", aud: machine.id, use: "machine", role: "owner", name: "me", iat: now, exp: now + 300, jti: randomUUID() };
  const head = `${b64(JSON.stringify({ alg: "EdDSA", typ: "JWT", kid: "k1" }))}.${b64(JSON.stringify(claims))}`;
  return `${head}.${b64(sign(null, Buffer.from(head), createPrivateKey(issuer.privateKey.export({ format: "pem", type: "pkcs8" }))))}`;
}

test("a browser with no device key opens a pane with machine tokens", async ({ page }) => {
  const asked: string[] = [];
  const enrolled: string[] = [];
  // Control on the page's origin: signed in, the account has a device
  // already (so this browser would wait to be approved), and the
  // directory lists the machine with its Noise key (#994).
  await page.route(/\/control\.json$/, (r) => r.fulfill({ json: { control: true, url: origin, github: true, passkeys: false, vapid: "" } }));
  await page.route(/\/\.well-known\/jwks\.json$/, (r) => r.fulfill({ json: jwks() }));
  await page.route(/\/api\//, async (r) => {
    const url = new URL(r.request().url());
    const method = r.request().method();
    if (url.pathname === "/api/me") return r.fulfill({ json: { account: "a1", login: "me", name: "Me", root: rootDevice, passkeys: 0 } });
    if (url.pathname === "/api/directory") return r.fulfill({ json: { daemons: [{ id: machine.id, name: "box", urls: [origin], online: true, last_seen: null, noise: machine.noise }] } });
    if (url.pathname === `/api/machines/${machine.id}/token` && method === "POST") {
      asked.push(method);
      // The first has run out: the machine refuses it, and the page asks again.
      const t = asked.length === 1 ? token(400) : token();
      return r.fulfill({ json: { token: t, expires: Math.floor(Date.now() / 1000) + 300, role: "owner" } });
    }
    if (url.pathname.startsWith("/api/devices") && method === "POST") enrolled.push(url.pathname);
    return r.fulfill({ status: 404, json: { error: "not here" } });
  });

  await page.goto(`${origin}/`);
  await expect.poll(() => page.evaluate(() => window.__arugula?.control?.phase)).toBe("ready");
  expect(await page.evaluate(() => [window.__arugula.control?.tokens, !!window.__arugula.control?.enrollment])).toEqual([true, false]);
  await expect.poll(() => page.evaluate((id) => window.__arugula.client.e2e?.daemon.id === id && window.__arugula.client.connected, machine.id), { timeout: 20_000 }).toBe(true);
  // The machine refused the token that ran out, and let in the next.
  expect(asked.length).toBeGreaterThanOrEqual(2);
  expect(readFileSync(join(state, "log"), "utf8")).toMatch(/refused a channel's machine token.*expired/);
  expect(enrolled).toEqual([]);
  expect(await page.evaluate(() => window.__arugula.client.path)).toBe("direct");

  // A terminal, typed into: the token's owner role may.
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state?.panes[0]?.id ?? null)).not.toBeNull();
  const pane = await page.evaluate(() => window.__arugula.client.active()!);
  await ready(page, pane);
  await run(page, pane, "echo token-$((6*7))", "token-42");
});

// #1004: no screen asks for an approval any more. Where control signs no
// tokens (a dev or test hub), only the account's first browser becomes a
// device, signing itself; any other browser of the account just signs in.
for (const root of [null, "other"] as const) {
  test(`with a control that signs no tokens, a new browser ${root ? "of an account with a device isn't asked to be approved" : "is the account's first device, with no screen"}`, async ({ page }) => {
    const enrolled: string[] = [];
    await page.route(/\/control\.json$/, (r) => r.fulfill({ json: { control: true, url: origin, github: true, passkeys: false, vapid: "" } }));
    await page.route(/\/\.well-known\/jwks\.json$/, (r) => r.fulfill({ status: 404, json: { error: "no issuer" } }));
    await page.route(/\/api\//, async (r) => {
      const url = new URL(r.request().url());
      const method = r.request().method();
      if (url.pathname === "/api/me") return r.fulfill({ json: { account: "a1", login: "me", name: "Me", root: root && rootDevice, passkeys: 0 } });
      if (url.pathname === "/api/devices" && method === "POST") enrolled.push((r.request().postDataJSON() as { cert: Cert }).cert.approver);
      return r.fulfill({ status: 404, json: { error: "not here" } });
    });

    await page.goto(`${origin}/`);
    if (root) {
      await expect.poll(() => page.evaluate(() => window.__arugula?.control?.phase)).toBe("ready");
      expect(enrolled).toEqual([]);
      expect(await page.evaluate(() => [window.__arugula.control!.tokens, !!window.__arugula.control!.enrollment])).toEqual([true, false]);
    } else {
      // It signs its own certificate (here control refuses, so it goes no
      // further), and nothing waits to approve it.
      await expect.poll(() => enrolled.length).toBeGreaterThan(0);
      expect(enrolled[0]).toBe(await page.evaluate(() => window.__arugula.control!.keys.id));
    }
    await expect(page.locator("[data-waiting-line], [data-device-code], [data-recovery-code]")).toHaveCount(0);
  });
}

// #1003: Getting started's link (`#join=CODE.SECRET`) on a browser with no
// device key: it isn't asked to be approved, and adds the machine with one
// click, as the signed-in person: no certificate and no fingerprint, but a
// MAC under the link's secret over the account and the machine's keys,
// which the machine checks. With the code alone, it asks for the rest of
// the code before it offers Add.
for (const whole of [true, false]) {
  test(`a browser with no device key adds a machine's join in one click${whole ? "" : ", from the code and the rest typed"}`, async ({ page }) => {
    const enrolled: string[] = [];
    const added: unknown[] = [];
    // Another machine, waiting with its code.
    const k = await generateKeys(true);
    const joining: Cert = { v: 1, account: "", device: k.id, kind: "daemon", name: "new-box", noise: k.noisePub, sign: k.signPub, created: Date.now(), approver: "", sig: "" };
    const code = await joinCode(joining);
    await page.route(/\/control\.json$/, (r) => r.fulfill({ json: { control: true, url: origin, github: true, passkeys: false, vapid: "" } }));
    await page.route(/\/\.well-known\/jwks\.json$/, (r) => r.fulfill({ json: jwks() }));
    await page.route(/\/api\//, async (r) => {
      const url = new URL(r.request().url());
      const method = r.request().method();
      if (url.pathname === "/api/me") return r.fulfill({ json: { account: "a1", login: "me", name: "Me", root: rootDevice, passkeys: 0 } });
      if (url.pathname === "/api/directory") return r.fulfill({ json: { daemons: [{ id: machine.id, name: "box", urls: [origin], online: true, last_seen: null, noise: machine.noise }] } });
      if (url.pathname === "/api/teams") return r.fulfill({ json: { teams: [] } });
      if (url.pathname === `/api/joins/${code}` && method === "GET")
        return r.fulfill({ json: { code, cert: joining, urls: [], created: Date.now(), team: null, features: "presigned-invites,owner-moves,join-secret,owner-passkeys" } });
      if (url.pathname === `/api/joins/${code}/approve` && method === "POST") {
        added.push(r.request().postDataJSON());
        return r.fulfill({ json: { approved: true, daemon: joining.device } });
      }
      if (url.pathname.startsWith("/api/devices") && method === "POST") enrolled.push(url.pathname);
      return r.fulfill({ status: 404, json: { error: "not here" } });
    });

    await page.goto(whole ? `${origin}/#join=${code}.ABCD-EFGH-JKMN-PQRS` : `${origin}/#join=${code}`);
    await expect.poll(() => page.evaluate(() => window.__arugula?.control?.phase)).toBe("ready");
    expect(await page.evaluate(() => [window.__arugula.control?.tokens, !!window.__arugula.control?.enrollment])).toEqual([true, false]);
    const dialog = page.getByRole("dialog", { name: "Add a machine?" });
    await expect(dialog).toContainText("new-box asks to join your account.");
    await expect(dialog).not.toContainText(/fingerprint|Its key/);
    const add = dialog.locator("[data-approve-join]").filter({ hasText: /^Add$/ });
    if (whole) await expect(dialog.locator("[data-join-rest]")).toHaveCount(0);
    else {
      await expect(add).toBeDisabled();
      await dialog.locator("[data-join-whole]").fill("ABCD-EFGH-JKMN-PQRS");
    }
    await add.click();
    await expect(dialog).toBeHidden();
    expect(added).toEqual([{ team: null, mac: await oneClickMac("ABCD-EFGH-JKMN-PQRS", "a1", joining) }]);
    expect(enrolled).toEqual([]);
  });
}

// #997: no browser becomes a device any more where control signs tokens:
// not the account's first (which used to sign itself as its root), and not
// the desktop app's window. Each lists its machines from the directory and
// asks control for tokens, with no `/api/devices` POST.
for (const app of [false, true]) {
  test(`the account's first ${app ? "app window" : "browser"} takes tokens and never enrolls`, async ({ page }) => {
    const enrolled: string[] = [];
    if (app) await page.addInitScript(() => Object.assign(window, { __arugulaApp: { name: "Arugula app on test-mac" } }));
    await page.route(/\/control\.json$/, (r) => r.fulfill({ json: { control: true, url: origin, github: true, passkeys: false, vapid: "" } }));
    await page.route(/\/\.well-known\/jwks\.json$/, (r) => r.fulfill({ json: jwks() }));
    await page.route(/\/api\//, async (r) => {
      const url = new URL(r.request().url());
      const method = r.request().method();
      // No device yet: no root.
      if (url.pathname === "/api/me") return r.fulfill({ json: { account: "a1", login: "me", name: "Me", root: null, passkeys: 0 } });
      if (url.pathname === "/api/directory") return r.fulfill({ json: { daemons: [{ id: machine.id, name: "box", urls: [origin], online: true, last_seen: null, noise: machine.noise }] } });
      if (url.pathname === `/api/machines/${machine.id}/token` && method === "POST") return r.fulfill({ json: { token: token(), expires: Math.floor(Date.now() / 1000) + 300, role: "owner" } });
      if (url.pathname.startsWith("/api/devices") && method === "POST") enrolled.push(url.pathname);
      return r.fulfill({ status: 404, json: { error: "not here" } });
    });

    await page.goto(`${origin}/`);
    await expect.poll(() => page.evaluate(() => window.__arugula?.control?.phase)).toBe("ready");
    expect(await page.evaluate(() => [window.__arugula.control?.tokens, !!window.__arugula.control?.enrollment, window.__arugula.control?.recoveryCodes ?? null])).toEqual([true, false, null]);
    await expect.poll(() => page.evaluate((id) => window.__arugula.client.e2e?.daemon.id === id && window.__arugula.client.connected, machine.id), { timeout: 20_000 }).toBe(true);
    expect(enrolled).toEqual([]);
  });
}

// #997, #1004: a CLI's join still needs a device's signature, which a
// browser with no device key can't give: it says how to sign the CLI in
// from a browser instead, and turning it down ends the CLI's wait.
test("a browser with no device key can't approve a CLI, and points it at arugula login --browser", async ({ page }) => {
  const k = await generateKeys(true);
  const cli: Cert = { v: 1, account: "", device: k.id, kind: "cli", name: "Arugula CLI on laptop", noise: k.noisePub, sign: k.signPub, created: Date.now(), approver: "", sig: "" };
  const code = await joinCode(cli);
  await page.route(/\/control\.json$/, (r) => r.fulfill({ json: { control: true, url: origin, github: true, passkeys: false, vapid: "" } }));
  await page.route(/\/\.well-known\/jwks\.json$/, (r) => r.fulfill({ json: jwks() }));
  await page.route(/\/api\//, async (r) => {
    const url = new URL(r.request().url());
    if (url.pathname === "/api/me") return r.fulfill({ json: { account: "a1", login: "me", name: "Me", root: rootDevice, passkeys: 0 } });
    if (url.pathname === "/api/directory") return r.fulfill({ json: { daemons: [] } });
    if (url.pathname === `/api/joins/${code}`) return r.fulfill({ json: { code, cert: cli, urls: [], created: Date.now(), team: null, features: "join-secret" } });
    return r.fulfill({ status: 404, json: { error: "not here" } });
  });
  await page.goto(`${origin}/#join=${code}.ABCD-EFGH-JKMN-PQRS`);
  const dialog = page.getByRole("dialog", { name: "Sign in the command line?" });
  await expect(dialog.locator("[data-join-cli-browser]")).toContainText("laptop");
  await expect(dialog.locator("[data-join-cli-command]")).toContainText(`arugula login --browser ${origin}`);
  await expect(dialog.locator("[data-approve-join]")).toHaveCount(0);
  await expect(dialog.getByRole("button", { name: "Turn it down" })).toBeVisible();
});

// Arugula #960: a panel opened right after signing in, while the page is
// still switching to the machine, stays open: the overlay mounts again on
// the switch (`ControlRoot` is keyed by the client's base), and the open
// panel is held above it. The directory waits until the panel is open, so
// the switch comes after it.
test("a panel opened right after signing in is still open once the machine is connected", async ({ page }) => {
  let release = () => {};
  const listed = new Promise<void>((r) => (release = r));
  await page.route(/\/control\.json$/, (r) => r.fulfill({ json: { control: true, url: origin, github: true, passkeys: false, vapid: "" } }));
  await page.route(/\/\.well-known\/jwks\.json$/, (r) => r.fulfill({ json: jwks() }));
  await page.route(/\/api\//, async (r) => {
    const url = new URL(r.request().url());
    const method = r.request().method();
    if (url.pathname === "/api/me") return r.fulfill({ json: { account: "a1", login: "me", name: "Me", root: rootDevice, passkeys: 0 } });
    if (url.pathname === "/api/directory") {
      await listed;
      return r.fulfill({ json: { daemons: [{ id: machine.id, name: "box", urls: [origin], online: true, last_seen: null, noise: machine.noise }] } });
    }
    if (url.pathname === `/api/machines/${machine.id}/token` && method === "POST") return r.fulfill({ json: { token: token(), expires: Math.floor(Date.now() / 1000) + 300, role: "owner" } });
    return r.fulfill({ status: 404, json: { error: "not here" } });
  });

  await page.goto(`${origin}/`);
  await controlPanel(page, "account");
  const before = await page.evaluate(() => window.__arugula.client.base);
  release();
  await expect.poll(() => page.evaluate((id) => window.__arugula.client.e2e?.daemon.id === id && window.__arugula.client.connected, machine.id), { timeout: 20_000 }).toBe(true);
  // The page did switch, so the overlay mounted again.
  expect(await page.evaluate(() => window.__arugula.client.base)).not.toBe(before);
  await expect(page.getByRole("dialog").getByRole("heading", { name: "Sign-in and account" })).toBeVisible();
});
