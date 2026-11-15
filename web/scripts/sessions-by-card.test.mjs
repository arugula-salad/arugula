// A project's Sessions page, by card (Arugula inversion L3). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { MOVE_KEPT_MS, branchOf, cardPlace, cardTitle, finished, groupsOf, keptMoves, movable, panesShown, readsBoards, undoMove, waitsOn, withMoves } from "../src/platform/sessions-by-card.ts";
import { finishedStates, hubStanding, setSessionCard, stateStanding } from "../src/shared/project-sessions.ts";
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
  assert.deepEqual(rail.sessions.map((s) => s.key), ["machine:7", "hosted:ses_0971dddd", "hosted:ses_0970bbbb"]);
  assert.equal(rail.sessions[2].wave.branch, "arugula/0970-rail");
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
  assert.ok(keys.includes("hosted:ses_0971dddd"), "a hosted session whose turn ended is going");
  assert.ok(keys.includes("machine:2"), "a pane with no state known is going");
});

test("finished: ended and waiting on no one", () => {
  const gs = groupsOf(byCard(M, 4), wave(M), all);
  const s = (key) => gs.flatMap((g) => g.sessions).find((x) => x.key === key);
  assert.equal(finished(s("hosted:ses_0970bbbb")), true);
  assert.equal(finished(s("hosted:ses_0985aaaa")), false);
  assert.equal(finished(s("hosted:ses_0971dddd")), false, "a hosted done is a turn ended, not finished");
  assert.equal(finished(s(`agent:${DANA}.9`)), true);
  assert.equal(finished(s("machine:7")), false);
});

test("a hosted session whose turn ended and settled isn't finished: settled isn't ended", () => {
  const src = { kind: "hosted", id: "ses_1", name: "arugula/1", on: { kind: "hosted" }, state: "done", settled: true, branch: "arugula/1", updated_at: 0 };
  assert.equal(finished({ key: "hosted:ses_1", src, wave: null }), false);
  const gs = groupsOf({ project: "p", groups: [{ card: null, sessions: [src] }] }, null, { narrowed: false, going: true });
  assert.deepEqual(gs.flatMap((g) => g.sessions.map((s) => s.key)), ["hosted:ses_1"]);
  assert.equal(finished({ key: "hosted:ses_1", src: { ...src, state: "stopped" }, wave: null }), true);
});

test("a session's branch: its own, else its panes', else not said", () => {
  const b = byCard(M, 4);
  assert.equal(branchOf(b.groups[0].sessions[0]), "arugula/0970-rail-pane");
  assert.equal(branchOf(b.groups[0].sessions[2]), "arugula/0970-rail-turn");
  assert.equal(branchOf(b.groups[2].sessions[1]), "main");
  assert.equal(branchOf(b.groups[2].sessions[0]), null);
  assert.equal(branchOf(byCard(M, 4, false).groups[0].sessions[0]), null, "a cloud from before branches");
});

test("a card's board and lane from the answer; the boards are read only when it doesn't say them", () => {
  const b = byCard(M, 4);
  assert.deepEqual(cardPlace(b.groups[1].card), { board: "brd_00e2e0", boardTitle: "Launch", lane: "Doing" });
  assert.equal(cardPlace(b.groups[0].card), null, "on no board");
  assert.equal(cardPlace(null), null);
  assert.deepEqual(cardPlace({ id: "row_1", board: { id: "brd_1", title: "B" }, lane: null }), { board: "brd_1", boardTitle: "B", lane: "" });
  assert.equal(readsBoards(b), false);
  assert.equal(readsBoards(byCard(M, 4, false)), true, "a cloud from before the fields");
  assert.equal(readsBoards(null), true, "the wave alone");
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
  assert.equal(stateStanding(["unresponsive"]), "failed", "a hosted session that stopped answering failed");
  assert.equal(finishedStates(["exited", "stopped", "closed"]), true);
  assert.equal(finishedStates(["done"]), false, "a hosted done only says its turn ended");
  assert.equal(finishedStates(["exited", null]), false, "a pane not said isn't finished");
  assert.equal(finishedStates(["failed"]), false, "a failure stays in sight");
  assert.equal(finishedStates([]), false);
});

// Put on a card (Arugula #1019): the move shows at once and is kept over
// reads that don't say it yet; a refusal takes it back.
const release = (answer) => answer.groups.find((g) => !g.card).sessions.find((s) => s.kind === "machine");
const rail = { id: "iss_970", number: 970, title: "The rail", board: null, lane: null };
const fresh = { id: "iss_1019", number: 1019, title: "Put a session on a card" };
const where = (answer, id) => answer.groups.find((g) => g.sessions.some((s) => s.kind === "machine" && s.id === id))?.card?.id ?? null;

test("only a machine's session is put on a card from the page", () => {
  const a = byCard(M, 4);
  assert.equal(movable(release(a)), true);
  assert.equal(movable(a.groups.find((g) => !g.card).sessions.find((s) => s.kind === "agent")), false);
  assert.equal(movable(a.groups[1].sessions[0]), false);
  assert.equal(movable(null), false);
});

