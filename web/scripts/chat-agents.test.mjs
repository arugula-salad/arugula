// Agents in Chat (Arugula #1522): which rooms are the agents', and the
// Agents section's rows. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { agentRooms, isAgentRoom, isAgentsRoom, isBoardChannel, isStaffRoom } from "../src/shared/chat/agent-rooms.ts";

const P = "prj_1";
const obj = (id, target, project = P) => ({ id, kind: "object", target, project, staff: false });
const board = (id, title, closed) => ({ id, title, project: P, ...(closed ? { closed_at: 1 } : {}) });

test("#agents is the project's thread; a board's channel is its board's; an issue's comments are neither", () => {
  assert.ok(isAgentsRoom(obj("ob_1", P), P));
  assert.ok(!isAgentsRoom(obj("ob_1", "prj_2", "prj_2"), P));
  assert.ok(isBoardChannel(obj("ob_2", "brd_1")));
  const issue = obj("ob_3", "iss_1");
  assert.ok(!isAgentRoom(issue) && !isBoardChannel(issue) && !isAgentsRoom(issue));
  assert.ok(!isAgentRoom({ id: "ch_1", kind: "channel", target: null }));
});

test("the chief of staff is the reader's staff room in this project", () => {
  assert.ok(isStaffRoom({ kind: "dm", staff: true, project: P }, P));
  assert.ok(!isStaffRoom({ kind: "dm", staff: true, project: "prj_2" }, P));
  assert.ok(!isStaffRoom({ kind: "dm", staff: false, project: P }, P));
});

test("the section: staff, #agents, open boards in order with their channels if made, closed boards archived", () => {
  const rooms = [{ id: "dm_s", kind: "dm", staff: true, project: P }, obj("ob_a", P), obj("ob_b2", "brd_2"), obj("ob_x", "iss_9"), obj("ob_o", "prj_2", "prj_2")];
  const boards = [board("brd_1", "One"), board("brd_2", "Two"), board("brd_3", "Old", true), { ...board("brd_4", "Elsewhere"), project: "prj_2" }];
  const a = agentRooms(rooms, boards, P);
  assert.equal(a.staff?.id, "dm_s");
  assert.equal(a.agents?.id, "ob_a");
  assert.deepEqual(a.open.map((c) => [c.board.id, c.room?.id]), [["brd_1", undefined], ["brd_2", "ob_b2"]]);
  assert.deepEqual(a.closed.map((c) => c.board.id), ["brd_3"]);
});

test("no #agents yet (a server without it): the section has no row for it", () => {
  const a = agentRooms([], [board("brd_1", "One")], P);
  assert.equal(a.agents, undefined);
  assert.equal(a.open.length, 1);
});
