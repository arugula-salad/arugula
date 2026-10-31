#!/usr/bin/env node
// S46 (#382): what Playwright's Firefox and WebKit builds say about related
// origins, with no authenticator. On each origin, a `get` for the RP's ID
// with a 3 s abort:
// - SecurityError at once: the browser refused the RP ID for this origin;
// - NotAllowedError / AbortError after the wait: it accepted the RP ID and
//   went looking for a passkey (none here).
//
//   S46_WEB=../../../web node probe-engines.mjs > ../results/probe-engines.txt
//
// Same throwaway certificate and CONNECT proxy as local-chromium.mjs. The
// browsers are told to ignore certificate errors for pages; a browser's own
// well-known fetch may still refuse the certificate, so a SecurityError on
// the app's origin here is a hint, not proof (the click-through sheet is).

import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const web = process.env.S46_WEB || join(here, "../../../web");
const pw = createRequire(join(web, "package.json"))("@playwright/test");

const dir = mkdtempSync(join(process.env.S46_TMP || tmpdir(), "s46-"));
execFileSync("openssl", [
  "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "2",
  "-subj", "/CN=s46.test", "-addext", "subjectAltName=DNS:s46.test,DNS:*.s46.test",
  "-keyout", join(dir, "key.pem"), "-out", join(dir, "cert.pem"),
], { stdio: "ignore" });
const srv = spawn(process.execPath, [
  join(here, "serve.mjs"), "--rp", "rp.s46.test", "--app", "app.s46.test", "--stranger", "stranger.s46.test",
  "--port", "8463", "--proxy", "8464", "--cert", join(dir, "cert.pem"), "--key", join(dir, "key.pem"), "--log", join(dir, "log.json"),
], { stdio: ["ignore", "pipe", "inherit"] });
const seen = [];
srv.stdout.on("data", (d) => seen.push(...String(d).trim().split("\n")));
process.on("exit", () => srv.kill());
await new Promise((ok) => setTimeout(ok, 600));

async function probe(name, type) {
  let browser;
  try {
    browser = await type.launch({ proxy: { server: "http://127.0.0.1:8464" } });
  } catch (e) {
    console.log(`\n## ${name}: can't launch (${String(e).split("\n")[0]})`);
    return;
  }
  const ctx = await browser.newContext({ ignoreHTTPSErrors: true });
  const page = await ctx.newPage();
  console.log(`\n## ${name} ${browser.version()}`);
  for (const host of ["rp.s46.test", "app.s46.test", "stranger.s46.test"]) {
    const before = seen.length;
    await page.goto(`https://${host}/`);
    await page.waitForFunction(() => window.__s46ready, null, { timeout: 15000 }).catch(() => {});
    const r = await page.evaluate(async () => {
      const out = { secure: isSecureContext, pkc: typeof PublicKeyCredential };
      if (typeof PublicKeyCredential === "undefined") return out;
      out.caps = PublicKeyCredential.getClientCapabilities ? await PublicKeyCredential.getClientCapabilities() : "none";
      const ac = new AbortController();
      setTimeout(() => ac.abort(), 3000);
      const t = performance.now();
      try {
        await navigator.credentials.get({ publicKey: { challenge: new Uint8Array(32), rpId: "rp.s46.test", userVerification: "required" }, signal: ac.signal });
        out.get = "a passkey?";
      } catch (e) {
        out.get = `${e.name} after ${Math.round(performance.now() - t)} ms: ${e.message}`.slice(0, 200);
      }
      return out;
    });
    const wk = seen.slice(before).filter((l) => l.includes("/.well-known/webauthn") && !l.includes("origin=https") && !l.includes("site=same-origin"));
    console.log(`- ${host}: secure=${r.secure} PublicKeyCredential=${r.pkc}`);
    if (r.caps) console.log(`  relatedOrigins=${r.caps.relatedOrigins} uvpa=${r.caps.userVerifyingPlatformAuthenticator} hybrid=${r.caps.hybridTransport}`);
    if (r.get) console.log(`  get(rpId rp.s46.test): ${r.get}`);
    console.log(`  the browser's own well-known fetches: ${wk.length}`);
  }
  await browser.close();
}

await probe("Playwright Firefox", pw.firefox);
await probe("Playwright WebKit", pw.webkit);
process.exit(0);
