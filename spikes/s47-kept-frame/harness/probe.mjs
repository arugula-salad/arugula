#!/usr/bin/env node
// S47 (#655): runs the frame facts in Chromium, Firefox and WebKit.
//   S47_WEB=../../../web node probe.mjs [chromium|firefox|webkit ...]

import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pw = createRequire(join(process.env.S47_WEB || join(here, "../../../web"), "package.json"))("@playwright/test");
const dir = mkdtempSync(join(process.env.S47_TMP || tmpdir(), "s47-"));
execFileSync("openssl", ["req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "2",
  "-subj", "/CN=example.com", "-addext", "subjectAltName=DNS:*.example.com,DNS:*.example.org",
  "-keyout", join(dir, "key.pem"), "-out", join(dir, "cert.pem")], { stdio: "ignore" });
const srv = spawn(process.execPath, [join(here, "serve.mjs"), "--port", "8473", "--proxy", "8474", "--cert", join(dir, "cert.pem"), "--key", join(dir, "key.pem")], { stdio: ["ignore", "ignore", "inherit"] });
process.on("exit", () => srv.kill());
await new Promise((r) => setTimeout(r, 600));

const CONTROL = "https://control.example.com";
const APP = "https://app.example.com";
const results = {};

async function launch(name) {
  const proxy = { server: "http://127.0.0.1:8474" };
  if (name === "firefox") return pw.firefox.launch({ proxy, firefoxUserPrefs: { "network.proxy.allow_hijacking_localhost": true } });
  if (name === "webkit") return pw.webkit.launch({ proxy });
  return pw.chromium.launch({ proxy, args: ["--ignore-certificate-errors"] });
}

// A frame in the page, loaded or refused. `timeout` is how long a blocked one gets.
const addFrame = (page, src, allow = "", timeout = 4000) => page.evaluate(([src, allow, timeout]) => new Promise((ok) => {
  const f = document.createElement("iframe");
  f.id = "f"; f.src = src; if (allow) f.allow = allow;
  const done = (v) => { removeEventListener("message", on); ok(v); };
  const on = (e) => { if (e.source === f.contentWindow && e.data && (e.data.loaded || e.data.hop)) done({ loaded: e.data.loaded || "hop page", origin: e.origin, docRequest: e.data.docRequest }); };
  addEventListener("message", on);
  document.body.append(f);
  setTimeout(() => done({ blocked: true }), timeout);
}), [src, allow, timeout]);

// Run a string of JS inside the frame through postMessage: the frame's own view.
const inFrame = (page, code, timeout = 5000) => page.evaluate(([code, timeout]) => new Promise((ok) => {
  const id = Math.random();
  const f = document.getElementById("f");
  const on = (e) => { if (e.data && e.data.id === id) { removeEventListener("message", on); ok(e.data.err ? { err: e.data.err } : e.data.ok); } };
  addEventListener("message", on);
  f.contentWindow.postMessage({ run: code, id }, "*");
  setTimeout(() => { removeEventListener("message", on); ok({ err: "no answer" }); }, timeout);
}), [code, timeout]);

const removeFrame = (page) => page.evaluate(() => document.getElementById("f")?.remove());
const short = (v) => (typeof v === "string" ? v.slice(0, 160) : v);

