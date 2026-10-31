#!/usr/bin/env node
// S46 (#382) harness server: one process serves every test origin, told
// apart by Host. No dependencies (Node 20+).
//
//   node serve.mjs --rp rp.example --app app.example --stranger other.example \
//     [--parent example] [--team ABCDE12345] [--port 8443 --cert c.pem --key k.pem | --http 8080] \
//     [--proxy 8444] [--no-well-known] [--log log.json]
//
// - `/` on any host: the harness page (index.html).
// - `/config.json`: the hosts, so the page knows its role.
// - `/.well-known/webauthn` on the RP's host only: `{"origins": [the app's]}`
//   (WebAuthn Related Origin Requests). The stranger is never listed.
// - `/.well-known/apple-app-site-association` on every host with --team:
//   `webcredentials` naming <team>.io.arugula.desktop, for a WKWebView test.
// - `/log` (GET) and `/log/append` (POST): one test account's passkey log,
//   in passkey-log.mjs's format, kept in memory and in --log.
// - `--http`: plain HTTP behind something that ends TLS (cloudflared). The
//   origins it names are then https on 443.
// - `--proxy`: an HTTP CONNECT proxy that sends every host:443 to this
//   server's TLS port, so a local browser (`--proxy-server`) reaches
//   `https://rp.s46.test/` with no DNS and no privileged port. Local runs
//   only (local-chromium.mjs).

import { createServer as https } from "node:https";
import { createServer as http } from "node:http";
import { connect } from "node:net";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { append } from "./passkey-log.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = args.indexOf(`--${name}`);
  return i < 0 ? dflt : args[i + 1];
};
const flag = (name) => args.includes(`--${name}`);

const cfg = {
  rp: opt("rp"),
  app: (opt("app") || "").split(",").filter(Boolean),
  stranger: opt("stranger"),
  parent: opt("parent") || null,
  team: opt("team") || null,
  wellKnown: !flag("no-well-known"),
  account: "acc_s46_test",
};
if (!cfg.rp || !cfg.app.length || !cfg.stranger) {
  console.error("serve.mjs: --rp, --app and --stranger are required");
  process.exit(2);
}
const origin = (h) => `https://${h}`;
const logFile = opt("log", join(here, "log.json"));
const log = existsSync(logFile) ? JSON.parse(readFileSync(logFile, "utf8")) : [];

const files = {
  "/": ["index.html", "text/html; charset=utf-8"],
  "/index.html": ["index.html", "text/html; charset=utf-8"],
  "/harness.js": ["harness.js", "text/javascript"],
  "/passkey-log.mjs": ["passkey-log.mjs", "text/javascript"],
};

function send(res, status, type, body, extra = {}) {
  res.writeHead(status, { "content-type": type, "cache-control": "no-store", ...extra });
  res.end(body);
}

async function handle(req, res) {
  const host = (req.headers.host || "").replace(/:\d+$/, "");
  const path = new URL(req.url, "http://x").pathname;
  // How a well-known was asked for: Chrome's own related-origins fetch shows
  // as mode=no-cors site=none, with no Origin and no cookie; the page's
  // shows its Origin (or site=same-origin).
  const who = `origin=${req.headers.origin || "-"} mode=${req.headers["sec-fetch-mode"] || "-"} dest=${req.headers["sec-fetch-dest"] || "-"} site=${req.headers["sec-fetch-site"] || "-"} cookie=${req.headers.cookie ? "yes" : "no"}`;
  console.log(new Date().toISOString(), host, req.method, path, path.startsWith("/.well-known/") ? who : "");
  if (path === "/.well-known/webauthn") {
    if (host !== cfg.rp || !cfg.wellKnown) return send(res, 404, "text/plain", "not here\n");
    return send(res, 200, "application/json", JSON.stringify({ origins: cfg.app.map(origin) }), {
      "access-control-allow-origin": "*",
    });
  }
  if (path === "/.well-known/apple-app-site-association" && cfg.team) {
    return send(res, 200, "application/json", JSON.stringify({ webcredentials: { apps: [`${cfg.team}.io.arugula.desktop`] } }));
  }
  if (path === "/config.json") {
    const role = host === cfg.rp ? "rp" : cfg.app.includes(host) ? "app" : host === cfg.stranger ? "stranger" : "unknown";
    const origins = { [cfg.rp]: [origin(cfg.rp), ...cfg.app.map(origin)] };
    if (cfg.parent) origins[cfg.parent] = [cfg.rp, ...cfg.app, cfg.stranger].map(origin);
    return send(res, 200, "application/json", JSON.stringify({ ...cfg, role, host, origins }));
  }
  if (path === "/log" && req.method === "GET") return send(res, 200, "application/json", JSON.stringify(log));
  if (path === "/log/append" && req.method === "POST") {
    let raw = "";
    for await (const c of req) raw += c;
    const e = JSON.parse(raw);
    const full = await append(log, { ...e, account: cfg.account, at: Date.now(), by: "github" });
    writeFileSync(logFile, JSON.stringify(log, null, 1));
    return send(res, 200, "application/json", JSON.stringify(full));
  }
  const f = files[path];
  if (f) return send(res, 200, f[1], readFileSync(join(here, f[0])));
  return send(res, 404, "text/plain", "no\n");
}

const wrap = (req, res) => handle(req, res).catch((e) => send(res, 500, "text/plain", String(e)));

const httpPort = opt("http");
if (httpPort) {
  http(wrap).listen(+httpPort, "127.0.0.1", () => console.log(`http on 127.0.0.1:${httpPort}`));
} else {
  const port = +opt("port", "8443");
  const tls = { cert: readFileSync(opt("cert")), key: readFileSync(opt("key")) };
  https(tls, wrap).listen(port, "127.0.0.1", () => console.log(`https on 127.0.0.1:${port}`));
  const proxy = opt("proxy");
  if (proxy) {
    http((_, res) => send(res, 405, "text/plain", "CONNECT only\n"))
      .on("connect", (req, sock, head) => {
        const up = connect(port, "127.0.0.1", () => {
          sock.write("HTTP/1.1 200 Connection established\r\n\r\n");
          up.write(head);
          up.pipe(sock).pipe(up);
        });
        up.on("error", () => sock.destroy());
        sock.on("error", () => up.destroy());
      })
      .listen(+proxy, "127.0.0.1", () => console.log(`CONNECT proxy on 127.0.0.1:${proxy} -> :${port}`));
  }
}
