// The chat sidebar's arrangement (#120): a room is in one place, sections
// come and go, and groups sort. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  addSection,
  collapse,
  emptySidebar,
  inScope,
  ordered,
  place,
  placed,
  placeOf,
  present,
  removeSection,
  renameSection,
  setSort,
  sortOf,
  star,
  STARRED,
} from "../src/shared/chat/sidebar.ts";

const room = (id, name, last = 0, archived = false) => ({ id, name, last, archived });
const label = (r) => r.name;

test("a room is in one place at a time", () => {
  let s = addSection(emptySidebar(), " Ops ", "s1");
  assert.equal(s.sections[0].name, "Ops");
  s = star(s, "a", true);
  assert.equal(placeOf(s, "a"), STARRED);
  s = place(s, "a", "s1");
  assert.deepEqual([s.starred, s.sections[0].rooms], [[], ["a"]]);
  s = star(s, "a", true);
  assert.deepEqual([s.starred, s.sections[0].rooms], [["a"], []]);
  s = star(s, "a", false);
  assert.equal(placeOf(s, "a"), null);
  // Unstarring a room that is in a section leaves it there.
  s = place(s, "b", "s1");
  assert.equal(placeOf(star(s, "b", false), "b"), "s1");
  // A section that is not there takes nothing.
  assert.equal(place(s, "c", "nope"), s);
  assert.deepEqual([...placed(s)], ["b"]);
});

test("removing a section frees its rooms and forgets its state", () => {
  let s = place(addSection(emptySidebar(), "Ops", "s1"), "a", "s1");
  s = setSort(collapse(s, "s1", true), "s1", "recent");
  assert.deepEqual([s.collapsed, s.sorts], [["s1"], { s1: "recent" }]);
  s = removeSection(s, "s1");
  assert.deepEqual(s, emptySidebar());
  assert.equal(renameSection(addSection(emptySidebar(), "A", "x"), "x", " B ").sections[0].name, "B");
});

test("collapse and sort are kept once, and name is the default", () => {
  let s = collapse(collapse(emptySidebar(), STARRED, true), STARRED, true);
  assert.deepEqual(s.collapsed, [STARRED]);
  assert.deepEqual(collapse(s, STARRED, false).collapsed, []);
  s = setSort(s, "t/dms", "recent");
  assert.equal(sortOf(s, "t/dms"), "recent");
  assert.equal(sortOf(s, "t/channels"), "name");
  assert.equal(sortOf(s, "t/dms", "recent"), "recent");
  assert.deepEqual(setSort(s, "t/dms", "name").sorts, {});
});

test("rooms sort by name or by activity, archived last, and unseen ones are skipped", () => {
  const rs = [room("1", "ops", 5), room("2", "alpha", 9), room("3", "zed", 1), room("4", "old", 99, true)];
  assert.deepEqual(ordered(rs, "name", label).map((r) => r.id), ["2", "1", "3", "4"]);
  assert.deepEqual(ordered(rs, "recent", label).map((r) => r.id), ["2", "1", "3", "4"]);
  assert.deepEqual(ordered([room("a", "x", 1), room("b", "y", 7)], "recent", label).map((r) => r.id), ["b", "a"]);
  assert.deepEqual(present(["3", "gone", "1"], rs).map((r) => r.id), ["3", "1"]);
});

test("a project's page lists its team's rooms, and Personal the rooms of no team (UI audit #169)", () => {
  const ch = { id: "c", team: "t1" };
  const other = { id: "o", team: "t2" };
  const pane = { id: "p", team: null };
  const acme = { team: "t1", name: "Acme" };
  const own = { team: null, name: "Personal" };
  assert.deepEqual([ch, other, pane].filter((r) => inScope(r, acme)).map((r) => r.id), ["c"]);
  assert.deepEqual([ch, other, pane].filter((r) => inScope(r, own)).map((r) => r.id), ["p"]);
  assert.deepEqual([ch, other, pane].filter((r) => inScope(r)).map((r) => r.id), ["c", "o", "p"]);
});
