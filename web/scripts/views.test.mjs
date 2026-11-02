// A View's tree (#503): splitting, closing up, the limits a pref sets, and
// what a leaf asks of its tab's size. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  MAX_DOC_BYTES,
  MAX_LEAVES,
  MAX_NAME,
  addressOf,
  arrange,
  bestSpot,
  decode,
  encode,
  fits,
  insert,
  leaves,
  parseAddress,
  placeholder,
  remove,
  resize,
  sizeAsk,
  viewName,
  zoneAt,
  zoneBox,
} from "../src/shell/views/model.ts";

const box = { x: 0, y: 0, w: 1000, h: 600 };

test("an address keeps the dots in a machine's name", () => {
  assert.deepEqual(parseAddress("build-01.tailnet.7"), { machine: "build-01.tailnet", pane: 7 });
  assert.deepEqual(parseAddress(addressOf("a1b2c3d4", 12)), { machine: "a1b2c3d4", pane: 12 });
  for (const bad of ["", "nodot", ".7", "m.", "m.x", "m.1.5.-1"]) assert.equal(parseAddress(bad), null, bad);
});

test("a drop splits the leaf it lands on, on that side", () => {
  let t = insert(null, null, "center", "m.1");
  assert.equal(t, "m.1");
  t = insert(t, "m.1", "left", "m.2");
  assert.deepEqual(t, { d: "h", s: [0.5, 0.5], c: ["m.2", "m.1"] });
  t = insert(t, "m.1", "bottom", "m.3");
  assert.deepEqual(leaves(t), ["m.2", "m.1", "m.3"]);
  const placed = arrange(t, box).leaves;
  assert.deepEqual(placed.get("m.2"), { x: 0, y: 0, w: 500, h: 600 });
  assert.deepEqual(placed.get("m.1"), { x: 500, y: 0, w: 500, h: 300 });
  assert.deepEqual(placed.get("m.3"), { x: 500, y: 300, w: 500, h: 300 });
});

test("taking a leaf out closes its split up, and the last leaves nothing", () => {
  let t = insert(insert("m.1", "m.1", "right", "m.2"), "m.2", "bottom", "m.3");
  t = remove(t, "m.2");
  assert.deepEqual(t, { d: "h", s: [0.5, 0.5], c: ["m.1", "m.3"] });
  t = remove(t, "m.1");
  assert.equal(t, "m.3");
  assert.equal(remove(t, "m.3"), null);
  assert.equal(remove(t, "not-here.1"), "m.3");
});

test("taking one of three shares its room among the rest", () => {
  const t = { d: "h", s: [0.5, 0.25, 0.25], c: ["m.1", "m.2", "m.3"] };
  assert.deepEqual(remove(t, "m.1").s, [0.5, 0.5]);
});

test("a divider moves between its two children and leaves each some room", () => {
  const t = { d: "h", s: [0.5, 0.5], c: ["m.1", { d: "v", s: [0.5, 0.5], c: ["m.2", "m.3"] }] };
  const { dividers } = arrange(t, box);
  assert.equal(dividers.length, 2);
  assert.deepEqual(resize(t, [], 0, 0.1).s, [0.6, 0.4]);
  assert.deepEqual(resize(t, [], 0, 5).s, [0.88, 0.12]);
  assert.deepEqual(resize(t, [1], 0, -0.25).c[1].s, [0.25, 0.75]);
});

test("a fling lands on the biggest leaf, along its long side", () => {
  const t = { d: "h", s: [0.3, 0.7], c: ["m.1", "m.2"] };
  assert.deepEqual(bestSpot(t, box), { at: "m.2", edge: "bottom" });
  assert.deepEqual(bestSpot("m.1", box), { at: "m.1", edge: "right" });
  assert.equal(bestSpot(null, box), null);
});

test("the pointer picks the nearest edge, or the middle", () => {
  const t = "m.1";
  assert.equal(zoneAt(t, box, 20, 300, 0.3).edge, "left");
  assert.equal(zoneAt(t, box, 980, 300, 0.3).edge, "right");
  assert.equal(zoneAt(t, box, 500, 20, 0.3).edge, "top");
  assert.equal(zoneAt(t, box, 500, 300, 0.3).edge, "center");
  assert.equal(zoneAt(t, box, 1500, 300, 0.3), null, "outside");
  assert.equal(zoneAt(null, box, 500, 300, 0.3).at, null, "an empty view takes it whole");
  assert.deepEqual(zoneBox("right", box), { x: 500, y: 0, w: 500, h: 600 });
});

