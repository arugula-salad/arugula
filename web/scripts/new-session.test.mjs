// A new session starts in the app page's project (Arugula #1163): the
// create call carries it. Run with `node --experimental-strip-types`.
import { test, afterEach } from "node:test";
import assert from "node:assert/strict";
import { newSessionsIn, withProject } from "../src/shell/new-session.ts";

const X = { id: "prj_8f2ec0ffee00112233445566778899aa", name: "Rhythm" };
const Y = { id: "prj_00112233445566778899aabbccddeeff", name: "Acme" };
const bare = { op: "new_session", name: null, from_pane: 3 };

afterEach(() => newSessionsIn(null));

test("on a project's machine page, a new session carries the project", () => {
  newSessionsIn(X);
  assert.deepEqual(withProject(bare), { ...bare, project: X });
  assert.equal(bare.project, undefined, "the caller's intent isn't changed");
});

test("one that names its own project keeps it", () => {
  newSessionsIn(X);
  const own = { ...bare, project: Y };
  assert.equal(withProject(own), own);
});

test("with no project page (the daemon's own), it goes as it was", () => {
  assert.equal(withProject(bare), bare);
  newSessionsIn(X);
  newSessionsIn(null);
  assert.equal(withProject(bare), bare);
});

test("only a new session takes it", () => {
  newSessionsIn(X);
  for (const i of [
    { op: "new_tab", session: 1, from_pane: null },
    { op: "split", pane: 1, edge: "right" },
    { op: "set_session_project", session: 1, project: null },
  ])
    assert.equal(withProject(i), i);
});
