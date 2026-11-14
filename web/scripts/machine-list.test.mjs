// The Machines list's rows (Arugula #990): the order (needs you, then
// online, then name), what you may do on each machine, and the words for a
// machine's sessions. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { roleOf, rowsOf, sessionsLine, sortRows } from "../src/app/machine-list.ts";

const row = (name, o = {}) => ({ name, online: true, cloud: false, role: "owner", sessions: 0, needsYou: 0, projects: [], ...o });

test("needs you first, then online, then name", () => {
  const rows = [row("zed"), row("amy", { online: false }), row("bob", { needsYou: 1, online: false }), row("cat"), row("dan", { needsYou: 2 })];
  assert.deepEqual(sortRows(rows).map((r) => r.name), ["dan", "bob", "cat", "zed", "amy"]);
});

test("sorting leaves its input alone", () => {
  const rows = [row("b"), row("a")];
  sortRows(rows);
  assert.deepEqual(rows.map((r) => r.name), ["b", "a"]);
});

test("your own machine is yours; a cloud workspace has the token's role; a teammate's is the most it gives you, else watching", () => {
  assert.equal(roleOf({ mine: true }), "owner");
  assert.equal(roleOf({ mine: false, hosted: "editor" }), "editor");
  assert.equal(roleOf({ mine: false }), "viewer");
  assert.equal(roleOf({ mine: false, roles: ["viewer", "editor"] }), "editor");
});

test("sessions in words", () => {
  assert.equal(sessionsLine({ sessions: null, needsYou: 0 }), "");
  assert.equal(sessionsLine({ sessions: 0, needsYou: 0 }), "No sessions");
  assert.equal(sessionsLine({ sessions: 1, needsYou: 0 }), "1 session");
  assert.equal(sessionsLine({ sessions: 3, needsYou: 1 }), "3 sessions · 1 needs you");
  assert.equal(sessionsLine({ sessions: 4, needsYou: 2 }), "4 sessions · 2 need you");
});

test("rows from the fleet: sessions, those asking, their projects, cloud and online", () => {
  const sessions = [{ id: 1 }, { id: 2 }, { id: 3 }];
  const host = { name: "mini", id: "ab12cd", status: "online", transport: "control", state: "connected", summary: { sessions } };
  const cloud = { name: "ws", id: "ee99ff", status: "offline", transport: "control", state: "offline", hosted: true, summary: null };
  const ask = { host: "mini", session: { id: 2, name: "b" }, info: { attention: "needs_input", reason: null, session_project: { id: "p2", name: "App" } } };
  const quiet = { host: "mini", session: { id: 1, name: "a" }, info: { attention: "idle", reason: null, session_project: { id: "p1", name: "Site" } } };
  const elsewhere = { host: "other", session: { id: 1, name: "a" }, info: { attention: "needs_input", reason: null, session_project: { id: "p9", name: "Nope" } } };
  const [a, b] = rowsOf([host, cloud], [ask, ask, quiet, elsewhere], (id) => (id === "ee99ff" ? "editor" : undefined));
  assert.deepEqual([a.sessions, a.needsYou, a.projects, a.online, a.cloud, a.role], [3, 1, ["App", "Site"], true, false, "owner"]);
  assert.deepEqual([b.sessions, b.online, b.cloud, b.role, b.projects], [null, false, true, "editor", []]);
});
