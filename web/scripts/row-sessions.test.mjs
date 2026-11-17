// A card's sessions on its board row (Arugula #1082, the Sessions page
// folded away). Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { pushedSessions, rowSessions } from "../src/shared/boards/row-sessions.ts";
import { finishedStates, hubStanding, stateStanding } from "../src/shared/project-sessions.ts";
import { DANA, byCard } from "../e2e/sessions-fixture.ts";

const M = "d00d0001";
const row = (over = {}) => ({ id: "row_0970", pointer: { kind: "issue", id: "iss_970" }, attachments: [], sessions: [], ...over });

test("a row shows its card's sessions, finished ones left out", () => {
  const got = rowSessions(row(), byCard(M, 4));
  assert.deepEqual(got.map((s) => s.key), [`machine:${M}:7`, "hosted::ses_0971dddd"]);
  assert.deepEqual(got[0], { key: `machine:${M}:7`, name: "rail-work", where: "sams-laptop", state: "working", needs: false, open: { kind: "pane", machine: M, pane: "4" } });
  assert.deepEqual(got[1].open, { kind: "hosted", session: "ses_0971dddd" });
  assert.equal(got[1].where, null);
});

test("what the row holds already isn't said twice: a hosted session on its issue, an agent block attached", () => {
  const hosted = rowSessions(row({ sessions: [{ session: "ses_0971dddd" }] }), byCard(M, 4));
  assert.deepEqual(hosted.map((s) => s.key), [`machine:${M}:7`]);
  const agent = { kind: "agent", id: 9, name: "lead", on: { kind: "machine", id: DANA, name: "danas-mini" }, state: "needs_input", updated_at: 0 };
  const answer = { project: "p", groups: [{ card: { id: "iss_970" }, sessions: [agent] }] };
  assert.equal(rowSessions(row(), answer).length, 1);
  const attached = row({ attachments: [{ id: "att_1", role: "work", pointer: { kind: "block", machine: DANA, pane: "%9" } }] });
  assert.deepEqual(rowSessions(attached, answer), []);
});

test("what needs you comes first, and a finished session that waits on you stays", () => {
  const answer = {
    project: "p",
    groups: [
      {
        card: { id: "row_note" },
        sessions: [
          { kind: "machine", id: "1", name: "", on: { kind: "machine", id: M, name: "m" }, panes: [{ pane: 2, kind: "terminal", work_item: null, state: "working" }], updated_at: 0 },
          { kind: "machine", id: "3", name: "asks", on: { kind: "machine", id: M, name: "m" }, panes: [{ pane: 5, kind: "terminal", work_item: null, state: "exited" }, { pane: 6, kind: "agent", work_item: null, state: "needs_input" }], updated_at: 0 },
        ],
      },
    ],
  };
  // A card that isn't an issue is found by the row's own id.
  const got = rowSessions(row({ id: "row_note", pointer: { kind: "note" } }), answer);
  assert.deepEqual(got.map((s) => [s.name, s.state, s.needs]), [["asks", "needs input", true], ["Session", "working", false]]);
  assert.deepEqual(got[0].open, { kind: "pane", machine: M, pane: "6" }, "the pane that asks");
});

test("another card's sessions, Not on a card, or no answer: none", () => {
  assert.deepEqual(rowSessions(row({ id: "row_x", pointer: { kind: "issue", id: "iss_1" } }), byCard(M, 4)), []);
  assert.deepEqual(rowSessions(row(), null), []);
});

test("the Overview and the rows read states alike: what waits on you, failures, working, finished", () => {
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

test("the pushes a board hears: its rows' own hosted sessions, and the hosted ones on its cards", () => {
  const rows = [row({ sessions: [{ session: "ses_row" }] }), row({ id: "row_0985", pointer: { kind: "issue", id: "iss_985" } })];
  assert.deepEqual(pushedSessions(rows, byCard(M, 4)), { board: ["ses_row"], cards: ["ses_0970bbbb", "ses_0971dddd", "ses_0985aaaa"] });
  // A card on no row, and a computer's session, push nothing here.
  assert.deepEqual(pushedSessions([row({ id: "row_x", pointer: { kind: "issue", id: "iss_1" } })], byCard(M, 4)), { board: [], cards: [] });
  assert.deepEqual(pushedSessions(rows, null).cards, []);
});
