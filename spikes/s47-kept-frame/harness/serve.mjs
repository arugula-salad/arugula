#!/usr/bin/env node
// S47 (#655): what a browser does with a page on one origin framed by a page
// on a sibling origin. One TLS server, told apart by Host, plus a CONNECT
// proxy so a browser reaches https://control.example.com/ with no DNS and no
// privileged port (S46's trick). No dependencies.
//
//   node serve.mjs --port 8473 --proxy 8474 --cert c.pem --key k.pem
//
// Hosts: control.example.com (the keys' page), app.example.com (the platform
// page), stranger.example.org (another site, standing in for GitHub). These
// names are on the public suffix list, as arugula.io is, so browsers treat the
// first two as same-site. A made-up TLD does not behave the same in WebKit.
// Nothing leaves this computer: the proxy sends every name to this server.
//
// The harness pages allow eval so the probe can run code in them; nothing tested depends on it.
// app's headers are what production sends today except for what the query
// asks for: ?framing=none|control|any, ?oac=1 (Origin-Agent-Cluster: ?1).

import { createServer as https } from "node:https";
import { connect } from "node:net";
import { readFileSync } from "node:fs";

const opt = (n) => process.argv[process.argv.indexOf(`--${n}`) + 1];
const port = Number(opt("port"));
const proxyPort = Number(opt("proxy"));
const CONTROL = "https://control.example.com";
const APP = "https://app.example.com";

const appCsp = (fa) =>
  `default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-src 'none'; frame-ancestors ${fa}; base-uri 'none'; form-action 'self'; object-src 'none'`;

const hop = `<!doctype html><title>signin</title><script>parent.postMessage({hop:"page"},"*")</script>`;

function page(role, url, seen) {
  return `<!doctype html><meta charset=utf-8><title>${role}</title><body data-role="${role}"><h1>${role}</h1>
<script>
const report = (m) => { try { parent.postMessage({ from: "${role}", ...m }, "*"); } catch {} };
addEventListener("message", async (e) => {
  if (!e.data || !e.data.run) return;
  let out;
  try { out = { ok: await (0, eval)(e.data.run) }; } catch (err) { out = { err: err.name + ": " + err.message }; }
  e.source.postMessage({ id: e.data.id, ...out }, e.origin);
});
report({ loaded: location.href, docRequest: ${JSON.stringify(seen)} });
</script>`;
}

const server = https({ cert: readFileSync(opt("cert")), key: readFileSync(opt("key")) }, (req, res) => {
  const host = (req.headers.host || "").replace(/:\d+$/, "");
  const url = new URL(req.url, `https://${host}`);
  const h = { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" };
  const send = (status, body, extra = {}) => { res.writeHead(status, { ...h, ...extra }); res.end(body); };
  const cookies = req.headers.cookie || "";

  if (url.pathname === "/.well-known/webauthn" && host === "control.example.com") {
    return send(200, JSON.stringify({ origins: [APP] }), { "content-type": "application/json" });
  }
  if (url.pathname === "/echo-cookies") {
    return send(200, JSON.stringify({ host, cookies, site: req.headers["sec-fetch-site"], dest: req.headers["sec-fetch-dest"] }), { "content-type": "application/json" });
  }
  if (url.pathname === "/set-cookie") {
    return send(200, "{}", { "content-type": "application/json", "set-cookie": `${url.searchParams.get("name")}=1; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=600` });
  }
  if (host === "app.example.com") {
    const framing = url.searchParams.get("framing") || "none";
    const fa = framing === "control" ? CONTROL : framing === "any" ? "*" : "'none'";
    const extra = { "content-security-policy": appCsp(fa), "x-content-type-options": "nosniff", "referrer-policy": "no-referrer" };
    if (!url.searchParams.get("nocoop")) extra["cross-origin-opener-policy"] = "same-origin";
    if (framing === "none" || framing === "xfo") extra["x-frame-options"] = "DENY";
    if (url.searchParams.get("oac")) extra["origin-agent-cluster"] = "?1";
    if (url.pathname === "/hop-to-control") {
      return send(302, "", { location: `${CONTROL}/signin?${url.searchParams}`, ...extra });
    }
    return send(200, page("app", url, { cookies, site: req.headers["sec-fetch-site"], dest: req.headers["sec-fetch-dest"] }), extra);
  }
  if (host === "control.example.com") {
    if (url.pathname === "/signin") {
      // The hub's hop: a redirect if it has a session, else its own page.
      const fa = url.searchParams.get("fa") === "self" ? "'self'" : "'none'";
      const csp = { "content-security-policy": `default-src 'self'; script-src 'self' 'unsafe-inline'; frame-ancestors ${fa}` };
      if (url.searchParams.get("session")) return send(302, "", { location: `${APP}/?framing=control&code=1`, ...csp });
      return send(200, hop, csp);
    }
    const extra = { "content-security-policy": `default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; connect-src 'self' wss: https:; frame-src 'self' https:; frame-ancestors 'none'` };
    if (url.searchParams.get("oac")) extra["origin-agent-cluster"] = "?1";
    return send(200, page("control", url), extra);
  }
  // The stranger, like GitHub's sign-in.
  const open = url.searchParams.has("open") ? {} : { "x-frame-options": "DENY", "content-security-policy": "frame-ancestors 'none'" };
  return send(200, page("stranger", url), open);
});
server.listen(port, "127.0.0.1", () => console.log(`serving ${port}`));

const proxy = (await import("node:http")).createServer((_, res) => { res.writeHead(405); res.end(); });
proxy.on("connect", (req, sock, head) => {
  const up = connect(port, "127.0.0.1", () => { sock.write("HTTP/1.1 200 OK\r\n\r\n"); up.write(head); up.pipe(sock); sock.pipe(up); });
  up.on("error", () => sock.destroy());
  sock.on("error", () => up.destroy());
});
proxy.listen(proxyPort, "127.0.0.1");
