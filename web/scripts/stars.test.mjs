// Starred tabs and panes (#854): the Sessions sidebar's and the phone's
// Starred group lists them, oldest star first, from the tabs and panes
// this client sees (a guest gets every tab's options), and hides when
// nothing is starred. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { starIntent, starOf, starredList } from "../src/shell/stars.ts";

const leaf = (pane) => ({ type: "pane", pane });
const split = (...panes) => ({ type: "split", id: 100, dir: "h", children: panes.map((p) => ({ node: leaf(p), weight: 1 })) });
const tab = (id, root) => ({ id, name: null, root, cols: 80, rows: 24, owner: null, zoom: null });
const state = (options) => ({
  tabs: [tab(1, split(1, 2)), tab(2, leaf(3))],
  panes: [1, 2, 3].map((id) => ({ id })),
  options,
});

test("oldest star first, tabs and panes together", () => {
  const s = state({ tabs: [[2, { star: "300" }], [1, { star: "100" }]], panes: [[3, { star: "200" }]] });
  assert.deepEqual(starredList(s), [
    { kind: "tab", id: 1, at: 100 },
    { kind: "pane", id: 3, at: 200 },
    { kind: "tab", id: 2, at: 300 },
  ]);
});

test("a tab or pane this client can't see is left out", () => {
  // Tab 9 and pane 7 are another session's, which this guest doesn't see.
  const s = state({ tabs: [[9, { star: "1" }]], panes: [[7, { star: "2" }], [2, { star: "3" }]] });
  assert.deepEqual(starredList(s), [{ kind: "pane", id: 2, at: 3 }]);
  // A pane in the layout the client has no info for yet isn't listed.
  assert.deepEqual(starredList({ ...s, panes: [{ id: 1 }] }), []);
});

test("nothing starred is an empty list, which hides the group", () => {
  assert.deepEqual(starredList(state({})), []);
  assert.deepEqual(starredList(state({ tabs: [[1, { other: "x" }]] })), []);
  assert.deepEqual(starredList(null), []);
});

test("a star is set_option's `star`, the time it was starred", () => {
  assert.deepEqual(starIntent("pane", 4, true, 1234), { op: "set_option", scope: { kind: "pane", id: 4 }, name: "star", value: "1234" });
  assert.deepEqual(starIntent("tab", 2, false), { op: "set_option", scope: { kind: "tab", id: 2 }, name: "star", value: null });
  assert.equal(starOf({ panes: [[4, { star: "1234" }]] }, "pane", 4), 1234);
  assert.equal(starOf({ panes: [[4, { star: "1234" }]] }, "tab", 4), null);
});