async function run(name) {
  // Playwright's Firefox hung loading a top-level page with COOP, so this leaves it off there.
  // A frame ignores COOP, so only the top-level comparison page changes.
  const NOCOOP = name === "firefox" ? "&nocoop=1" : "";
  const r = (results[name] = {});
  const browser = await launch(name);
  r.version = browser.version();
  const context = await browser.newContext({ ignoreHTTPSErrors: true });
  const page = await context.newPage();
  page.setDefaultTimeout(15000);
  await page.goto(`${CONTROL}/`);

  // 1. Headers.
  r["today's headers (DENY + 'none') frame?"] = await addFrame(page, `${APP}/?framing=none`); await removeFrame(page);
  r["frame-ancestors control, control frames it?"] = await addFrame(page, `${APP}/?framing=control`);

  // 2. The frame's cookies and storage against the top-level page's.
  await inFrame(page, `fetch("/set-cookie?name=framed_set").then(r=>r.status)`);
  r["cookie the frame sets, frame's request carries it"] = (await inFrame(page, `fetch("/echo-cookies").then(r=>r.json())`));
  await inFrame(page, `localStorage.setItem("from_frame","1"); sessionStorage.setItem("from_frame","1"); true`);
  const top = await context.newPage();
  await top.goto(`${APP}/?framing=control${NOCOOP}`);
  r["top-level app sees the frame's cookie"] = await top.evaluate(() => fetch("/echo-cookies").then((r) => r.json()));
  r["top-level app sees the frame's localStorage"] = await top.evaluate(() => localStorage.getItem("from_frame"));
  await top.evaluate(() => fetch("/set-cookie?name=top_set"));
  await top.evaluate(() => localStorage.setItem("from_top", "1"));
  r["frame sees top-level's cookie"] = await inFrame(page, `fetch("/echo-cookies").then(r=>r.json())`);
  r["frame sees top-level's localStorage"] = await inFrame(page, `localStorage.getItem("from_top")`);
  await top.close();
  await removeFrame(page);
  r["new framed document request carries the session cookie"] = await addFrame(page, `${APP}/?framing=control`);

  // 3. What the frame can reach.
  r["frame reads parent.document"] = await inFrame(page, `parent.document.title`);
  r["frame reads parent.localStorage"] = await inFrame(page, `parent.localStorage.length`);
  r["frame sets document.domain, reads parent"] = await inFrame(page, `document.domain="example.com"; parent.document.title`);
  r["frame fetches control with credentials"] = await inFrame(page, `fetch("${CONTROL}/echo-cookies",{credentials:"include"}).then(r=>r.status)`);
  r["frame origin-keyed (no header)"] = await inFrame(page, `window.originAgentCluster`);
  await removeFrame(page);
  await addFrame(page, `${APP}/?framing=control&oac=1`);
  r["frame origin-keyed (Origin-Agent-Cluster)"] = await inFrame(page, `window.originAgentCluster`);
  await removeFrame(page);
  await addFrame(page, `${APP}/?framing=control`);

  // A frame moving the top page with no click, and with one.
  const stay = await inFrame(page, `(()=>{ try { top.location = "https://stranger.example.org/"; return "called"; } catch (e) { return e.name } })()`);
  await page.waitForTimeout(800);
  r["frame moves the top page with no click"] = { call: stay, topNow: page.url() };
  if (!page.url().startsWith(CONTROL)) { await page.goto(`${CONTROL}/`); await addFrame(page, `${APP}/?framing=control`); }

  // 4. The hop from inside the frame.
  await removeFrame(page);
  r["hop: hub redirects (302) under frame-ancestors 'none'"] = await addFrame(page, `${APP}/hop-to-control?session=1`); await removeFrame(page);
  r["hop: hub redirects (302) under frame-ancestors 'self'"] = await addFrame(page, `${APP}/hop-to-control?session=1&fa=self`); await removeFrame(page);
  r["hop: hub's page under frame-ancestors 'none'"] = await addFrame(page, `${APP}/hop-to-control`); await removeFrame(page);
  r["hop: hub's page under frame-ancestors 'self'"] = await addFrame(page, `${APP}/hop-to-control?fa=self`); await removeFrame(page);
  r["GitHub-like stranger in a frame"] = await addFrame(page, `https://stranger.example.org/`); await removeFrame(page);

  // 5. A hidden frame: is it running?
  await addFrame(page, `${APP}/?framing=control`);
  await page.evaluate(() => { document.getElementById("f").style.display = "none"; });
  await page.waitForTimeout(300);
  r["display:none frame"] = await inFrame(page, `new Promise((ok)=>{ let t=0, raf=0; const i=setInterval(()=>t++,100); const loop=()=>{raf++;requestAnimationFrame(loop)}; requestAnimationFrame(loop); setTimeout(()=>{clearInterval(i); ok({visibility:document.visibilityState, hasFocus:document.hasFocus(), width:innerWidth, timersIn2s:t, rafIn2s:raf})},2000)})`, 6000);
  await page.evaluate(() => { const f = document.getElementById("f"); f.style.display = ""; f.style.visibility = "hidden"; });
  await page.waitForTimeout(300);
  r["visibility:hidden frame"] = await inFrame(page, `new Promise((ok)=>{ let t=0, raf=0; const i=setInterval(()=>t++,100); const loop=()=>{raf++;requestAnimationFrame(loop)}; requestAnimationFrame(loop); setTimeout(()=>{clearInterval(i); ok({visibility:document.visibilityState, width:innerWidth, timersIn2s:t, rafIn2s:raf})},2000)})`, 6000);
  await page.evaluate(() => { document.getElementById("f").style.visibility = ""; });

  // 6. History: a frame's pushState goes in the tab's back list.
  const before = await page.evaluate(() => history.length);
  await inFrame(page, `history.pushState({},"","/p/one"); history.pushState({},"","/p/two"); location.pathname`);
  const after = await page.evaluate(() => history.length);
  await page.goBack().catch(() => {});
  await page.waitForTimeout(300);
  r["history"] = { before, after, topAfterBack: page.url(), frameAfterBack: await inFrame(page, `location.pathname`) };
  await removeFrame(page);

  // 7. Powers a framed page loses without allow=, and keeps with it.
  const perms = `(async()=>{ const out={}; for (const n of ["camera","microphone","clipboard-write","publickey-credentials-get","fullscreen"]) { try { out[n]=document.featurePolicy?document.featurePolicy.allowsFeature(n):"no featurePolicy API" } catch(e){out[n]=e.name} } return out })()`;
  await addFrame(page, `${APP}/?framing=control`);
  r["powers without allow="] = await inFrame(page, perms); await removeFrame(page);
  await addFrame(page, `${APP}/?framing=control`, "camera; microphone; clipboard-write; publickey-credentials-get; publickey-credentials-create; fullscreen");
  r["powers with allow="] = await inFrame(page, perms); await removeFrame(page);

  // 8. Passkeys in the frame. Chromium has a virtual authenticator.
  if (name === "chromium") {
    const cdp = await context.newCDPSession(page);
    await cdp.send("WebAuthn.enable");
    await cdp.send("WebAuthn.addVirtualAuthenticator", { options: { protocol: "ctap2", transport: "internal", hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true } });
    const reg = await page.evaluate(() => navigator.credentials.create({ publicKey: { challenge: new Uint8Array(32), rp: { id: "control.example.com", name: "c" }, user: { id: new Uint8Array(8), name: "a", displayName: "a" }, pubKeyCredParams: [{ type: "public-key", alg: -7 }], authenticatorSelection: { residentKey: "required", userVerification: "required" } } }).then((c) => btoa(String.fromCharCode(...new Uint8Array(c.rawId)))));
    const get = (rp) => `(async()=>{ const raw = Uint8Array.from(atob("${reg}"),c=>c.charCodeAt(0)); const c = await navigator.credentials.get({ publicKey: { challenge: new Uint8Array(32), rpId: "${rp}", allowCredentials: [{type:"public-key",id:raw}], userVerification:"required" } }); const j = JSON.parse(new TextDecoder().decode(c.response.clientDataJSON)); return {origin:j.origin, crossOrigin:j.crossOrigin, topOrigin:j.topOrigin}; })()`;
    r["passkey get: top-level control"] = await page.evaluate(get("control.example.com"));
    // The virtual authenticator belongs to this tab, so the top-level app test runs here.
    await page.goto(`${APP}/?framing=control${NOCOOP}`);
    r["passkey get: top-level app (related origin)"] = await page.evaluate(`Promise.race([${get("control.example.com")}, new Promise((_, no) => setTimeout(() => no(new Error("no answer in 8s")), 8000))])`).catch((e) => ({ err: short(e.message) }));
    await page.goto(`${CONTROL}/`);
    await addFrame(page, `${APP}/?framing=control`);
    r["passkey get: framed app, no allow="] = await inFrame(page, get("control.example.com"), 8000); await removeFrame(page);
    await addFrame(page, `${APP}/?framing=control`, "publickey-credentials-get");
    r["passkey get: framed app, allow="] = await inFrame(page, get("control.example.com"), 8000); await removeFrame(page);
  } else {
    await addFrame(page, `${APP}/?framing=control`, "publickey-credentials-get");
    r["passkey get: framed app, allow= (no authenticator; SecurityError is a refusal)"] = await inFrame(page, `navigator.credentials.get({publicKey:{challenge:new Uint8Array(32),rpId:"control.example.com",userVerification:"required",timeout:2000}, signal:AbortSignal.timeout(2500)}).then(()=>"got one")`, 7000);
    await removeFrame(page);
  }
  await browser.close();
}

for (const n of process.argv.slice(2).length ? process.argv.slice(2) : ["chromium", "firefox", "webkit"]) {
  try { await run(n); } catch (e) { (results[n] ??= {}).failed = String(e).slice(0, 400); }
}
console.log(JSON.stringify(results, null, 1));
process.exit(0);
