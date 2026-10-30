// The live presence store (shared/live/store.ts, Arugula #343) with a fake
// SDK and token fetch: it's in every team's room while the switch says
// Watching or Live and in none at Off; live() follows members' cameras;
// a refused token and an unreachable relay say so, and Try again rejoins;
// Live's camera goes to one room (the working team's) and stops while the
// window is hidden; your call or voice video keeps its camera, and a
// camera Live didn't start is left alone; a held room (a People block) stays at Off. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { LiveStore, Refused } from "../src/shared/live/store.ts";

const settle = () => new Promise((r) => setTimeout(r, 0));
const until = async (f) => {
  for (let i = 0; i < 50 && !f(); i++) await settle();
  assert.ok(f(), "never happened");
};

/** A fake arugula.stream: rooms by name, each with members we can change. */
function fakeSdk() {
  const rooms = new Map();
  const sdk = {
    rooms,
    joins: 0,
    async connect({ room, token }) {
      sdk.joins++;
      const t = await token();
      if (t === "bad") throw new Refused("refused");
      const handlers = { members: new Set(), live: new Set(), voice: new Set(), call: new Set() };
      const r = {
        name: room,
        token: t,
        left: false,
        members: [],
        voice: { joined: null },
        inCall: null,
        on(ev, fn) {
          handlers[ev].add(fn);
          return () => handlers[ev].delete(fn);
        },
        camera: {
          on: false,
          async start() {
            this.on = true;
          },
          stop() {
            this.on = false;
            // As arugula.stream does: no camera, no video.
            if (r.voice.joined) r.voice = { joined: { ...r.voice.joined, video: false } };
            if (r.inCall) r.inCall = { ...r.inCall, video: false };
          },
        },
        avatar: () => () => {},
        leave() {
          r.left = true;
        },
        set(members) {
          r.members = members;
          for (const f of handlers.members) f(members);
        },
        say(s) {
          for (const f of handlers.live) f(s);
        },
        /** You in voice with video (its camera on), or out of it. */
        video(on) {
          r.voice = { joined: on ? { video: true } : null };
          if (on) r.camera.on = true;
          for (const f of handlers.voice) f([], r.voice.joined);
        },
      };
      rooms.set(room, r);
      return r;
    },
  };
  return sdk;
}

function rig({ state = "watching", tokens = {} } = {}) {
  const sdk = fakeSdk();
  let s = state;
  const subs = new Set();
  const store = new LiveStore({
    state: () => s,
    subscribe: (fn) => (subs.add(fn), () => subs.delete(fn)),
    token: async (team) => {
      const t = tokens[team];
      if (t instanceof Error) throw t;
      return { token: t ?? `tok-${team}`, relay: "wss://relay.test/socket" };
    },
    sdk: async () => sdk,
  });
  const set = (next) => {
    s = next;
    for (const f of subs) f();
  };
  return { sdk, store, set };
}

test("Watching joins every team's room; Off leaves them all", async () => {
  const { sdk, store, set } = rig();
  store.setTeams(["t2", "t1", "t1"]);
  await until(() => store.states().t1 === "on" && store.states().t2 === "on");
  assert.deepEqual([...sdk.rooms.keys()].sort(), ["team:t1", "team:t2"]);
  assert.equal(sdk.rooms.get("team:t1").token, "tok-t1", "the first token is the one fetched");
  set("off");
  assert.deepEqual(store.states(), {});
  assert.ok(sdk.rooms.get("team:t1").left && sdk.rooms.get("team:t2").left);
  set(null);
  assert.deepEqual(store.states(), {}, "flag off: nothing");
});

test("live() is a member with their camera on and not away", async () => {
  const { sdk, store } = rig();
  store.setTeams(["t1"]);
  await until(() => store.states().t1 === "on");
  const r = sdk.rooms.get("team:t1");
  assert.equal(store.live("ada"), null);
  let told = 0;
  store.subscribe(() => told++);
  r.set([{ sub: "ada", name: "Ada", mode: "avatar", away: false }]);
  assert.ok(told > 0, "members changing tells the page");
  assert.equal(store.live("ada").member.name, "Ada");
  r.set([{ sub: "ada", name: "Ada", mode: "outline", away: true }]);
  assert.equal(store.live("ada"), null, "away");
  r.set([{ sub: "ada", name: "Ada", mode: "none", away: false }]);
  assert.equal(store.live("ada"), null, "camera off");
});

