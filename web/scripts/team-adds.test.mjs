// Which teams the page offers for a machine, by control's two rules
// (`in_team` for a move or a signed join, `may_add_to` for a join in one
// click): owner, editor, viewer and someone not in the roster, each with
// the team locked and not. Since #1184 both rules are the same: a viewer
// never adds a machine. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { addsInOneClick, cantAdd, movesInto } from "../src/shared/team-adds.ts";

const roles = ["owner", "editor", "viewer", null];
const offered = (rule, locked) => roles.filter((role) => rule({ role, locked }));

test("a move or a signed join: owners and editors, only owners while locked", () => {
  assert.deepEqual(offered(movesInto, false), ["owner", "editor"]);
  assert.deepEqual(offered(movesInto, true), ["owner"]);
});

test("a join in one click: owners and editors, only owners while locked", () => {
  assert.deepEqual(offered(addsInOneClick, false), ["owner", "editor"]);
  assert.deepEqual(offered(addsInOneClick, true), ["owner"]);
});

test("why a team asked for isn't offered: a viewer is told they're a viewer (#1183)", () => {
  const viewer = cantAdd("Acme", { role: "viewer", locked: false });
  assert.equal(viewer.note, "viewers can't add machines");
  assert.match(viewer.error, /^Viewers can't add machines to Acme: only its owners and editors do\./);
  assert.doesNotMatch(viewer.note + viewer.error, /not in it|members add/);
  assert.equal(cantAdd("Acme", { role: "viewer", locked: true }).note, "locked");
  assert.equal(cantAdd("Acme", { role: "editor", locked: true }).note, "locked");
  const stranger = cantAdd("Acme", null);
  assert.equal(stranger.note, "you're not in it");
  assert.match(stranger.error, /^Only Acme's owners and editors add machines to it\./);
});
