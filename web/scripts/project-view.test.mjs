// The project the machine view shows (Arugula #1164): which sessions show
// in it, and which project a link to a session switches to. Run with
// `node --experimental-strip-types`.
import { test, afterEach } from "node:test";
import assert from "node:assert/strict";
import { inShown, keepLastSession, lastSession, onShownProject, projectOf, showProject, shownProject } from "../src/shell/project-view.ts";

const site = { id: "prj_site", name: "Site" };
const app = { id: "prj_app", name: "App" };
const def = { id: "prj_me-personal", name: "Default", personal: true };

afterEach(() => showProject(null));

test("with no project shown (the daemon's own page), every session shows", () => {
  assert.equal(inShown(site, false), true);
  assert.equal(inShown(undefined, false), true);
});

test("a project shows its own sessions only", () => {
  showProject({ ...site, personal: false });
  assert.equal(inShown(site, true), true);
  assert.equal(inShown(app, true), false);
  assert.equal(inShown(undefined, true), false, "one placed nowhere is your Default's, not this one's");
});

test("a session placed nowhere shows in your Default, on your own machine only", () => {
  showProject(def);
  assert.equal(inShown(undefined, true), true);
  assert.equal(inShown(null, false), false);
  assert.equal(inShown(site, true), false);
});

test("a link to a session goes to its project: its own, else your Default on your machine, else none", () => {
  assert.equal(projectOf(app, false, def.id), app.id);
  assert.equal(projectOf(undefined, true, def.id), def.id);
  assert.equal(projectOf(undefined, false, def.id), null);
});

test("listeners hear a change of project, not the same one again", () => {
  let heard = 0;
  const off = onShownProject(() => heard++);
  showProject({ ...site, personal: false });
  showProject({ ...site, personal: false });
  showProject({ ...app, personal: false });
  off();
  showProject(null);
  assert.equal(heard, 2);
  assert.equal(shownProject(), null);
});

test("the last session in each project is kept per viewer and machine, across a reload", () => {
  const kept = new Map();
  globalThis.localStorage = { getItem: (k) => kept.get(k) ?? null, setItem: (k, v) => void kept.set(k, String(v)) };
  try {
    keepLastSession("acct_a", "e2e:m1", site.id, 4);
    keepLastSession("acct_a", "e2e:m1", app.id, 7);
    assert.equal(lastSession("acct_a", "e2e:m1", site.id), 4);
    assert.equal(lastSession("acct_a", "e2e:m1", app.id), 7);
    assert.equal(lastSession("acct_b", "e2e:m1", site.id), null, "another viewer's isn't yours");
    assert.equal(lastSession("acct_a", "e2e:m2", site.id), null, "nor another machine's");
    kept.set("arugula.last-session:acct_a:e2e:m1", "not json");
    assert.equal(lastSession("acct_a", "e2e:m1", site.id), null);
  } finally {
    delete globalThis.localStorage;
  }
});

test("without storage, nothing is kept and nothing throws", () => {
  keepLastSession("acct_a", "e2e:m1", site.id, 4);
  assert.equal(lastSession("acct_a", "e2e:m1", site.id), null);
});
