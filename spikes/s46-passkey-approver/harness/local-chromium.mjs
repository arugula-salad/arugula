#!/usr/bin/env node
// S46 (#382): related origins in a real Chromium with its virtual
// authenticator, on three https origins that exist only on this computer
// (rp.s46.test, app.s46.test, stranger.s46.test; parent s46.test).
//
//   node local-chromium.mjs [chrome|chromium] > ../results/local-chromium.txt
//
// - A throwaway certificate for *.s46.test, which the browser accepts by its
//   key's hash (--ignore-certificate-errors-spki-list, as web-platform-tests
//   run WebAuthn). Nothing is added to any trust store.
// - serve.mjs's CONNECT proxy sends every host:443 to its TLS port, so the
//   browser's own fetch of https://rp.s46.test/.well-known/webauthn reaches
//   it too: no DNS, no privileged port.
// - S46_TMP: where the certificate and logs go (default: the system tmp).
// - Uses the repo's Playwright (web/node_modules; S46_WEB to point at
//   another checkout's web/).
//
// The virtual authenticator always verifies the person, so this shows the
// browser's half only: that it checks the well-known file and what the
// assertion carries. Real providers are the test sheet's (TEST-SHEET.md).

import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const web = process.env.S46_WEB || join(here, "../../../web");
const { chromium } = createRequire(join(web, "package.json"))("@playwright/test");
const which = process.argv[2] || "chrome";

// S46_TMP, not TMPDIR: Chrome keeps a socket under TMPDIR, and a long path kills it.
const dir = mkdtempSync(join(process.env.S46_TMP || tmpdir(), "s46-"));
const servers = [];
process.on("exit", () => servers.forEach((p) => p.kill()));
process.on("uncaughtException", (e) => { console.error(e); process.exit(1); });
process.on("unhandledRejection", (e) => { console.error(e); process.exit(1); });
const sh = (cmd, args, input) => execFileSync(cmd, args, { input, encoding: "utf8" });
sh("openssl", [
  "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "2",
  "-subj", "/CN=s46.test", "-addext", "subjectAltName=DNS:s46.test,DNS:*.s46.test",
  "-keyout", join(dir, "key.pem"), "-out", join(dir, "cert.pem"),
]);
const der = execFileSync("sh", ["-c", `openssl x509 -in ${join(dir, "cert.pem")} -pubkey -noout | openssl pkey -pubin -outform der`]);
const spki = execFileSync("openssl", ["dgst", "-sha256", "-binary"], { input: der }).toString("base64");

function server(port, proxy, extra = []) {
  const p = spawn(process.execPath, [
    join(here, "serve.mjs"), "--rp", "rp.s46.test", "--app", "app.s46.test", "--stranger", "stranger.s46.test",
    "--parent", "s46.test", "--port", String(port), "--proxy", String(proxy),
    "--cert", join(dir, "cert.pem"), "--key", join(dir, "key.pem"), "--log", join(dir, `log-${port}.json`), ...extra,
  ], { stdio: ["ignore", "pipe", "inherit"] });
  servers.push(p);
  p.requests = [];
  p.stdout.on("data", (d) => p.requests.push(...String(d).trim().split("\n")));
  return new Promise((ok) => setTimeout(() => ok(p), 600));
}

async function run(title, proxy, steps) {
  const browser = await chromium.launch({
    ...(which === "chrome" ? { channel: "chrome" } : {}),
    args: [`--proxy-server=http://127.0.0.1:${proxy}`, "--proxy-bypass-list=<-loopback>", `--ignore-certificate-errors-spki-list=${spki}`],
  });
  const page = await (await browser.newContext()).newPage();
  page.setDefaultTimeout(20000);
  page.on("console", (m) => process.stderr.write(`[page] ${m.text()}\n`));
  page.on("pageerror", (e) => process.stderr.write(`[page error] ${e}\n`));
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("WebAuthn.enable", { enableUI: false });
  await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", ctap2Version: "ctap2_1", transport: "internal", hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  console.log(`\n## ${title} (${browser.version()})`);
  let caps;
  for (const [host, button] of steps) {
    process.stderr.write(`[step] ${host} ${button}\n`);
    if (!page.url().startsWith(`https://${host}/`)) {
      await page.goto(`https://${host}/`);
      await page.waitForFunction(() => window.__s46ready);
      caps ??= await page.evaluate(() => JSON.parse(document.getElementById("caps").textContent));
    }
    const before = await page.evaluate(() => (window.__s46 || []).length);
    await page.click(`#${button}`);
    await page.waitForFunction((n) => (window.__s46 || []).length > n, before, { timeout: 30000 });
    const r = await page.evaluate(() => window.__s46.at(-1));
    const f = r.flags ? Object.entries(r.flags).filter(([, v]) => v).map(([k]) => k).join(",") : "";
    console.log(
      `- ${host.padEnd(17)} ${r.step.padEnd(24)} ${r.ok ? "ok " : "NO "} ${r.ok ? `flags ${f}; origin ${r.origin}; ${r.verified || r.provider}` : r.error}`,
    );
  }
  console.log(`- clientCapabilities: ${JSON.stringify(caps.clientCapabilities)}`);
  console.log(`- the page's fetch of the RP's well-known: ${caps.wellKnown}`);
  await browser.close();
}

const a = await server(8443, 8444);
await run("with /.well-known/webauthn listing app.s46.test", 8444, [
  ["rp.s46.test", "reg-rp"],
  ["rp.s46.test", "get-rp"],
  ["app.s46.test", "get-rp"],
  ["stranger.s46.test", "get-rp"],
  ["app.s46.test", "reg-parent"],
  ["rp.s46.test", "get-parent"],
  ["stranger.s46.test", "get-parent"],
]);
const wk = a.requests.filter((l) => l.includes("rp.s46.test GET /.well-known/webauthn"));
for (const l of wk) console.log(`- well-known fetch: ${l.split("/.well-known/webauthn ")[1]}`);
a.kill();

const b = await server(8453, 8454, ["--no-well-known"]);
await run("without it (rp.s46.test answers 404)", 8454, [
  ["rp.s46.test", "reg-rp"],
  ["app.s46.test", "get-rp"],
]);
b.kill();
writeFileSync(join(dir, "done"), "");