test("a move shows at once: off Not on a card and onto its card's group, made when no session was on it", () => {
  const a = byCard(M, 4);
  const onRail = withMoves(a, [{ session: release(a), from: null, to: rail, at: 1 }]);
  assert.equal(where(onRail, "2"), "iss_970");
  assert.equal(onRail.groups.find((g) => g.card?.id === "iss_970").sessions.length, 4);
  // The answer itself is left as it was.
  assert.equal(where(a, "2"), null);
  const made = withMoves(a, [{ session: release(a), from: null, to: fresh, at: 1 }]);
  assert.deepEqual(made.groups.find((g) => g.card?.id === "iss_1019"), { card: fresh, sessions: [release(a)] });
  // Its new group is before Not on a card, which keeps the lead.
  assert.deepEqual(made.groups.map((g) => g.card?.id ?? null), ["iss_970", "iss_985", "iss_1019", null]);
  assert.deepEqual(titles(groupsOf(made, wave(M), all)), ["#985 A project's Sessions page", "#970 The rail", "#1019 Put a session on a card", "#990 Fold finished", "Not on a card"]);
  // Back to Not on a card, and a group left empty goes.
  const back = withMoves(made, [{ session: release(a), from: fresh, to: null, at: 2 }]);
  assert.equal(where(back, "2"), null);
  assert.equal(back.groups.some((g) => g.card?.id === "iss_1019"), false);
  // No by-card answer: nothing to move.
  assert.equal(withMoves(null, [{ session: release(a), from: null, to: rail, at: 1 }]), null);
});

test("a move is kept over a read that doesn't say it yet, and dropped once one does, or after a while", () => {
  const a = byCard(M, 4);
  const m = { session: release(a), from: null, to: fresh, at: 1_000 };
  // The 60s re-read, from before the machine reported: kept, and still shown.
  const kept = keptMoves(a, [m], 61_000);
  assert.deepEqual(kept, [m]);
  assert.equal(where(withMoves(a, kept), "2"), "iss_1019");
  // A read that agrees: the answer alone says it now.
  const agreed = withMoves(a, [m]);
  assert.deepEqual(keptMoves(agreed, [m], 61_000), []);
  // Nobody agreed in time: the cloud's answer is the page's again.
  assert.deepEqual(keptMoves(a, [m], 1_000 + MOVE_KEPT_MS + 1), []);
  // Not on a card agrees once it's off the card it was on, whatever its branch gives.
  const off = { session: release(a), from: rail, to: null, at: 1_000 };
  const onRail = withMoves(a, [{ session: release(a), from: null, to: rail, at: 0 }]);
  assert.deepEqual(keptMoves(onRail, [off], 2_000), [off]);
  assert.deepEqual(keptMoves(a, [off], 2_000), []);
  assert.deepEqual(keptMoves(withMoves(a, [{ session: release(a), from: null, to: fresh, at: 0 }]), [off], 2_000), []);
});

test("a refused move is undone: the session is back where the answer has it", () => {
  const a = byCard(M, 4);
  const m = { session: release(a), from: null, to: rail, at: 1 };
  const other = { session: a.groups[0].sessions[0], from: rail, to: fresh, at: 1 };
  const left = undoMove([m, other], m);
  assert.deepEqual(left, [other]);
  assert.equal(where(withMoves(a, left), "2"), null);
  assert.equal(where(withMoves(a, left), "7"), "iss_1019");
});

test("setSessionCard puts the session's card, and says why not", async () => {
  const a = byCard(M, 4);
  const sent = [];
  const answer = (status, body) => async (url, init) => (sent.push({ url, init }), new Response(body === undefined ? null : JSON.stringify(body), { status }));
  await setSessionCard("prj_aaaa0002", release(a), "iss_1019", answer(204));
  assert.equal(sent[0].url, "/api/projects/prj_aaaa0002/sessions/machine/2/card");
  assert.equal(sent[0].init.method, "PUT");
  assert.deepEqual(JSON.parse(sent[0].init.body), { card: "iss_1019", machine: DANA });
  await setSessionCard("prj_aaaa0002", release(a), null, answer(200, {}));
  assert.deepEqual(JSON.parse(sent[1].init.body), { card: null, machine: DANA });
  await assert.rejects(setSessionCard("prj_aaaa0002", release(a), "iss_1019", answer(409, { error: "danas-mini is offline" })), /danas-mini is offline/);
  await assert.rejects(setSessionCard("prj_aaaa0002", release(a), "iss_1019", answer(404, { error: "not found" })), /can't move sessions/);
  await assert.rejects(setSessionCard("prj_aaaa0002", release(a), "iss_1019", answer(404, { error: "no such session" })), /no such session/);
  await assert.rejects(setSessionCard("prj_aaaa0002", release(a), "iss_1019", answer(422)), /HTTP 422/);
});
