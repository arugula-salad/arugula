// The swarm's plan board (#445, shell/swarm/plan-model.ts): an agent
// session's task list sorted into Pending, In progress, Review, Blocked and
// Done. Review holds what's in progress while the session waits on its
// person, with why; a waiting session with nothing in progress, or no list,
// is a Review card itself; a pending step waiting on another is blocked;
// sessions with neither a list nor a question are in the no-plan strip.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { boardOf, columnOf, DONE_KEPT, modelCol, MODEL_OTHER } from "../src/shell/swarm/plan-model.ts";

const step = (id, status, over = {}) => ({ id, text: `step ${id}`, active: `doing ${id}`, status, ...over });
const session = (key, steps, over = {}) => ({ key, plan: { steps, model: "Opus 5.5", agent: "Claude Code", title: `session ${key}`, waits: null, ...over } });
const cols = (b) => Object.fromEntries(Object.entries(b.columns).map(([c, cards]) => [c, cards.map((x) => x.key)]));

test("each status has its column, in progress in its active words", () => {
  const b = boardOf([session("a", [step("1", "completed"), step("2", "in_progress"), step("3", "pending")])]);
  assert.deepEqual(cols(b), { pending: ["a#3"], in_progress: ["a#2"], review: [], blocked: [], done: ["a#1"] });
  assert.equal(b.columns.in_progress[0].text, "doing 2");
  assert.equal(b.columns.pending[0].text, "step 3");
  assert.equal(b.columns.in_progress[0].agent, "Claude Code");
  assert.deepEqual(b.noPlan, []);
});

test("a session waiting on its person moves what's in progress to Review, saying why", () => {
  const b = boardOf([session("q", [step("1", "in_progress"), step("2", "pending")], { waits: "Where should search results be cached?" })]);
  assert.deepEqual(b.columns.review.map((c) => [c.key, c.why]), [["q#1", "Where should search results be cached?"]]);
  assert.deepEqual(b.columns.in_progress, []);
  // Pending steps stay pending.
  assert.deepEqual(b.columns.pending.map((c) => c.key), ["q#2"]);
});

test("a waiting session with no plan, or nothing in progress, is one Review card", () => {
  const b = boardOf([
    session("none", [], { waits: "Bash wants to run npm test" }),
    session("idle", [step("1", "pending")], { waits: "Bash wants to run make" }),
  ]);
  assert.deepEqual(b.columns.review.map((c) => [c.key, c.text, c.why]), [
    ["none#waits", "session none", "Bash wants to run npm test"],
    ["idle#waits", "session idle", "Bash wants to run make"],
  ]);
  assert.deepEqual(b.noPlan, []);
  assert.deepEqual(b.columns.pending.map((c) => c.key), ["idle#1"]);
});

test("a pending step waiting on an unfinished one is blocked, naming it", () => {
  const b = boardOf([session("m", [step("1", "in_progress"), step("2", "pending", { blocked_by: ["1"] }), step("3", "pending", { blocked_by: ["9"] })])]);
  assert.deepEqual(b.columns.blocked.map((c) => [c.key, c.after]), [["m#2", ["step 1"]], ["m#3", ["#9"]]]);
  assert.equal(columnOf(step("x", "pending", { blocked_by: [] }), false), "pending");
  // An in-progress step isn't blocked, whatever it lists.
  assert.equal(columnOf(step("x", "in_progress", { blocked_by: ["1"] }), false), "in_progress");
});

test("sessions without a list or a question are in the strip, in order", () => {
  const b = boardOf([session("b", []), session("a", [step("1", "pending")]), session("c", [])]);
  assert.deepEqual(b.noPlan, ["b", "c"]);
});

test("a long Done keeps a session's last steps, the first of them saying how many more", () => {
  const steps = Array.from({ length: DONE_KEPT + 4 }, (_, i) => step(String(i + 1), "completed"));
  const b = boardOf([session("d", steps)]);
  assert.equal(b.columns.done.length, DONE_KEPT);
  assert.equal(b.columns.done[0].key, `d#${4 + 1}`);
  assert.equal(b.columns.done[0].more, 4);
  // None hidden, none said.
  assert.equal(boardOf([session("e", [step("1", "completed")])]).columns.done[0].more, undefined);
});

test("a step given to a teammate says who; the model has its colour", () => {
  const b = boardOf([session("t", [step("1", "in_progress", { owner: "migrator" })])]);
  assert.equal(b.columns.in_progress[0].agent, "migrator");
  assert.notDeepEqual(modelCol("Opus 5.5"), modelCol("Sonnet 5.5"));
  assert.notDeepEqual(modelCol("Haiku 4.5"), MODEL_OTHER);
  assert.deepEqual(modelCol(null), MODEL_OTHER);
  assert.deepEqual(modelCol("gpt-5"), MODEL_OTHER);
});
