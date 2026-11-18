// The session menu's Project… (Arugula #1177): who sees a session, and
// what moving it does to that. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { movedSaid, projectRef, projectRows, teamNow, whoLine, whoSees } from "../src/shell/ui/session-project.ts";

const D = "prj_dddddddddddddddddddddddddddddddd";
const F = "prj_ffffffffffffffffffffffffffffffff";
const A = "prj_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const R = "prj_rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr";
const mine = { kind: "personal", account: "acc_1" };
const all = {
  me: { account: "acc_1", login: "ada", name: "Ada" },
  projects: [
    { id: D, name: "Default", team: null, owner: mine },
    { id: F, name: "Factorio mods", team: null, owner: mine },
    { id: A, name: "Arugula", team: null, owner: { kind: "team", id: "team_a", name: "Arugula" } },
    { id: R, name: "Old", team: null, owner: mine, archived: true },
  ],
  hosted: false,
};
const [def, fac, aru] = all.projects;
const ref = (p) => projectRef(p);

test("each project says who sees its sessions", () => {
  assert.equal(whoSees(aru), "Arugula watches");
  assert.equal(whoSees(def), "Just you");
  // An older control names no owner: no team.
  assert.equal(whoSees({ id: A, name: "Arugula", team: null }), "Just you");
});

test("the list leaves out archived projects, unless the session is in one", () => {
  assert.deepEqual(projectRows(all, ref(def)).map((p) => p.id), [D, F, A]);
  assert.deepEqual(projectRows(all, { id: R, name: "Old" }).map((p) => p.id), [D, F, A, R]);
});

test("with nothing picked, it says who sees the session now", () => {
  assert.deepEqual(whoLine(teamNow(all, ref(aru)), ref(aru), null), { text: "Now: you, and the Arugula team watching.", warn: false });
  assert.deepEqual(whoLine(teamNow(all, ref(def)), ref(def), def), { text: "Now: only you.", warn: false });
});

test("into a team's project warns that the team will watch what it printed before", () => {
  const l = whoLine(teamNow(all, ref(fac)), ref(fac), aru);
  assert.equal(l.warn, true);
  assert.equal(l.text, "Moving it to Arugula lets the Arugula team watch it: its tabs, its panes and what they print, including what they printed before.");
});

test("out of a team's project takes it away from the team", () => {
  assert.deepEqual(whoLine(teamNow(all, ref(aru)), ref(aru), def), { text: "Moving it to Default takes it away from the Arugula team: only you will see it.", warn: false });
});

test("between two of your own, who sees it doesn't change", () => {
  assert.deepEqual(whoLine(teamNow(all, ref(def)), ref(def), fac), { text: "Who sees it doesn't change: only you.", warn: false });
});

test("a share with a person stays, and the line names them", () => {
  assert.match(whoLine(null, ref(def), aru, ["Jake", "Ana", "Bo"]).text, / It stays shared with Jake, Ana and Bo\.$/);
});

test("a session whose project isn't listed goes by the team it keeps", () => {
  assert.deepEqual(teamNow(all, { id: "prj_gone", name: "Gone", team: "team_a" }), { id: "team_a", name: "Arugula" });
  assert.equal(teamNow(all, { id: "prj_gone", name: "Gone" }), null);
});

test("the toast says where it went and who sees it", () => {
  assert.equal(movedSaid("belt-balancer", aru), "“belt-balancer” is in Arugula now. The Arugula team can watch it.");
  assert.equal(movedSaid("dotfiles", fac), "“dotfiles” is in Factorio mods now. Only you can see it.");
});

test("a team's project carries its team, which says who watches the session (Arugula #1162)", () => {
  assert.deepEqual(projectRef(aru), { id: A, name: "Arugula", team: "team_a" });
  assert.deepEqual(projectRef(def), { id: D, name: "Default" });
  assert.deepEqual(projectRef({ id: A, name: "Arugula", team: null }), { id: A, name: "Arugula" });
});
