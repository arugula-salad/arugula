// A board drawn as its workers (#466): which panes work on a board's rows
// and in which lane, joined by an agent's `on_issue` or by a row's agent-block
// attachment. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { boardFocus, rowChip } from "../src/shell/swarm/board-focus.ts";
import { clusterOrder } from "../src/shared/boards/text.ts";

const B = "brd_0123456789abcdef0123456789abcdef";
const row = (id, lane, extra = {}) => ({ id, lane, pointer: { kind: "none" }, note: "", attachments: [], ...extra });
const view = (zones) => ({
  board: { id: B, title: "Launch" },
  zones: zones.map(([title, rows]) => ({ id: `lane_${title.toLowerCase()}`, title, rows })),
});
const pane = (host, id, row) => ({ key: `${host}:${id}`, host, id, on_issue: row ? { issue: "#1", boards: [{ project: "prj", board: B, title: "Launch", row }] } : null });
const hostOf = (m) => ({ m_ws: "workstation", m_b1: "build-01" })[m];
const attach = (machine, pane, extra = {}) => ({ id: "att_1", role: "work", pointer: { kind: "block", machine, pane }, ...extra });

const lanes = [
  ["Next", [row("row_a", "next")]],
  ["Doing", [row("row_b", "doing"), row("row_c", "doing")]],
  ["Review", []],
  ["Done", [row("row_d", "done")]],
];

test("an agent whose on_issue names the board and a row is a worker in that row's lane", () => {
  const f = boardFocus(view(lanes), [pane("workstation", 1, "row_b"), pane("build-01", 2, "row_a"), pane("build-01", 3, null)], hostOf);
  assert.equal(f.placed.get("workstation:1").name, "Doing");
  assert.equal(f.placed.get("build-01:2").name, "Next");
  assert.equal(f.placed.has("build-01:3"), false);
});

test("an agent only a row's attachment points at is a worker too, with the row's lane", () => {
  const z = [["Next", []], ["Doing", [row("row_b", "doing", { attachments: [attach("m_b1", "%7")] })]]];
  const f = boardFocus(view(z), [pane("build-01", 7, null), pane("build-01", 8, null)], hostOf);
  assert.deepEqual([...f.placed.keys()], ["build-01:7"]);
  assert.equal(f.placed.get("build-01:7").name, "Doing");
  // The pane number may come without its %.
  const g = boardFocus(view([["Doing", [row("row_b", "doing", { attachments: [attach("m_b1", "7")] })]]]), [pane("build-01", 7, null)], hostOf);
  assert.equal(g.placed.get("build-01:7").name, "Doing");
});

test("when both say where, the attachment wins, and a pane is never in two lanes", () => {
  const z = [["Next", [row("row_a", "next")]], ["Review", [row("row_r", "review", { attachments: [attach("m_ws", "4")] })]]];
  const f = boardFocus(view(z), [pane("workstation", 4, "row_a")], hostOf);
  assert.equal(f.placed.get("workstation:4").name, "Review");
  assert.equal(f.placed.get("workstation:4").row.id, "row_r");
  assert.deepEqual(f.lanes.map((l) => [l.name, l.keys]), [["Review", ["workstation:4"]], ["Next", []]]);
});

test("a released or proof attachment, an unknown machine, or another board's row places no one", () => {
  const z = [["Doing", [row("row_b", "doing", { attachments: [attach("m_ws", "1", { released: true }), attach("m_ws", "2", { role: "proof" }), attach("m_zz", "3")] })]]];
  const other = { key: "workstation:5", host: "workstation", id: 5, on_issue: { issue: "#2", boards: [{ project: "p", board: "brd_other", title: "Later", row: "row_b" }] } };
  const f = boardFocus(view(z), [pane("workstation", 1, null), pane("workstation", 2, null), pane("workstation", 3, null), other], hostOf);
  assert.equal(f.placed.size, 0);
});

test("lanes come in Doing, Review, Next, Done order (#479) with their counts; one with rows and no worker says so, one without rows is left out", () => {
  const f = boardFocus(view(lanes), [pane("workstation", 1, "row_b"), pane("build-01", 2, "row_c")], hostOf);
  assert.deepEqual(f.lanes.map((l) => l.label), ["Doing · 2 rows · 2 workers", "Next · 1 row · no one on it", "Done · 1 row · no one on it"]);
  const one = boardFocus(view(lanes), [pane("workstation", 1, "row_a")], hostOf);
  assert.equal(one.lanes[1].label, "Next · 1 row · 1 worker");
});

test("a row's chip is its issue and a short title, or its note", () => {
  const live = { state: "open", kind: "native", number: 12, title: "Fix the installer" };
  assert.equal(rowChip({ pointer: { kind: "issue", id: "iss_1" }, live, note: "" }), "#12 · Fix the installer");
  const long = { ...live, title: "Make the installer ask for nothing it doesn't need" };
  assert.equal(rowChip({ pointer: { kind: "issue", id: "iss_1" }, live: long, note: "" }), "#12 · Make the installer ask…");
  assert.equal(rowChip({ pointer: { kind: "none" }, note: "\nTalk to Jake\nmore" }), "Talk to Jake");
  assert.equal(rowChip({ pointer: { kind: "none" }, note: "" }), "a row");
});

test("clusters follow the lanes' order when given one, else the biggest first", () => {
  const sort = (c, order) => [...c].sort((a, b) => clusterOrder(a, b, order)).map((x) => x[0]);
  const c = [["Review", 3], ["Doing", 1], ["Next", 2]];
  assert.deepEqual(sort(c), ["Review", "Next", "Doing"]);
  assert.deepEqual(sort(c, ["Next", "Doing", "Review"]), ["Next", "Doing", "Review"]);
  assert.deepEqual(sort([...c, ["No board", 9]], ["Next", "Doing", "Review"]), ["Next", "Doing", "Review", "No board"]);
});
