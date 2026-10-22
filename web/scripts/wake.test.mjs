// Waking a hosted machine on connect (M182), the page's side of
// `/api/relay/m`: an open that asks to wake sends OPEN_WAKE, hears
// `waking` (and waits past its usual timeout while the box wakes), then
// opens, or hears why the box didn't wake. Run with
// `node --experimental-strip-types`; a stand-in WebSocket plays control.
import { test } from "node:test";
import assert from "node:assert/strict";

const sockets = [];
class FakeSocket {
  static OPEN = 1;
  static CONNECTING = 0;
  static CLOSED = 3;
  readyState = 0;
  sent = [];
  constructor(url) {
    this.url = url;
    sockets.push(this);
    setTimeout(() => {
      this.readyState = 1;
      this.onopen?.();
    }, 0);
  }
  send(f) {
    this.sent.push(new Uint8Array(f));
  }
  close() {
    this.readyState = 3;
  }
  /** Control sends a frame. */
  say(kind, chan, payload = "") {
    const p = new TextEncoder().encode(payload);
    const f = new Uint8Array(5 + p.length);
    f[0] = kind;
    new DataView(f.buffer).setUint32(1, chan);
    f.set(p, 5);
    this.onmessage?.({ data: f.buffer });
  }
}
globalThis.WebSocket = FakeSocket;
const { RelayMux } = await import("../src/shell/e2e/relaymux.ts");

const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms));
const opened = (ws, n) => {
  const f = ws.sent[n];
  return { kind: f[0], chan: new DataView(f.buffer).getUint32(1), id: new TextDecoder().decode(f.subarray(5)) };
};

test("an open that asks to wake waits while the box wakes, then opens", async () => {
  const mux = RelayMux.for("wss://control.test/api/relay/m#a");
  const heard = [];
  const open = mux.open("box1", 100, true, (w) => heard.push(w));
  await tick(5);
  const ws = sockets.at(-1);
  const o = opened(ws, 0);
  assert.deepEqual({ kind: o.kind, id: o.id }, { kind: 5, id: "box1" }, "OPEN_WAKE");
  ws.say(6, o.chan, JSON.stringify({ state: "waking" }));
  // Past the open's own timeout: waking holds it.
  await tick(200);
  ws.say(4, o.chan);
  const ch = await open;
  assert.equal(ch.readyState, FakeSocket.OPEN);
  assert.deepEqual(heard, [{ state: "waking" }]);
});

test("a box that didn't wake says why, and an open that didn't ask hears it's asleep", async () => {
  const mux = RelayMux.for("wss://control.test/api/relay/m#b");
  const open = mux.open("box2", 5_000, true);
  await tick(5);
  const ws = sockets.at(-1);
  const o = opened(ws, 0);
  ws.say(6, o.chan, JSON.stringify({ state: "waking" }));
  ws.say(6, o.chan, JSON.stringify({ state: "failed", code: "gone", why: "The box is gone." }));
  ws.say(3, o.chan, "The box is gone.");
  await assert.rejects(open, (e) => e.message === "The box is gone." && e.refused && e.wake.code === "gone");

  const plain = mux.open("box2", 5_000);
  await tick(5);
  const p = opened(ws, 1);
  assert.equal(p.kind, 1, "a plain OPEN");
  ws.say(6, p.chan, JSON.stringify({ state: "asleep" }));
  ws.say(3, p.chan, "asleep");
  await assert.rejects(plain, (e) => e.wake.state === "asleep");
});

test("without a word from control, an open still times out", async () => {
  const mux = RelayMux.for("wss://control.test/api/relay/m#c");
  const open = mux.open("box3", 50, true);
  await assert.rejects(open, /timed out/);
});
