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

test("a room's messages: history and said fire message once each, and say sends", async () => {
  const t = fake();
  t.say = async (text) => ({ id: 9, sub: "me", name: "Me", text, at: 3 });
  const room = new Room({ room: "r", token: token({ sub: "me" }), relay: "wss://relay.example/socket", transport: t });
  const got = [];
  room.on("message", (m) => got.push(m.text));
  t.on.live("on");
  t.on.history([{ id: 1, sub: "b", name: "B", text: "hi", at: 1 }]);
  t.on.history([{ id: 1, sub: "b", name: "B", text: "hi", at: 1 }]);
  t.on.said({ id: 2, sub: "b", name: "B", text: "you there?", at: 2 });
  await room.say("yes");
  assert.deepEqual(got, ["hi", "you there?", "yes"]);
  assert.deepEqual(room.messages.map((m) => m.id), [1, 2, 9]);
  room.leave();
});

test("with a key: a member's token or a guest's name; your sub comes on joining; one or the other is needed", () => {
  const member = new Room({ room: "acme:standup", key: "pk_live_abc", auth: async () => "eyJ.a.b", transport: fake() });
  assert.equal(member.me, "", "not in a provider token: the relay says it on joining");
  member.leave();
  const guest = new Room({ room: "lobby", key: "pk_live_abc", guest: "Ada", transport: fake() });
  guest.leave();
  assert.throws(() => new Room({ room: "lobby", transport: fake() }), /needs a token, or a key/);
});

test("a room's calls: whether the relay has them, rings and the call as events, call(sub) rings", async () => {
  const t = fake();
  const rung = [];
  t.call = async (to) => (rung.push(to), { id: "c1" });
  t.callAnswer = async () => {};
  t.callLeave = (id) => rung.push(`left ${id}`);
  const room = new Room({ room: "r", token: token({ sub: "me" }), relay: "wss://relay.example/socket", transport: t });
  const seen = [];
  room.on("ring", (from, id) => seen.push(["ring", from, id]));
  room.on("call", (c) => seen.push(["call", c && c.state]));
  t.on.live("on");
  assert.equal(room.calls, false);
  t.on.calls(true);
  assert.equal(room.calls, true);
  t.on.call("b", "c7");
  t.on.call("b", "c7");
  assert.equal(await room.call("b"), "c1");
  assert.deepEqual(rung, ["b"]);
  // Video waits for the call to start: a ring has none.
  await room.setVideo(true);
  assert.equal(room.inCall.video, false);
  assert.equal(room.client.videoOf("b"), false);
  room.leaveCall();
  assert.deepEqual(rung, ["b", "left c1"]);
  assert.deepEqual(seen, [["ring", "b", "c7"], ["call", "ringing"], ["call", null]]);
  room.leave();
});
