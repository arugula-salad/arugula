// The project the machine view shows (Arugula #1164): which sessions show
// in it, and which project a link to a session switches to. Run with
// `node --experimental-strip-types`.
import { test, afterEach } from "node:test";
import assert from "node:assert/strict";
import { inShown, onShownProject, projectOf, showProject, shownProject } from "../src/shell/project-view.ts";

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
