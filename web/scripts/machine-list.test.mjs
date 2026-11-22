// The Machines page's model (Arugula #1346): each machine's sessions in the
// project, each with its tabs and panes; the machines' order (this computer,
// online, yours, the one you used last, name); the quiet ones; pins first;
// and the stats line. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { machineSections, pinnedCards, statsLine } from "../src/app/machine-list.ts";

const site = { id: "p1", name: "Site" };
const app = { id: "p2", name: "App" };

/** A host whose summary has `sessions`, each `[id, project, [[tab, [pane, ...]], ...]]`. */
function host(name, sessions = [], o = {}) {
  const tabs = sessions.flatMap(([, , ts]) => ts.map(([id, panes, tabName = null]) => ({ id, name: tabName, layout: { panes: panes.map((p) => [p, {}]) } })));
  const summary = { sessions: sessions.map(([id, project, ts]) => ({ id, name: `s${id}`, tabs: ts.map(([t]) => t), ...(project ? { project } : {}) })), tabs, panes: [] };
  return { name, id: `${name}0000`, transport: "relay", state: "connected", lastSeen: null, summary, path: null, ...o };
}
const pane = (h, id, info = {}) => ({ key: `${h}:${id}`, host: h, id, info: { type: "terminal", attention: "idle", ...info }, session: null, stale: false, person: { kind: "me" }, driver: null, watchers: [], unread: 0, mention: false });

test("a machine's sessions, each its tabs and their panes in layout order; needs you first", () => {
  const h = host("mini", [
    [1, site, [[10, [3, 1]], [11, [4], "logs"]]],
    [2, site, [[12, [5]]]],
  ]);
  const panes = [pane("mini", 1), pane("mini", 3), pane("mini", 4), pane("mini", 5, { attention: "needs_input" })];
  const { machines } = machineSections([h], panes);
  assert.deepEqual(machines[0].sessions.map((s) => [s.id, s.state, s.panes]), [
    [2, "needs_input", 1],
    [1, "idle", 3],
  ]);
  const s1 = machines[0].sessions[1];
  assert.deepEqual(s1.tabs.map((t) => [t.id, t.name, t.panes.map((p) => p.id)]), [
    [10, null, [3, 1]],
    [11, "logs", [4]],
  ]);
});

test("another project's session isn't there; a pane the fleet doesn't have (private) isn't; a session with none left isn't", () => {
  const h = host("mini", [
    [1, site, [[10, [1]]]],
    [2, app, [[11, [2]]]],
    [3, site, [[12, [9]]]],
  ]);
  const seen = [];
  const { machines } = machineSections([h], [pane("mini", 1), pane("mini", 2)], { shows: (p, yours) => (seen.push(yours), p?.id === "p1") });
  assert.deepEqual(machines[0].sessions.map((s) => s.id), [1]);
  assert.deepEqual([...new Set(seen)], [true]);
});

test("Agents only keeps the agents' panes, and the sessions with one", () => {
  const h = host("mini", [
    [1, null, [[10, [1, 2]]]],
    [2, null, [[11, [3]]]],
  ]);
  const panes = [pane("mini", 1, { type: "agent" }), pane("mini", 2), pane("mini", 3)];
  const { machines, quiet } = machineSections([h], panes, { only: (p) => p.info.type === "agent" });
  assert.deepEqual(machines[0].sessions.map((s) => [s.id, s.panes]), [[1, 1]]);
  assert.deepEqual(quiet, []);
});

test("this computer first, then online before offline, yours before teammates', then name; quiet ones apart", () => {
  const s = (id) => [[id, null, [[id * 10, [id]]]]];
  const hosts = [
    host("zed", s(1)),
    host("amy", s(2), { state: "offline", status: "offline" }),
    host("bob", s(3), { owner: "Bob" }),
    host("cat", s(4)),
    host("dan", s(5), { owner: "Dan", state: "offline", status: "offline" }),
    host("here", s(6), { state: "offline", status: "offline" }),
    host("idle", []),
    host("eve", [], { owner: "Eve" }),
  ];
  const panes = [1, 2, 3, 4, 5, 6].map((id) => pane(hosts[id - 1].name, id));
  const { machines, quiet } = machineSections(hosts, panes, { here: "here", keep: (h) => !h.owner });
  assert.deepEqual(machines.map((m) => m.name), ["here", "cat", "zed", "bob", "amy", "dan"]);
  assert.equal(machines[0].here, true);
  assert.deepEqual(quiet.map((m) => m.name), ["idle"]);
});

test("with no this computer, the machine you used last leads yours", () => {
  const s = (id) => [[id, null, [[id * 10, [id]]]]];
  const hosts = [host("amy", s(1)), host("bob", s(2)), host("cat", s(3), { owner: "Cat" })];
  const panes = [pane("amy", 1), pane("bob", 2), pane("cat", 3)];
  assert.deepEqual(machineSections(hosts, panes, { recent: "bob0000" }).machines.map((m) => m.name), ["bob", "amy", "cat"]);
  // A teammate's machine you used last stays among teammates'.
  assert.deepEqual(machineSections(hosts, panes, { recent: "cat0000" }).machines.map((m) => m.name), ["amy", "bob", "cat"]);
});

test("pinned sessions first, in pin order, and not again under their machine", () => {
  const hosts = [host("amy", [[1, null, [[10, [1]]]], [2, null, [[11, [2]]]]]), host("bob", [[1, null, [[10, [1]]]]])];
  const panes = [pane("amy", 1), pane("amy", 2), pane("bob", 1)];
  const { machines } = machineSections(hosts, panes);
  const pins = [
    { machine: "bob0000", session: 1 },
    { machine: "amy0000", session: 2 },
    { machine: "amy0000", session: 7 },
  ];
  const r = pinnedCards(machines, pins, (m) => m.id);
  assert.deepEqual(r.pinned.map((p) => [p.machine.name, p.session.id]), [
    ["bob", 1],
    ["amy", 2],
  ]);
  assert.deepEqual(r.machines.map((m) => [m.name, m.sessions.map((s) => s.id)]), [["amy", [1]]]);
  assert.deepEqual(pinnedCards(machines, [], (m) => m.id).machines, machines);
});

test("the stats line", () => {
  const h = host("mini", [[1, null, [[10, [1, 2]]]], [2, null, [[11, [3]]]]]);
  const { machines } = machineSections([h], [pane("mini", 1, { attention: "needs_input" }), pane("mini", 2), pane("mini", 3)]);
  assert.equal(statsLine(1, machines[0].sessions), "1 machine · 2 sessions · 3 panes · 1 needs you");
  assert.equal(statsLine(2, []), "2 machines · 0 sessions · 0 panes");
});