test("the most a View can hold fits a pref, with a full name and dragged dividers", () => {
  // Control's device ids are 16 hex characters.
  const addr = (i) => addressOf("a1b2c3d4e5f60718", 1000 + i);
  const dragged = (n) => (typeof n === "string" ? n : { ...n, s: [0.123, 0.877], c: n.c.map(dragged) });
  let t = addr(0);
  for (let i = 1; i < MAX_LEAVES; i++) t = insert(t, addr(i - 1), i % 2 ? "right" : "bottom", addr(i));
  t = dragged(t);
  const doc = { n: "x".repeat(MAX_NAME), t, at: 1793578746819 };
  const bytes = JSON.stringify(encode(doc)).length;
  assert.ok(bytes <= MAX_DOC_BYTES, `${bytes} bytes`);
  assert.ok(fits(doc));
  assert.equal(fits({ ...doc, t: insert(t, addr(0), "right", addr(99)) }), false, "one more leaf");
});

test("what's kept comes back the same, and only a real View decodes", () => {
  const good = { t: { d: "h", s: [0.5, 0.5], c: ["m.1", { d: "v", s: [0.25, 0.75], c: ["m.2", "m.3"] }] }, at: 5, n: "mine" };
  assert.deepEqual(decode(JSON.parse(JSON.stringify(encode(good)))), good);
  assert.deepEqual(decode(encode({ t: "m.1", at: 5 })), { t: "m.1", at: 5 });
  for (const bad of [null, 3, "m.1", {}, { t: "nope", at: 1 }, { t: "m.1" }, { t: ["x", [1, 1], "m.1", "m.2"], at: 1 }, { t: ["h", [1], "m.1", "m.2"], at: 1 }, { t: ["h", [1, 1], "m.1"], at: 1 }]) {
    assert.equal(decode(bad), null, JSON.stringify(bad));
  }
});

test("a View is named from what's in it until someone names it", () => {
  const title = (a) => `t${a}`;
  assert.equal(viewName(null, title), "New view");
  assert.equal(viewName({ t: "m.1" }, title), "tm.1");
  assert.equal(viewName({ t: { d: "h", s: [0.5, 0.5], c: ["m.1", "m.2"] } }, title), "tm.1 + 1 more");
  assert.equal(viewName({ n: "Deploy", t: "m.1" }, title), "Deploy");
});

test("a leaf takes a size only when no other screen is using it", () => {
  const base = { me: 9, ownPage: false, present: true, sharers: 1, viewer: false };
  assert.equal(sizeAsk({ ...base, owner: null }), "claim", "nobody's");
  assert.equal(sizeAsk({ ...base, owner: 4, present: false }), "claim", "its holder has gone");
  assert.equal(sizeAsk({ ...base, owner: 9 }), "keep", "already this leaf's");
  assert.equal(sizeAsk({ ...base, owner: 4, ownPage: true }), "ask", "this page's own main client");
  assert.equal(sizeAsk({ ...base, owner: 4 }), "none", "another screen's");
  assert.equal(sizeAsk({ ...base, owner: null, sharers: 2 }), "none", "two leaves of one tab");
  assert.equal(sizeAsk({ ...base, owner: null, viewer: true }), "none", "a viewer");
});

test("a leaf says why it can't show its pane", () => {
  const base = { machine: "build-01", pane: 41, known: true, connected: true, hasState: true, hasPane: false, shared: false, graceOver: true };
  assert.match(placeholder({ ...base, known: false }), /isn't in your list/);
  assert.match(placeholder({ ...base, connected: false }), /build-01 is unreachable/);
  assert.match(placeholder(base), /%41 has closed on build-01/);
  assert.match(placeholder({ ...base, shared: true }), /isn't shared with you/);
  assert.equal(placeholder({ ...base, graceOver: false }), null, "not yet seen: wait");
  assert.equal(placeholder({ ...base, hasState: false }), null, "no state yet");
  assert.equal(placeholder({ ...base, hasPane: true }), null, "it's there");
});
