// connect()'s Room over a fake transport: your sub from the token, the
// relay's origin for demo tokens and models, events for members, waves,
// answers and the connection, and the starting mode where none is kept.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { Room, connect, httpOrigin, subOf } from "../src/room.ts";
import { assets } from "../../core/src/assets.ts";

const token = (claims) => ["e30", Buffer.from(JSON.stringify(claims)).toString("base64url"), "sig"].join(".");

function fake() {
  const t = { on: null, listen(on) { t.on = on; }, unlisten() {}, publish() {}, off() {}, state() {}, wave(to) { t.waved = to; }, answer() {} };
  return t;
}

test("the relay's HTTP origin, and a token's sub", () => {
  assert.equal(httpOrigin("wss://arugula.stream/socket"), "https://arugula.stream");
  assert.equal(httpOrigin("ws://127.0.0.1:4130/socket"), "http://127.0.0.1:4130");
  assert.equal(subOf(token({ sub: "u_1", app: "demo" })), "u_1");
  assert.equal(subOf("not a token"), "");
});

test("a room hears members, waves, answers and the connection as events", async () => {
  const t = fake();
  const room = new Room({ room: "r", token: token({ sub: "me" }), relay: "wss://relay.example/socket", transport: t });
  assert.equal(room.me, "me");
  assert.equal(assets.base, "https://relay.example/", "models from the relay by default");
  assert.equal(room.client.mode, "avatar", "avatar where nothing's kept");
  const seen = [];
  room.on("members", (m) => seen.push(["members", m.map((x) => x.sub)]));
  room.on("wave", (from) => seen.push(["wave", from]));
  room.on("answer", (from, yes) => seen.push(["answer", from, yes]));
  room.on("live", (s) => seen.push(["live", s]));
  t.on.live("on");
  t.on.members([{ sub: "me", name: "Me", look: null, mode: "none", away: false, watching: true }, { sub: "b", name: "B", look: null, mode: "avatar", away: false, watching: true }]);
  t.on.wave("b");
  t.on.answer("b", true);
  assert.deepEqual(seen, [["live", "on"], ["members", ["me", "b"]], ["wave", "b"], ["answer", "b", true]]);
  room.wave("b");
  assert.equal(t.waved, "b");
  room.leave();
});

test("connect resolves when the relay lets you in, and rejects when it refuses", async () => {
  const t = fake();
  const p = connect({ room: "r", token: async () => token({ sub: "x" }), transport: t });
  t.on.live("on");
  const room = await p;
  assert.ok(room instanceof Room);
  room.leave();
  const u = fake();
  const q = connect({ room: "r", token: "t", transport: u });
  u.on.live("refused");
  await assert.rejects(q, /refused/);
});
