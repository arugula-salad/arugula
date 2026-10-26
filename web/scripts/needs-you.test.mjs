// What "needs you" means (shared/needs-you, UI audit #154): the bell, the
// footer, the Sessions sidebar and the Swarm all count with it. An ask or
// a failure needs you; a plain finish is done; a remote pane's own machine
// counts it; a stopped agent isn't running. Run with `node
// --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { agentRunning, finished, needsYou, needYouText } from "../src/shared/needs-you.ts";

const reason = (kind) => ({ kind, since_ms: 1, headline: "", actions: [] });
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
