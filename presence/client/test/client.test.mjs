// The presence client over a fake transport: hearing runs while a view
// watches, rigs and outlines land where views read them (a rig tells
// listeners only when someone starts or changes look), `off` forgets, and
// the Phoenix transport's pieces (a relayed frame's sender, a member from
// their metas) read what the relay sends. Run with `node
// --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { PresenceClient, memberOf, unprefix } from "../src/index.ts";
import { neutral, packRig } from "@arugula/presence-core/avatar/rig";
import { encodeFrame } from "@arugula/presence-core/outline/frame";

function fake() {
  const t = { on: null, listening: false, published: [], offs: 0, states: [] };
  t.transport = {
    listen(on) {
      t.on = on;
      t.listening = true;
      on.live("on");
    },
    unlisten() {
      t.listening = false;
    },
    publish(b) {
      t.published.push(b);
    },
    off() {
      t.offs++;
    },
    state(s) {
      t.states.push(s);
    },
  };
  return t;
}

const rigBytes = (look) => {
  const b = packRig(Object.assign(neutral(), { face: true, smile: 0.5 }), look);
  const wire = new Uint8Array(b.length + 1);
  wire.set(b, 1);
  return wire;
};

test("hearing runs while a view watches, and stops with the last", () => {
  const t = fake();
  const c = new PresenceClient({ transport: t.transport, storage: "test." });
  const a = c.watch();
  const b = c.watch();
  assert.equal(t.listening, true);
  assert.equal(c.live, "on");
  a();
  assert.equal(t.listening, true, "one view still watches");
  b();
  assert.equal(t.listening, false);
  assert.equal(c.live, "off");
});

test("a rig lands with its look; listeners hear of someone new or a new look, not every frame", async () => {
  const t = fake();
  const c = new PresenceClient({ transport: t.transport, storage: "test." });
  let told = 0;
  c.subscribe(() => told++);
  const stop = c.watch();
  told = 0;
  t.on.frame("sam", rigBytes({ character: 3, colour: 1 }));
  await Promise.resolve();
  assert.equal(c.of("sam")?.rig?.smile > 0.4, true);
  assert.deepEqual(c.lookOf("sam"), { character: 3, colour: 1 });
  assert.equal(told, 1);
  t.on.frame("sam", rigBytes({ character: 3, colour: 1 }));
  t.on.frame("sam", rigBytes({ character: 3, colour: 1 }));
  assert.equal(told, 1, "the same look again: no news");
  t.on.frame("sam", rigBytes({ character: 1, colour: 1 }));
  assert.equal(told, 2, "a new look: news");
  t.on.off("sam");
  assert.equal(c.of("sam"), null);
  assert.deepEqual(c.lookOf("sam"), { character: 1, colour: 1 }, "the look stays, for idling");
  stop();
});

test("an outline frame lands decoded, and an unreadable frame is skipped", async () => {
  const t = fake();
  const c = new PresenceClient({ transport: t.transport, storage: "test." });
  const stop = c.watch();
  t.on.frame("riley", await encodeFrame({ w: 8, h: 8, lines: [[0, 0, 7, 7]] }));
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(c.of("riley")?.frame?.lines, [[0, 0, 7, 7]]);
  t.on.frame("riley", Uint8Array.from([0, 99, 1, 2]));
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(c.of("riley")?.frame?.lines, [[0, 0, 7, 7]], "kept the last good one");
  stop();
});

test("waves are kept until answered, and the look goes to the transport", () => {
  const t = fake();
  const c = new PresenceClient({ transport: t.transport, storage: "test." });
  const stop = c.watch();
  t.on.wave("sam");
  assert.deepEqual(c.waves.map((k) => k.from), ["sam"]);
  c.answer("sam", true);
  assert.deepEqual(c.waves, []);
  c.setLook({ character: 1, colour: 4 });
  assert.deepEqual(t.states.at(-1), { look: { character: 1, colour: 4 } });
  stop();
});

test("the relay's frames say who sent them; a member is on camera if any page is", () => {
  const sub = new TextEncoder().encode("jake@x");
  const b = new Uint8Array(1 + sub.length + 3);
  b[0] = sub.length;
  b.set(sub, 1);
  b.set([0, 0x40, 1], 1 + sub.length);
  const { sub: who, frame } = unprefix(b);
  assert.equal(who, "jake@x");
  assert.deepEqual([...frame], [0, 0x40, 1]);
  assert.throws(() => unprefix(Uint8Array.from([9, 1])), /cut short/);
  const m = memberOf("jake", [
    { name: "Jake", mode: "none", away: true, watching: false },
    { name: "Jake", mode: "avatar", look: { character: 2, colour: 0 }, away: false, watching: true },
  ]);
  assert.deepEqual(m, { sub: "jake", name: "Jake", look: { character: 2, colour: 0 }, mode: "avatar", away: false, watching: true });
});

test("messages: history and said merge by id in order, say adds yours, loadEarlier pages back", async () => {
  const t = fake();
  const kept = [];
  let next = 100;
  t.transport.say = async (text) => {
    if (text.length > 5) throw new Error("too long");
    const m = { id: next++, sub: "me", name: "Me", text, at: 1 };
    kept.push(m);
    return m;
  };
  t.transport.history = async (before) => Array.from({ length: 3 }, (_, i) => ({ id: before - 3 + i, sub: "x", name: "X", text: `old${i}`, at: 0 }));
  const c = new PresenceClient({ transport: t.transport, storage: "t5." });
  const stop = c.watch();
  const page = Array.from({ length: 50 }, (_, i) => ({ id: 40 + i, sub: "x", name: "X", text: `m${i}`, at: 0 }));
  t.on.history(page);
  t.on.said({ id: 95, sub: "y", name: "Y", text: "hey", at: 0 });
  t.on.history(page); // a reconnect: no duplicates
  assert.equal(c.messages.length, 51);
  assert.deepEqual(c.messages.at(-1).text, "hey");
  const mine = await c.say("hi");
  assert.equal(c.messages.at(-1).id, mine.id);
  await assert.rejects(c.say("way too long"), /too long/);
  assert.equal(c.hasEarlier, true);
  await c.loadEarlier();
  assert.deepEqual(c.messages.slice(0, 3).map((m) => m.text), ["old0", "old1", "old2"]);
  assert.equal(c.hasEarlier, false, "a short page means that's all");
  stop();
});
