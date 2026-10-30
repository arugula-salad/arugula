// This daemon's page key (shell/local-key.ts): only the page's own daemon
// (this host and port, http or ws) gets it, never another port on the same
// host or another host; a WebSocket carries it in its URL; only loopback
// pages use it. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { keyedSocketUrl, onLoopback, ownUrl } from "../src/shell/local-key.ts";

const page = { href: "http://127.0.0.1:7681/#pane=3", host: "127.0.0.1:7681" };

test("only this page's own daemon is ours", () => {
  for (const u of ["/api/host", "http://127.0.0.1:7681/api/x", "ws://127.0.0.1:7681/ws", "/h/box/ws"]) assert.ok(ownUrl(u, page), u);
  // Another port on the same host is another server: a cookie would reach it, the key doesn't.
  for (const u of ["http://127.0.0.1:9000/", "ws://127.0.0.1:9000/ws", "http://localhost:7681/", "https://127.0.0.1:7681/", "https://box.example.ts.net/api", "data:text/plain,x", "blob:http://127.0.0.1:7681/x"])
    assert.equal(ownUrl(u, page), null, u);
});

test("a WebSocket to our daemon carries the key in its URL, others don't", () => {
  assert.equal(keyedSocketUrl("ws://127.0.0.1:7681/ws?surface=web", page, "ilp_k"), "ws://127.0.0.1:7681/ws?surface=web&arugula_key=ilp_k");
  assert.equal(keyedSocketUrl("ws://127.0.0.1:9000/ws", page, "ilp_k"), "ws://127.0.0.1:9000/ws");
  assert.equal(keyedSocketUrl("wss://control.example/api/relay/m", page, "ilp_k"), "wss://control.example/api/relay/m");
  assert.equal(keyedSocketUrl("ws://127.0.0.1:7681/ws", page, null), "ws://127.0.0.1:7681/ws");
});

test("loopback pages only", () => {
  for (const h of ["127.0.0.1", "localhost", "[::1]"]) assert.ok(onLoopback({ protocol: "http:", hostname: h }), h);
  assert.ok(!onLoopback({ protocol: "https:", hostname: "box.example.ts.net" }));
  assert.ok(!onLoopback({ protocol: "http:", hostname: "100.64.0.1" }));
  assert.ok(!onLoopback({ protocol: "https:", hostname: "localhost" }));
});
