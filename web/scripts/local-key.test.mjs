// This daemon's page key (shell/local-key.ts): only the page's own daemon
// (this host and port, http or ws) gets it, never another port on the same
// host or another host; a WebSocket carries it in its URL; only loopback
// pages use it. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { keyedRequest, keyedSocketUrl, onLoopback, ownApiUrl, ownUrl } from "../src/shell/local-key.ts";

const page = { href: "http://127.0.0.1:7681/#pane=3", host: "127.0.0.1:7681" };

test("only this page's own daemon is ours", () => {
  for (const u of ["/api/host", "http://127.0.0.1:7681/api/x", "ws://127.0.0.1:7681/ws", "/h/box/ws"]) assert.ok(ownUrl(u, page), u);
  // Another port on the same host is another server: a cookie would reach it, the key doesn't.
  for (const u of ["http://127.0.0.1:9000/", "ws://127.0.0.1:9000/ws", "http://localhost:7681/", "https://127.0.0.1:7681/", "https://box.example.ts.net/api", "data:text/plain,x", "blob:http://127.0.0.1:7681/x"])
    assert.equal(ownUrl(u, page), null, u);
});

test("the key goes to the daemon's API and its ways to other hosts, not other paths", () => {
  for (const u of ["/api/host", "/ws", "ws://127.0.0.1:7681/ws?surface=web", "/h/box/api/panes", "/h/box/ws", "/tunnel/box/api/host", "/tunnel/box/ws"]) assert.ok(ownApiUrl(u, page), u);
  for (const u of ["/", "/assets/x.js", "/auth?next=/", "/share/abc", "/h/box/", "/h/box/other", "/wsx", "/apix", "/control.json", "http://127.0.0.1:9000/api/host"]) assert.equal(ownApiUrl(u, page), null, u);
  assert.equal(keyedSocketUrl("ws://127.0.0.1:7681/share/abc/ws", page, "ilp_k"), "ws://127.0.0.1:7681/share/abc/ws");
});

test("a WebSocket to our daemon carries the key in its URL, others don't", () => {
  assert.equal(keyedSocketUrl("ws://127.0.0.1:7681/ws?surface=web", page, "ilp_k"), "ws://127.0.0.1:7681/ws?surface=web&arugula_key=ilp_k");
  assert.equal(keyedSocketUrl("ws://127.0.0.1:9000/ws", page, "ilp_k"), "ws://127.0.0.1:9000/ws");
  assert.equal(keyedSocketUrl("wss://control.example/api/relay/m", page, "ilp_k"), "wss://control.example/api/relay/m");
  assert.equal(keyedSocketUrl("ws://127.0.0.1:7681/ws", page, null), "ws://127.0.0.1:7681/ws");
});

test("a keyed request carries the key and follows no redirect; an unkeyed one is untouched", () => {
  const [u, o] = keyedRequest("/api/host", { method: "POST", headers: { "content-type": "application/json" } }, "ilp_k");
  assert.equal(u, "/api/host");
  assert.equal(o.redirect, "manual");
  assert.equal(new Headers(o.headers).get("x-arugula-key"), "ilp_k");
  assert.equal(new Headers(o.headers).get("content-type"), "application/json");
  const [r, none] = keyedRequest(new Request("http://127.0.0.1:7681/api/x", { headers: { a: "1" } }), undefined, "ilp_k");
  assert.equal(none, undefined);
  assert.equal(r.redirect, "manual");
  assert.equal(r.headers.get("x-arugula-key"), "ilp_k");
  assert.equal(r.headers.get("a"), "1");
  const init = { method: "GET" };
  assert.deepEqual(keyedRequest("/api/x", init, null), ["/api/x", init]);
});

test("loopback pages only", () => {
  for (const h of ["127.0.0.1", "localhost", "[::1]"]) assert.ok(onLoopback({ protocol: "http:", hostname: h }), h);
  assert.ok(!onLoopback({ protocol: "https:", hostname: "box.example.ts.net" }));
  assert.ok(!onLoopback({ protocol: "http:", hostname: "100.64.0.1" }));
  assert.ok(!onLoopback({ protocol: "https:", hostname: "localhost" }));
});
