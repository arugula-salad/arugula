// Project defaults and overrides (Arugula #897): staging cells, the PUTs
// Save sends, Cancel, and what each cell and Overrides line shows. Run
// with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { OWNER, changes, ownerCell, overrideLine, projectCell, send, stage, valueOf, words, writes } from "../src/platform/settings/overrides.ts";

const A = "prj_aaaa";
const B = "prj_bbbb";
const layers = {
  owner: [
    { key: "issue_home", value: "native", source: "owner" },
    { key: "agents_model", value: null, source: "default" },
    { key: "stall_minutes", value: 30, source: "default" },
    { key: "default_project", value: A, source: "owner" },
  ],
  projects: {
    [A]: [
      { key: "issue_home", value: "native", source: "owner" },
      { key: "agents_model", value: "sonnet", source: "project" },
      { key: "agents_workspace", value: null, source: "default" },
      { key: "stall_minutes", value: 30, source: "default" },
    ],
    [B]: [
      { key: "issue_home", value: "github", source: "project" },
      { key: "agents_model", value: null, source: "default" },
      { key: "agents_workspace", value: "hw_1", source: "project" },
      { key: "stall_minutes", value: 30, source: "default" },
    ],
  },
};

test("a cell set back to what was read is no longer a change", () => {
  let s = stage({}, layers, A, "agents_model", "opus");
  assert.equal(changes(s), 1);
  s = stage(s, layers, A, "agents_model", "sonnet");
  assert.equal(changes(s), 0);
  // Clearing an override is a change; clearing one that isn't, isn't.
  assert.equal(changes(stage({}, layers, B, "issue_home", null)), 1);
  assert.equal(changes(stage({}, layers, A, "issue_home", null)), 0);
});

test("Save sends one PUT per changed cell, the owner's first, null to clear", () => {
  let s = stage({}, layers, B, "issue_home", null);
  s = stage(s, layers, A, "stall_minutes", 60);
  s = stage(s, layers, OWNER, "stall_minutes", 120);
  const w = writes(s, "team_1", [A, B]);
  assert.deepEqual(
    w.map((x) => [x.path, x.body]),
    [
      ["/api/owners/team_1/settings", { key: "stall_minutes", value: 120 }],
      [`/api/projects/${A}/settings`, { key: "stall_minutes", value: 60 }],
      [`/api/projects/${B}/settings`, { key: "issue_home", value: null }],
    ],
  );
  assert.equal(writes(stage({}, layers, OWNER, "agents_model", "opus"), "personal", [])[0].path, "/api/owners/personal/settings");
});

test("Cancel leaves every cell as it was read", () => {
  const s = stage(stage({}, layers, OWNER, "issue_home", "github"), layers, B, "agents_workspace", null);
  assert.equal(projectCell(layers, s, A, "issue_home").shown, "github");
  // Cancel is an empty set of changes.
  assert.deepEqual(projectCell(layers, {}, A, "issue_home"), { own: null, shown: "native", overridden: false, staged: false });
  assert.deepEqual(projectCell(layers, {}, B, "agents_workspace"), { own: "hw_1", shown: "hw_1", overridden: true, staged: false });
});

test("a project on the default follows a staged change to it; an override doesn't", () => {
  const s = stage({}, layers, OWNER, "stall_minutes", 60);
  assert.deepEqual(ownerCell(layers, s, "stall_minutes"), { own: 60, shown: 60, overridden: true, staged: true });
  assert.equal(projectCell(layers, s, A, "stall_minutes").shown, 60);
  const t = stage(s, layers, B, "stall_minutes", 15);
  assert.deepEqual(projectCell(layers, t, B, "stall_minutes"), { own: 15, shown: 15, overridden: true, staged: true });
  // The owner's default cleared: the built-in value answers.
  assert.equal(ownerCell(layers, stage({}, layers, OWNER, "issue_home", null), "issue_home").shown, null);
  assert.equal(ownerCell(layers, {}, "stall_minutes").shown, 30);
});

test("a failed PUT stays staged, with why; the rest go", async () => {
  const s = stage(stage({}, layers, A, "stall_minutes", 60), layers, B, "agents_model", "opus");
  const sent = [];
  const r = await send(writes(s, "team_1", [A, B]), async (w) => {
    sent.push(w.scope);
    if (w.scope === A) throw new Error("Viewers can't change it");
  });
  assert.deepEqual(sent, [A, B]);
  assert.deepEqual(r.left, { [`${A}/stall_minutes`]: 60 });
  assert.equal(r.failed[0].error, "Viewers can't change it");
});

test("the Overrides line says whose value it is", () => {
  const [a, b] = [layers.projects[A], layers.projects[B]];
  assert.deepEqual(overrideLine(a[1], "agents_model", "Acme"), { overridden: true, text: "Overridden: Sonnet" });
  assert.deepEqual(overrideLine(a[0], "issue_home", "Acme"), { overridden: false, text: "Using team default: Here, on Arugula" });
  assert.equal(overrideLine(a[0], "issue_home", null).text, "Using your default: Here, on Arugula");
  assert.equal(overrideLine(a[3], "stall_minutes", "Acme").text, "Using the default: 30 minutes");
  assert.equal(overrideLine(b[2], "agents_workspace", "Acme", (id) => (id === "hw_1" ? "big box" : undefined)).text, "Overridden: big box");
});

test("values in words, and from a select", () => {
  assert.equal(words("stall_minutes", 120), "2 hours");
  assert.equal(words("stall_minutes", 60), "1 hour");
  assert.equal(words("stall_minutes", 45), "45 minutes");
  assert.equal(words("agents_model", null), "The strongest");
  assert.equal(valueOf("stall_minutes", "60"), 60);
  assert.equal(valueOf("issue_home", ""), null);
});
