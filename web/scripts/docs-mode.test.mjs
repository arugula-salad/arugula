// Whose Docs a project's are (Arugula #864): personal or a team's, and what
// the list's Mine filter holds. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { madeBy, modeOf, PERSONAL } from "../src/shared/docs/docs-mode.ts";

const personal = { id: "prj_me", name: "Mine", team: null, owner: { kind: "personal", account: "acc_me" } };
const teamDefault = { id: "prj_t", name: "Acme", team: { id: "team_1", name: "Acme", role: "editor", members: [] }, owner: { kind: "team", id: "team_1", name: "Acme" } };
const teamOther = { id: "prj_t2", name: "Acme web", team: null, owner: { kind: "team", id: "team_1", name: "Acme" } };

test("a personal project is the reader's own; a team's is the team's, its default project or another", () => {
  assert.deepEqual(modeOf(personal), PERSONAL);
  assert.deepEqual(modeOf(teamDefault), { kind: "team", project: "prj_t", name: "Acme" });
  assert.deepEqual(modeOf(teamOther), { kind: "team", project: "prj_t2", name: "Acme web" });
});

test("from an older control with no owner, a project's team says", () => {
  assert.deepEqual(modeOf({ ...teamDefault, owner: undefined }), { kind: "team", project: "prj_t", name: "Acme" });
  assert.deepEqual(modeOf({ ...personal, owner: undefined }), PERSONAL);
});

test("Mine is the reader's own and their agents'", () => {
  const byMe = { author: "acc_me", agent: null };
  const byMyAgent = { author: "acc_me", agent: { machine: "dev_1", machine_name: "laptop" } };
  const byThem = { author: "acc_them", agent: null };
  const byTheirAgent = { author: "acc_them", agent: { machine: "dev_2", machine_name: "box" } };
  // An agent session on a Cloud workspace makes its Docs as itself: nobody's.
  const bySession = { author: "", agent: { machine: "ses_1", machine_name: "Agent session ses_1" } };
  assert.deepEqual([byMe, byMyAgent, byThem, byTheirAgent, bySession].map((d) => madeBy(d, "acc_me")), [true, true, false, false, false]);
  // Signed out, nothing is the reader's.
  assert.equal(madeBy(byMe, null), false);
  assert.equal(madeBy(bySession, ""), false);
});
