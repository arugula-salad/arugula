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
import { ready, run } from "./helpers";
import { keyHeaders } from "./local-token";
import { certBody, deviceId, hex, unhex, type Cert } from "../src/shell/e2e/cert.ts";
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
  const jwks = { keys: [{ kty: "OKP", crv: "Ed25519", kid: "k1", alg: "EdDSA", use: "sig", x: issuer.publicKey.export({ format: "jwk" }).x }] };
  control = createServer((req, res) => {
    const path = (req.url ?? "").split("?")[0];
    const body =
      path === "/control.json"
        ? { control: true, daemon_auth: 2 }
        : path === "/.well-known/jwks.json"
          ? jwks
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

test.afterAll(async () => {
  daemon?.kill();
  control?.close();
  if (state) rmSync(state, { recursive: true, force: true });
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
