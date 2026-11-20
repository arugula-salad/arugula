// Pinned sessions (Arugula iss_c0fd27ac196647fa7c857dc3659a0098): the
// sidebar's groups, Needs you over Pinned, pin order, gone sessions and
// the Panes page's section. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { MAX_PINS, isPinned, pin, pinnedOn, pinnedPanes, sessionGroups, stateOf, tidy, unpin } from "../src/shared/session-pins.ts";

const M = "m_laptop";
const row = (id, group) => ({ id, group });
const groups = (rows, pinned) => Object.fromEntries(sessionGroups(rows, pinned, (r) => r).map((g) => [g.id, g.rows.map((r) => r.id)]));

test("the sidebar's groups: Needs you, Pinned, Working, Idle", () => {
  const rows = [row(1, "idle"), row(2, "working"), row(3, "needs"), row(4, "idle"), row(5, "working")];
  const out = sessionGroups(rows, [4, 2], (r) => r);
  assert.deepEqual(
    out.map((g) => [g.id, g.rows.map((r) => r.id)]),
    [
      ["needs", [3]],
      ["pinned", [4, 2]],
      ["working", [5]],
      ["idle", [1]],
    ],
  );
});

test("a pinned session that needs you is only in Needs you, and back in Pinned once answered", () => {
  assert.deepEqual(groups([row(1, "needs"), row(2, "idle")], [1, 2]), { needs: [1], pinned: [2], working: [], idle: [] });
  assert.deepEqual(groups([row(1, "idle"), row(2, "idle")], [1, 2]), { needs: [], pinned: [1, 2], working: [], idle: [] });
});

test("pins keep the order they were pinned in, oldest first", () => {
  let p = pin([], M, 7);
  p = pin(p, M, 3);
  p = pin(p, "m_other", 1);
  p = pin(p, M, 9);
  assert.deepEqual(pinnedOn(p, M), [7, 3, 9]);
  // Pinned again: where it was.
  assert.equal(pin(p, M, 3), p);
  p = unpin(p, M, 3);
  assert.deepEqual(pinnedOn(p, M), [7, 9]);
  assert.equal(isPinned(p, "m_other", 1), true);
  assert.equal(isPinned(p, M, 1), false);
  // The rows come in the sidebar's order, not the pins'.
  assert.deepEqual(groups([row(9, "idle"), row(7, "working")], pinnedOn(p, M)).pinned, [7, 9]);
});

test("a pin on a gone session isn't drawn, and is dropped when the list is written", () => {
  const p = [
    { machine: M, session: 1 },
    { machine: M, session: 2 },
    { machine: "m_other", session: 2 },
  ];
  assert.deepEqual(groups([row(2, "idle")], pinnedOn(p, M)), { needs: [], pinned: [2], working: [], idle: [] });
  assert.deepEqual(tidy(p, M, [2, 5]), [
    { machine: M, session: 2 },
    { machine: "m_other", session: 2 },
  ]);
  assert.equal(tidy(p, M, [1, 2]), p);
});

test("no pins: no Pinned section", () => {
  const out = groups([row(1, "idle"), row(2, "needs")], []);
  assert.deepEqual(out.pinned, []);
  assert.deepEqual(pinnedOn([], M), []);
  // A page that knows no machine has none.
  assert.deepEqual(pinnedOn([{ machine: M, session: 1 }], undefined), []);
});

test("at most MAX_PINS", () => {
  let p = [];
  for (let i = 0; i < MAX_PINS + 5; i++) p = pin(p, M, i);
  assert.equal(p.length, MAX_PINS);
  assert.equal(isPinned(p, M, MAX_PINS), false);
});

test("the Panes page: pinned sessions first, in pin order, and not again under their machine", () => {
  const pane = (id, session, attention = "idle") => ({ id, session, attention });
  const laptop = { id: M, name: "laptop", panes: [pane(1, 1), pane(2, 2, "needs_input"), pane(3, 2)] };
  const box = { id: "m_box", name: "box", panes: [pane(1, 4, "working")] };
  const pins = [
    { machine: "m_box", session: 4 },
    { machine: M, session: 2 },
    // Gone, or not in what the page shows: not there.
    { machine: M, session: 8 },
    { machine: "m_gone", session: 1 },
  ];
  const out = pinnedPanes([laptop, box], pins, (g) => g.id, (p) => p.session);
  assert.deepEqual(
    out.pinned.map((x) => [x.pin.machine, x.pin.session, x.group.name, x.panes.map((p) => p.id)]),
    [
      ["m_box", 4, "box", [1]],
      [M, 2, "laptop", [2, 3]],
    ],
  );
  // The box's only session is pinned: its section goes.
  assert.deepEqual(
    out.groups.map((g) => [g.name, g.panes.map((p) => p.id)]),
    [["laptop", [1]]],
  );
  assert.equal(stateOf(out.pinned[1].panes, (p) => p.attention === "needs_input", (p) => p.attention === "working"), "needs_input");
  assert.equal(stateOf(out.pinned[0].panes, (p) => p.attention === "needs_input", (p) => p.attention === "working"), "working");
  // No pins: the groups as they were.
  assert.deepEqual(pinnedPanes([laptop, box], [], (g) => g.id, (p) => p.session), { pinned: [], groups: [laptop, box] });
});
