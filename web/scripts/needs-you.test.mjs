// What "needs you" means (shared/needs-you, UI audit #154): the bell, the
// footer, the Sessions sidebar and the Swarm all count with it. An ask or
// a failure needs you; a plain finish is done; a remote pane's own machine
// counts it; a stopped agent isn't running. Every list of them shares one
// order (#451): asks (a gate first), failures, then what finished, each
// oldest first. Run with `node
// --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { agentRunning, byNeedsYou, finished, needsYou, needsYouRank, needYouText, placeOf, sortNeedsYou } from "../src/shared/needs-you.ts";

const reason = (kind, since_ms = 1) => ({ kind, since_ms, headline: "", actions: [] });
const pane = (attention, kind, over = {}) => ({ type: "terminal", attention, reason: kind ? reason(kind) : null, ...over });

test("an ask or a failure needs you; a finish is done", () => {
  assert.equal(needsYou(pane("needs_input", "ask")), true);
  assert.equal(needsYou(pane("needs_input", "input")), true);
  // A PR's failed checks, a failed command, a program that exited.
  assert.equal(needsYou(pane("done", "failed", { type: "forge" })), true);
  assert.equal(needsYou(pane("done", "exited")), true);
  assert.equal(needsYou(pane("done", "done")), false);
  assert.equal(finished(pane("done", "done")), true);
  assert.equal(finished(pane("done", "failed")), false);
  for (const a of ["idle", "working"]) {
    assert.equal(needsYou(pane(a, null)), false);
    assert.equal(finished(pane(a, null)), false);
  }
});

test("a remote pane is counted on its own machine, not twice", () => {
  assert.equal(needsYou(pane("needs_input", "ask", { type: "remote" })), false);
  assert.equal(finished(pane("done", "done", { type: "remote" })), false);
});

test("a stopped agent isn't running", () => {
  assert.equal(agentRunning({ type: "agent", kind: "agent", running: true }), true);
  assert.equal(agentRunning({ type: "agent", kind: "agent", running: false }), false);
  // Claude Code in a terminal is an agent while it runs there.
  assert.equal(agentRunning({ type: "terminal", kind: "agent", running: true }), true);
  assert.equal(agentRunning({ type: "terminal", kind: "build", running: true }), false);
});

test("the count reads as one label", () => {
  assert.equal(needYouText(1), "1 needs you");
  assert.equal(needYouText(3), "3 need you");
});

// #451: the panes of a wall's rail, by when each started wanting you.
const at = (attention, kind, since, id) => ({ id, type: "terminal", attention, reason: reason(kind, since) });
const order = (panes) => sortNeedsYou(panes, (p) => placeOf(p, p.id)).map((p) => p.id);

test("what waits on a person comes first: a gate, the asks, the failures, then what finished", () => {
  assert.equal(needsYouRank(at("needs_input", "gate", 1)), 0);
  for (const k of ["ask", "input", "diff", "paused", "errors", "conflict"]) assert.equal(needsYouRank(at("needs_input", k, 1)), 1, k);
  assert.equal(needsYouRank(at("done", "failed", 1)), 2);
  assert.equal(needsYouRank(at("done", "exited", 1)), 2);
  assert.equal(needsYouRank(at("done", "done", 1)), 3);
  // Three builds finished, a test run failed, then Claude Code asked: the
  // ask goes to the top, the failure after it, the finished ones last.
  const panes = [at("done", "done", 10, 1), at("done", "done", 20, 2), at("done", "done", 30, 3), at("done", "failed", 40, 4), at("needs_input", "ask", 50, 5)];
  assert.deepEqual(order(panes), [5, 4, 1, 2, 3]);
  // A gate goes above an older ask.
  assert.deepEqual(order([...panes, at("needs_input", "gate", 60, 6)]), [6, 5, 4, 1, 2, 3]);
});

test("within a group it's oldest first, so a new one never moves those above it", () => {
  const before = [at("needs_input", "ask", 50, 1), at("needs_input", "ask", 20, 2), at("done", "failed", 5, 3), at("done", "done", 1, 4)];
  assert.deepEqual(order(before), [2, 1, 3, 4]);
  // Another ask and another failure arrive: each goes to the end of its
  // group, and every card that was there keeps its place in its group.
  const after = order([at("needs_input", "ask", 90, 5), ...before, at("done", "failed", 95, 6)]);
  assert.deepEqual(after, [2, 1, 5, 3, 6, 4]);
  // The same in any input order: the sort doesn't lean on it.
  assert.deepEqual(order([...before].reverse().concat(at("done", "failed", 95, 6), at("needs_input", "ask", 90, 5))), after);
});

test("an ask without a reason dates from its ask; ties fall to the key", () => {
  const ask = { id: 7, type: "agent", attention: "needs_input", reason: null, ask: { at_ms: 3 } };
  assert.deepEqual(placeOf(ask, "m:7"), { rank: 1, since: 3, key: "m:7" });
  assert.ok(byNeedsYou({ rank: 1, since: 3, key: "a" }, { rank: 1, since: 3, key: "b" }) < 0);
  assert.ok(byNeedsYou({ rank: 1, since: 3, key: 9 }, { rank: 1, since: 3, key: 10 }) < 0);
  assert.equal(byNeedsYou({ rank: 2, since: 3, key: "a" }, { rank: 2, since: 3, key: "a" }), 0);
});
