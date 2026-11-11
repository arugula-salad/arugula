// The current project (Arugula #880): set from a project's page, kept in the
// browser, forgotten when stale, and what each entry point opens for it. Run
// with `node --experimental-strip-types`.
import { test, beforeEach } from "node:test";
import assert from "node:assert/strict";

const KEY = "arugula.platform-active-project";
const P = "prj_8f2ec0ffee00112233445566778899aa";
const Q = "prj_00112233445566778899aabbccddeeff";

let kept;
const fake = {
  getItem: (k) => (k in kept ? kept[k] : null),
  setItem: (k, v) => void (kept[k] = String(v)),
  removeItem: (k) => void delete kept[k],
};
beforeEach(() => {
  kept = {};
  globalThis.localStorage = fake;
});

const { currentProject, setCurrentProject, forgetCurrentProject, onCurrentProject, entryPath } = await import("../src/platform/current-project.ts");

test("a project's page makes it current, under the key the rail used", () => {
  assert.equal(currentProject(), null);
  setCurrentProject(P);
  assert.equal(currentProject(), P);
  assert.equal(kept[KEY], P);
});

test("it's remembered by the next page, and the latest project wins", () => {
  kept[KEY] = Q;
  assert.equal(currentProject(), Q);
  setCurrentProject(P);
  assert.equal(currentProject(), P);
});

test("a stale one is forgotten, and listeners hear each change once", () => {
  let heard = 0;
  const off = onCurrentProject(() => heard++);
  setCurrentProject(P);
  setCurrentProject(P);
  assert.equal(heard, 1);
  forgetCurrentProject();
  forgetCurrentProject();
  assert.equal(heard, 2);
  assert.equal(currentProject(), null);
  assert.equal(KEY in kept, false);
  off();
  setCurrentProject(Q);
  assert.equal(heard, 2);
});

test("storage that throws leaves it for the life of the page", () => {
  const boom = () => {
    throw new Error("denied");
  };
  globalThis.localStorage = { getItem: boom, setItem: boom, removeItem: boom };
  setCurrentProject(P);
  assert.equal(currentProject(), P);
  forgetCurrentProject();
  assert.equal(currentProject(), null);
});

test("each entry point opens the current project's page, or the global one without", () => {
  const team = { id: P, team: { id: "team_1" } };
  const personal = { id: Q };
  assert.equal(entryPath("projects", team), `/p/${P}`);
  assert.equal(entryPath("docs", team), `/p/${P}/docs`);
  assert.equal(entryPath("boards", team), `/p/${P}/boards`);
  assert.equal(entryPath("chat", team), `/p/${P}/chat`);
  // Personal has no team chat: the chat page, as before.
  assert.equal(entryPath("chat", personal), "/chat");
  assert.equal(entryPath("docs", personal), `/p/${Q}/docs`);
  for (const [entry, path] of [["projects", "/"], ["docs", "/docs"], ["boards", "/boards"], ["chat", "/chat"]]) {
    assert.equal(entryPath(entry, undefined), path, entry);
  }
});
