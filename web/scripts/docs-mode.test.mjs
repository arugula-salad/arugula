// The one Docs list's modes (Arugula #864): personal or a team's, what My
// Docs holds, and the tab a browser remembers. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { inTab, keptLine, machineMode, madeBy, modeOf, PERSONAL, savedTab, saveTab } from "../src/shared/docs/docs-mode.ts";

const personal = { id: "prj_me", name: "Mine", team: null, owner: { kind: "personal", account: "acc_me" } };
const teamDefault = { id: "prj_t", name: "Acme", team: { id: "team_1", name: "Acme", role: "editor", members: [] }, owner: { kind: "team", id: "team_1", name: "Acme" } };
const teamOther = { id: "prj_t2", name: "Acme web", team: null, owner: { kind: "team", id: "team_1", name: "Acme" } };

test("a personal project has no tabs; a team's has them, its default project or another", () => {
  assert.deepEqual(modeOf(personal), PERSONAL);
  assert.deepEqual(modeOf(teamDefault), { kind: "team", project: "prj_t", name: "Acme" });
  assert.deepEqual(modeOf(teamOther), { kind: "team", project: "prj_t2", name: "Acme web" });
});

test("from an older control with no owner, a project's team says", () => {
  assert.deepEqual(modeOf({ ...teamDefault, owner: undefined }), { kind: "team", project: "prj_t", name: "Acme" });
  assert.deepEqual(modeOf({ ...personal, owner: undefined }), PERSONAL);
});

test("a machine is in a team when it's a team's machine: its Docs are the team's default project's", () => {
  const projects = [personal, teamOther, teamDefault];
  assert.deepEqual(machineMode("team_1", projects), { kind: "team", project: "prj_t", name: "Acme" });
  // A personal machine, or one whose access said no team, keeps its library.
  assert.deepEqual(machineMode(null, projects), PERSONAL);
  assert.deepEqual(machineMode(undefined, projects), PERSONAL);
  // A team the reader has no project of (or the platform couldn't say) leaves the library.
  assert.deepEqual(machineMode("team_9", projects), PERSONAL);
  assert.deepEqual(machineMode("team_1", []), PERSONAL);
});

test("My Docs is the reader's own and their agents'; Team Docs is all of them", () => {
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
  assert.deepEqual([inTab("mine", true), inTab("mine", false), inTab("team", true), inTab("team", false)], [true, false, true, true]);
});

test("the tab used last is remembered; My Docs until then, or when storage fails", () => {
  const kept = new Map();
  const store = { getItem: (k) => kept.get(k) ?? null, setItem: (k, v) => kept.set(k, v) };
  assert.equal(savedTab(store), "mine");
  saveTab("team", store);
  assert.equal(savedTab(store), "team");
  saveTab("mine", store);
  assert.equal(savedTab(store), "mine");
  kept.set("docs-tab", "nonsense");
  assert.equal(savedTab(store), "mine");
  // A browser that refuses storage (Safari's private mode, a quota) still shows a tab.
  const broken = {
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("denied");
    },
  };
  assert.doesNotThrow(() => saveTab("team", broken));
  assert.equal(savedTab(broken), "mine");
  assert.equal(savedTab(null), "mine");
});

test("a team machine's line counts the library Docs it still keeps, and is gone with none", () => {
  assert.equal(keptLine(0), null);
  assert.equal(keptLine(1), "1 Doc kept only on this machine");
  assert.equal(keptLine(34), "34 Docs kept only on this machine");
});
