// A native app pairs by code from a signed-in browser (aru-131): the page asks,
// control records it, and the app's signature then works on its own routes and
// no others. The app is played here by node:crypto, with the keys it would
// keep in its keychain.

import { spawn, type ChildProcess } from "node:child_process";
import { createHash, createPrivateKey, generateKeyPairSync, randomBytes, sign as edSign, type KeyObject } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { closeContexts } from "./helpers";
import { ANY, controlPort, listen } from "./ports";

test.afterAll(closeContexts);

let base = "";
let dir = "";
let control: ChildProcess | undefined;
let gh: Server;

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
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-native-app-"));
  const db = join(dir, "control.db");
  control = spawn(
    "../target/debug/arugula-control",
    [
      ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db, "--native-clients"],
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

const raw = (k: KeyObject) => Buffer.from(k.export({ format: "jwk" }).x!, "base64url");

/** A new app's keys and its request, as `Cert::new` makes one. */
function newApp(name: string) {
  const signing = generateKeyPairSync("ed25519");
  const noise = raw(generateKeyPairSync("x25519").publicKey);
  const sign = raw(signing.publicKey);
  const device = createHash("sha256").update("illogical device id").update(noise).update(sign).digest().subarray(0, 8).toString("hex");
  const cert = { v: 1, account: "", device, kind: "browser", name, noise: noise.toString("hex"), sign: sign.toString("hex"), created: Date.now(), approver: "", sig: "" };
  const key = createPrivateKey(signing.privateKey.export({ format: "pem", type: "pkcs8" }));
  return { cert, key };
}

const signText = (key: KeyObject, text: string) => edSign(null, Buffer.from(text), key).toString("hex");

function body(c: { account: string; device: string; kind: string; name: string; noise: string; sign: string; created: number; approver: string }) {
  return `illogical device v1\naccount ${c.account}\ndevice ${c.device}\nkind ${c.kind}\nname ${c.name}\nnoise ${c.noise}\nsign ${c.sign}\ncreated ${c.created}\napprover ${c.approver}\n`;
}

/** A GET the app signs, as `request_auth` does. */
function signedGet(path: string, device: string, key: KeyObject) {
  const ms = Date.now();
  const nonce = randomBytes(16).toString("hex");
  const digest = createHash("sha256").update("").digest("hex");
  const sig = signText(key, `illogical daemon auth v2\nGET\n${path}\n${ms}\n${nonce}\n${digest}\n`);
  return fetch(`${base}${path}`, { headers: { "x-arugula-auth": `v2 ${device} ${ms} ${nonce} ${sig}` } });
}

test("a signed-in browser pairs the app, whose signature then works", async ({ page }) => {
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();

  const { cert, key } = newApp("Arugula on iPhone");
  const ms = Date.now();
  const asked = await fetch(`${base}/api/join`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ cert, urls: [], proof: { ms, sig: signText(key, `illogical join proof v1\n${ms}\n${body(cert)}`) } }),
  });
  expect(asked.status).toBe(200);
  const { code, poll } = await asked.json();

  // Not paired yet: its key signs nothing.
  expect((await signedGet("/api/me", cert.device, key)).status).toBe(401);

  await page.goto(`/#join=${code}`);
  await expect(page.locator("[data-join-app]")).toContainText("Arugula on iPhone");
  await expect(page.locator(`[data-join-code="${code}"]`)).toBeVisible();
  await page.screenshot({ path: "test-results/native-app-join.png" });
  await page.locator("[data-approve-join]").click();
  await expect(page.locator("[data-join-app]")).toHaveCount(0);

  const answered = await (await fetch(`${base}/api/join/${code}?poll=${poll}`)).json();
  expect(answered.approved).toBe(true);

  const me = await signedGet("/api/me", cert.device, key);
  expect(me.status).toBe(200);
  expect((await me.json()).login).toBe("stranger");
  // Account security stays on the web.
  const passkeys = await signedGet("/api/me/passkeys", cert.device, key);
  expect(passkeys.status).toBe(401);
  expect((await passkeys.json()).reason).toBe("page_only");
});
