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

// Chat's own choices (Arugula #882).
const { chatProject, rememberTeamProject, lastTeamProject, noteRoute, arrival, dismissArrival, leaveChat } = await import("../src/platform/current-project.ts");

const T1 = "prj_1111111111111111111111111111111a";
const T2 = "prj_2222222222222222222222222222222b";
const projects = [{ id: Q }, { id: T1, team: {} }, { id: T2, team: {} }];

test("/chat with no project opens the current team's, else the last team's, else the first", () => {
  assert.equal(chatProject(projects, T2, null), T2);
  assert.equal(chatProject(projects, Q, T2), T2, "Personal: the last current team");
  assert.equal(chatProject(projects, null, T2), T2);
  assert.equal(chatProject(projects, Q, null), T1, "no history: the first team, as before");
  assert.equal(chatProject(projects, "prj_gone", "prj_also_gone"), T1);
  assert.equal(chatProject([{ id: Q }], Q, null), undefined);
});

test("the last current team is kept, and Personal doesn't replace it", () => {
  setCurrentProject(T2);
  rememberTeamProject(projects);
  setCurrentProject(Q);
  rememberTeamProject(projects);
  assert.equal(lastTeamProject(), T2);
  assert.equal(chatProject(projects, currentProject(), lastTeamProject()), T2);
});

test("a room link across projects is an arrival that names where they were", () => {
  noteRoute(null, { chat: false, room: false }, false);
  assert.equal(arrival(), null);
  noteRoute(T1, { project: T2, chat: true, room: true }, false);
  assert.deepEqual(arrival(), { from: T1, to: T2 });
  // The same project's rooms, and its list, keep it; another place ends it.
  noteRoute(T2, { project: T2, chat: true, room: true }, false);
  noteRoute(T2, { project: T2, chat: true, room: false }, false);
  assert.deepEqual(arrival(), { from: T1, to: T2 });
  noteRoute(T2, { project: T2, chat: false, room: false }, false);
  assert.equal(arrival(), null);
});

test("no arrival for a room in the current project, nor from Back", () => {
  noteRoute(T1, { project: T1, chat: true, room: true }, false);
  assert.equal(arrival(), null);
  noteRoute(null, { project: T1, chat: true, room: true }, false);
  assert.equal(arrival(), null);
  noteRoute(T2, { project: T1, chat: true, room: true }, true);
  assert.equal(arrival(), null);
  // A chat list in another project (the picker) isn't a room link.
  noteRoute(T2, { project: T1, chat: true, room: false }, false);
  assert.equal(arrival(), null);
});

test("a stale link's second hop keeps the first origin; dismissing ends it", () => {
  noteRoute(Q, { project: T1, chat: true, room: true }, false);
  noteRoute(T1, { project: T2, chat: true, room: true }, false);
  assert.deepEqual(arrival(), { from: Q, to: T2 });
  dismissArrival();
  assert.equal(arrival(), null);
});

test("leaving chat goes to the project you came from, else this one's: never the list", () => {
  const known = (id) => id !== "prj_gone";
  const across = { from: T1, to: T2 };
  assert.deepEqual(leaveChat(T2, across, known, true), { project: T1, back: false });
  assert.deepEqual(leaveChat(T2, null, known, false), { project: T2, back: false });
  assert.deepEqual(leaveChat(T2, null, known, true), { project: T2, back: true });
  // The origin is gone: this project's. An arrival of another project doesn't apply.
  assert.deepEqual(leaveChat(T2, { from: "prj_gone", to: T2 }, known, false), { project: T2, back: false });
  assert.deepEqual(leaveChat(T1, across, known, false), { project: T1, back: false });
});
