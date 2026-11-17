// Which teams the page offers for a machine, by control's two rules
// (`in_team` for a move or a signed join, `may_add_to` for a join in one
// click): owner, editor, viewer and someone not in the roster, each with
// the team locked and not. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { addsInOneClick, movesInto } from "../src/shell/team-adds.ts";

const roles = ["owner", "editor", "viewer", null];
const offered = (rule, locked) => roles.filter((role) => rule({ role, locked }));

test("a move or a signed join: any member, only owners while locked", () => {
  assert.deepEqual(offered(movesInto, false), ["owner", "editor", "viewer"]);
  assert.deepEqual(offered(movesInto, true), ["owner"]);
});

test("a join in one click: owners and editors, only owners while locked", () => {
  assert.deepEqual(offered(addsInOneClick, false), ["owner", "editor"]);
  assert.deepEqual(offered(addsInOneClick, true), ["owner"]);
});