test("a refused token and an unreachable control say so, and Try again rejoins", async () => {
  const { sdk, store } = rig({ tokens: { t1: new Refused("no such team"), t2: new Error("Control said 502") } });
  store.setTeams(["t1", "t2"]);
  await until(() => store.room("t1")?.state === "refused" && store.room("t2")?.state === "unreachable");
  assert.equal(store.room("t1").why, "no such team");
  assert.equal(sdk.joins, 0);
  store.retry("t2");
  assert.equal(store.room("t2").state, "connecting");
});

test("the relay's live state shows on the room", async () => {
  const { sdk, store } = rig();
  store.setTeams(["t1"]);
  await until(() => store.states().t1 === "on");
  sdk.rooms.get("team:t1").say("connecting");
  assert.equal(store.room("t1").state, "connecting");
  sdk.rooms.get("team:t1").say("refused");
  assert.equal(store.room("t1").state, "refused");
});

test("Live's camera goes to the working team's room only, and stops while hidden", async () => {
  const { sdk, store, set } = rig({ state: "live" });
  store.setTeams(["t1", "t2"]);
  await until(() => store.states().t1 === "on" && store.states().t2 === "on");
  const cam = (t) => sdk.rooms.get(`team:${t}`).camera.on;
  assert.deepEqual([cam("t1"), cam("t2")], [true, false], "the first, with no working team");
  store.setWorking("t2");
  assert.deepEqual([cam("t1"), cam("t2")], [false, true]);
  store.setHidden(true);
  assert.deepEqual([cam("t1"), cam("t2")], [false, false]);
  store.setHidden(false);
  assert.equal(cam("t2"), true);
  set("watching");
  assert.deepEqual([cam("t1"), cam("t2")], [false, false], "Watching sends nothing");
});

test("a held room stays open at Off, and goes when its last hold does", async () => {
  const { sdk, store } = rig({ state: "off" });
  store.setTeams(["t1", "t2"]);
  const a = store.hold("t1");
  const b = store.hold("t1");
  await until(() => store.states().t1 === "on");
  assert.deepEqual(Object.keys(store.states()), ["t1"]);
  a();
  a();
  assert.equal(store.states().t1, "on", "one hold left");
  b();
  assert.deepEqual(store.states(), {});
  assert.ok(sdk.rooms.get("team:t1").left);
});

test("video in voice keeps its camera whatever Live, the window or the working team do", async () => {
  const { sdk, store, set } = rig({ state: "live" });
  store.setTeams(["t1", "t2"]);
  await until(() => store.states().t1 === "on" && store.states().t2 === "on");
  const t1 = sdk.rooms.get("team:t1");
  const t2 = sdk.rooms.get("team:t2");
  t1.video(true);
  for (const go of [() => store.setHidden(true), () => store.setWorking("t2"), () => set("watching")]) {
    go();
    assert.ok(t1.camera.on && t1.voice.joined.video, "still on video");
    assert.equal(t2.camera.on, false, "one camera: none for Live elsewhere");
  }
});

test("video in another room takes Live's camera there off, and Live comes back after", async () => {
  const { sdk, store } = rig({ state: "live" });
  store.setTeams(["t1", "t2"]);
  await until(() => store.states().t1 === "on" && store.states().t2 === "on");
  const t1 = sdk.rooms.get("team:t1");
  const t2 = sdk.rooms.get("team:t2");
  assert.deepEqual([t1.camera.on, t2.camera.on], [true, false]);
  t2.video(true);
  assert.deepEqual([t1.camera.on, t2.camera.on], [false, true]);
  t2.camera.on = false;
  t2.video(false);
  assert.deepEqual([t1.camera.on, t2.camera.on], [true, false]);
});

test("a camera the store didn't start (a People block's picker) stays on at Off and Watching", async () => {
  const { sdk, store, set } = rig({ state: "watching" });
  store.setTeams(["t1"]);
  store.hold("t1");
  await until(() => store.states().t1 === "on");
  const t1 = sdk.rooms.get("team:t1");
  t1.camera.on = true;
  store.setHidden(true);
  store.setWorking("t1");
  set("off");
  assert.equal(t1.camera.on, true);
});

test("Live's camera still starting when Live moves goes off once it's on", async () => {
  const { sdk, store } = rig({ state: "live" });
  store.setHidden(true);
  store.setTeams(["t1"]);
  await until(() => store.states().t1 === "on");
  const t1 = sdk.rooms.get("team:t1");
  let go;
  t1.camera.start = () => new Promise((r) => (go = () => ((t1.camera.on = true), r())));
  store.setHidden(false);
  store.setHidden(true);
  go();
  await settle();
  assert.equal(t1.camera.on, false);
});
