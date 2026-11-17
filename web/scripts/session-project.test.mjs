// The session menu's Project… (Arugula projects RFC). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { NO_PROJECT, projectChoices, projectPicked } from "../src/shell/ui/session-project.ts";

const A = "prj_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B = "prj_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const all = {
  me: { account: "acc_1", login: "ada", name: "Ada" },
  projects: [
    { id: A, name: "Default", team: null },
    { id: B, name: "Acme", team: { id: "team_1", name: "Acme", role: "member", members: [] } },
  ],
  hosted: false,
};

test("a session in no project gets the reader's projects, none marked, and no clear", () => {
  const c = projectChoices(all, null);
  assert.deepEqual(c.map((x) => x.value), [A, B]);
  assert.ok(c.every((x) => !x.strong));
  assert.equal(c[1].hint, "team");
});

test("a session in a project has it first and marked, and can be taken out", () => {
  const c = projectChoices(all, { id: B, name: "Acme" });
  assert.deepEqual(c.map((x) => x.value), [B, A, NO_PROJECT]);
  assert.equal(c[0].strong, true);
  assert.equal(c[2].label, "No project");
});

test("an answer is a choice, an id, a name in any case, or none", () => {
  assert.deepEqual(projectPicked(B, all), { id: B, name: "Acme" });
  assert.deepEqual(projectPicked(" acme ", all), { id: B, name: "Acme" });
  assert.equal(projectPicked(NO_PROJECT, all), null);
  assert.equal(projectPicked("No project", all), null);
  assert.equal(projectPicked("Elsewhere", all), undefined);
});

test("a team's project carries its team, which says who watches the session (Arugula #1162)", () => {
  const owned = {
    ...all,
    projects: [
      { ...all.projects[0], owner: { kind: "personal", account: "acc_1" } },
      { ...all.projects[1], owner: { kind: "team", id: "team_1", name: "Acme" } },
    ],
  };
  assert.deepEqual(projectPicked(B, owned), { id: B, name: "Acme", team: "team_1" });
  assert.deepEqual(projectPicked(A, owned), { id: A, name: "Default" });
  // An older control names no owner: no team.
  assert.deepEqual(projectPicked(B, all), { id: B, name: "Acme" });
});
