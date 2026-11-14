// A project's Sessions page, by card (Arugula inversion L3). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { cardTitle, finished, groupsOf, panesShown, waitsOn } from "../src/platform/sessions-by-card.ts";
import { finishedStates, hubStanding, stateStanding } from "../src/shared/project-sessions.ts";
import { DANA, byCard, wave } from "../e2e/sessions-fixture.ts";

const M = "d00d0001";
const all = { narrowed: false, going: false };
const titles = (gs) => gs.map((g) => cardTitle(g.card));

test("groups that need you come first, then the cloud's order, and Not on a card last", () => {
  const gs = groupsOf(byCard(M, 4), wave(M), all);
  assert.deepEqual(titles(gs), ["#985 A project's Sessions page", "#970 The rail", "#990 Fold finished", "Not on a card"]);
});

test("a hosted session is one row, with the wave's words; machine sessions sit beside it", () => {
  const gs = groupsOf(byCard(M, 4), wave(M), all);
  const rail = gs.find((g) => g.card?.number === 970);
  assert.deepEqual(rail.sessions.map((s) => s.key), ["machine:7", "hosted:ses_0970bbbb"]);
  assert.equal(rail.sessions[1].wave.branch, "arugula/0970-rail");
  assert.equal(waitsOn(gs[0].sessions[0]), "its question");
});

test("without the by-card route, the wave alone, grouped by its issues", () => {
  const gs = groupsOf(null, wave(M), all);
  assert.deepEqual(titles(gs), ["#985 A project's Sessions page", "#970 The rail", "#990 Fold finished"]);
  assert.ok(gs.every((g) => g.sessions.every((s) => s.src === null)));
});

test("a label, a tracker or who asked narrows to the wave's sessions", () => {
  const gs = groupsOf(byCard(M, 4), wave(M), { narrowed: true, going: false });
  assert.ok(gs.flatMap((g) => g.sessions).every((s) => s.wave));
  assert.equal(gs.some((g) => !g.card), false);
});

test("Going only leaves finished sessions out, on every kind", () => {
  const gs = groupsOf(byCard(M, 4), wave(M), { narrowed: false, going: true });
  const keys = gs.flatMap((g) => g.sessions.map((s) => s.key));
  assert.ok(!keys.includes("hosted:ses_0970bbbb"));
  assert.ok(!keys.includes(`agent:${DANA}.9`));
  assert.ok(keys.includes("machine:2"), "a pane with no state known is going");
});

test("finished: ended and waiting on no one", () => {
  const gs = groupsOf(byCard(M, 4), wave(M), all);
  const s = (key) => gs.flatMap((g) => g.sessions).find((x) => x.key === key);
  assert.equal(finished(s("hosted:ses_0970bbbb")), true);
  assert.equal(finished(s("hosted:ses_0985aaaa")), false);
  assert.equal(finished(s(`agent:${DANA}.9`)), true);
  assert.equal(finished(s("machine:7")), false);
});

test("a row's panes: a machine session's, an agent's own, a hosted one's none", () => {
  const b = byCard(M, 4);
  assert.deepEqual(panesShown(b.groups[0].sessions[0]), [{ machine: M, pane: 4, kind: "terminal", state: "working" }]);
  assert.deepEqual(panesShown(b.groups[2].sessions[1]), [{ machine: DANA, pane: 9, kind: "agent", state: "exited" }]);
  assert.deepEqual(panesShown(b.groups[1].sessions[0]), []);
});

test("a card that isn't an issue is named by its row's words, else its id", () => {
  assert.equal(cardTitle({ id: "row_1" }, "Ship it"), "Ship it");
  assert.equal(cardTitle({ id: "fi_2" }), "fi_2");
});

test("the Overview and Sessions read states alike: what waits on you, failures, working, finished", () => {
  assert.equal(stateStanding(["working", "needs_input"]), "needs");
  assert.equal(stateStanding(["permission"]), "needs");
  assert.equal(stateStanding(["error", "working"]), "failed");
  assert.equal(stateStanding(["running"]), "working");
  assert.equal(stateStanding(["", "idle"]), "quiet");
  assert.equal(hubStanding(byCard(M, 4).groups[1].sessions[0]), "needs");
  assert.equal(finishedStates(["exited", "done"]), true);
  assert.equal(finishedStates(["exited", null]), false, "a pane not said isn't finished");
  assert.equal(finishedStates(["failed"]), false, "a failure stays in sight");
  assert.equal(finishedStates([]), false);
});
