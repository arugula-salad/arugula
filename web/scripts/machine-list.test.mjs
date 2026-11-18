// The Machines list's rows (Arugula #990): the order (needs you, then
// online, then name), what you may do on each machine, and the words for a
// machine's sessions. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { paneGroups, roleOf, rowsOf, sessionsLine, sortRows } from "../src/app/machine-list.ts";

const row = (name, o = {}) => ({ name, online: true, cloud: false, role: "owner", sessions: 0, needsYou: 0, ...o });

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
  assert.equal(sessionsLine({ sessions: 0, needsYou: 0 }, "Site"), "No Site sessions yet.");
  assert.equal(sessionsLine({ sessions: 2, needsYou: 0 }, "Site"), "2 sessions");
});

test("rows from the fleet: sessions, those asking, cloud and online", () => {
  const sessions = [{ id: 1 }, { id: 2 }, { id: 3 }];
  const host = { name: "mini", id: "ab12cd", status: "online", transport: "control", state: "connected", summary: { sessions } };
  const cloud = { name: "ws", id: "ee99ff", status: "offline", transport: "control", state: "offline", hosted: true, summary: null };
  const ask = { host: "mini", session: { id: 2, name: "b" }, info: { attention: "needs_input", reason: null, session_project: { id: "p2", name: "App" } } };
  const quiet = { host: "mini", session: { id: 1, name: "a" }, info: { attention: "idle", reason: null, session_project: { id: "p1", name: "Site" } } };
  const elsewhere = { host: "other", session: { id: 1, name: "a" }, info: { attention: "needs_input", reason: null, session_project: { id: "p9", name: "Nope" } } };
  const [a, b] = rowsOf([host, cloud], [ask, ask, quiet, elsewhere], (id) => (id === "ee99ff" ? "editor" : undefined));
  assert.deepEqual([a.sessions, a.needsYou, a.online, a.cloud, a.role], [3, 1, true, false, "owner"]);
  assert.deepEqual([b.sessions, b.online, b.cloud, b.role], [null, false, true, "editor"]);
});

test("rows count only the sessions shown: one project's, and what needs you in it (#1164)", () => {
  const site = { id: "p1", name: "Site" };
  const app = { id: "p2", name: "App" };
  const sessions = [{ id: 1, project: site }, { id: 2, project: app }, { id: 3 }];
  const host = { name: "mini", id: "ab12cd", status: "online", transport: "control", state: "connected", summary: { sessions } };
  const ask = { host: "mini", session: { id: 2, name: "b" }, info: { attention: "needs_input", reason: null, session_project: app } };
  const shows = (p) => p?.id === "p1";
  const [a] = rowsOf([host], [ask], () => undefined, shows);
  assert.deepEqual([a.sessions, a.needsYou], [1, 0]);
});

test("Build's panes: by machine, what needs you first; a machine filters; an editor that only joined isn't one (#1083)", () => {
  const host = (name, o = {}) => ({ name, id: `${name}0000`, transport: "relay", state: "connected", lastSeen: null, summary: null, path: null, ...o });
  const pane = (h, id, o = {}) => ({ key: `${h}:${id}`, host: h, id, info: { type: "terminal", ...o.info }, session: { id: o.session ?? 1, name: "main" }, stale: false, person: { kind: "me" }, driver: null, watchers: [], unread: 0, mention: false, ...o.extra });
  const hosts = [host("zed"), host("amy", { state: "offline", status: "offline" }), host("bob"), host("cat")];
  const panes = [pane("bob", 3), pane("bob", 1, { session: 2 }), pane("cat", 5), pane("cat", 2, { info: { attention: "needs_input" } }), pane("zed", 1, { extra: { session: null }, info: { type: "editor" } })];
  const g = paneGroups(hosts, panes);
  assert.deepEqual(g.map((x) => x.name), ["cat", "bob", "zed", "amy"]);
  assert.deepEqual(g[0].panes.map((p) => p.id), [2, 5]);
  assert.deepEqual(g[1].panes.map((p) => p.id), [3, 1]);
  assert.deepEqual(g[2].panes, []);
  assert.equal(g[3].online, false);
  assert.deepEqual(paneGroups(hosts, panes, "bob0000").map((x) => x.name), ["bob"]);
  assert.deepEqual(paneGroups([host("solo", { id: undefined })], [], "solo").map((x) => x.name), ["solo"]);
  // Another project's pane isn't there at all (#1164); a teammate's machine says it isn't yours.
  const site = { id: "p1", name: "Site" };
  const ours = [pane("bob", 3, { info: { session_project: site } }), pane("bob", 1, { session: 2, info: { session_project: { id: "p2", name: "App" } } })];
  const seen = [];
  const only = paneGroups([host("bob", { owner: "Bob" })], ours, undefined, (p, yours) => (seen.push(yours), p?.id === "p1"));
  assert.deepEqual(only[0].panes.map((p) => p.id), [3]);
  assert.deepEqual([...new Set(seen)], [false]);
});
