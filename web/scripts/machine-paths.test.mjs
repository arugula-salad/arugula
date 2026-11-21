// Where the machines are in the app (Arugula #989): each of control's page
// addresses lands on its place in the app, those that stay on control's
// page (secrets, signing in, passkeys) never move, and the app's places go
// back to control's page as the same address while the hub can't reach
// machines from the app's host. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { controlHash, fromControl, machinesPath, parseMachines, popAddress } from "../src/shared/machine-paths.ts";
import { FRAGMENTS } from "../src/shell/fragments.ts";
import { TOP, parse, pathOf } from "../src/platform/paths.ts";

const D = "da5b786733ac68a9";
const P = "prj_3e5d802cb4b08d43074df6c32f5344c1";
const B = "brd_d08cd00433b2b2a052ba37b7899d3785";

test("each of control's page addresses has its place in the app, project-less", () => {
  const rows = [
    ["", "", "/"],
    ["", "?machine=Aarons-Mac-mini", "/machines?machine=Aarons-Mac-mini"],
    [`#pane=${D}.12`, "", `/machines/${D}?pane=12`],
    [`#pane=${D}.12&share`, "", `/machines/${D}?pane=12&share`],
    [`#pane=${D}.12&follow=sam%20k`, "", `/machines/${D}?pane=12&follow=sam+k`],
    [`#pane=${D}.12&thread=pane-3`, "", `/machines/${D}?pane=12`],
    ["#pane=7", "", "/machines?pane=7"],
    [`#machine=${D}`, "", `/machines/${D}`],
    [`#machine=${D}&agent`, "", `/machines/${D}?agent`],
    ["#swarm", "", "/machines/swarm"],
    [`#swarm=${D}.4`, "", `/machines/swarm?focus=${D}.4`],
    ["#swarm=4", "", "/machines/swarm?focus=4"],
    [`#swarm=board:${B}`, "", `/machines/swarm?board=${B}`],
    [`#pop=${D}.9`, "", `/machines/${D}?pane=9&pop`],
    ["#pop=9", "", "/machines?pane=9&pop"],
    [`#project=${P}`, "", `/p/${P}`],
    ["#docs", "", "/docs"],
    ["#overview", "", "/machines#overview"],
    ["#overview=3", "", "/machines#overview=3"],
    ["#agents", "", "/machines#agents"],
    ["#agents=team", "", "/machines#agents=team"],
    ["#settings=machine", "", "/machines#settings=machine"],
    ["#settings=remote-sessions", "", "/machines#settings=remote-sessions"],
    ["#developer", "", "/machines#developer"],
  ];
  for (const [hash, search, path] of rows) assert.equal(fromControl(hash, search), path, `${hash}${search}`);
});

test("what stays on control's page never moves: secrets, signing in, passkeys, approvals, the platform's own", () => {
  const moves = new Set(["#swarm", "#overview", "#agents", "#docs", "#pane=", "#pop=", "#machine=", "#developer"]);
  for (const f of FRAGMENTS) {
    const sample = f.route.endsWith("=") ? `${f.route}${f.route === "#pane=" || f.route === "#pop=" ? `${D}.1` : f.route === "#machine=" ? D : f.route === "#getting-started=" ? "cloud" : "c2VjcmV0.abc"}` : f.route;
    const to = fromControl(sample);
    if (moves.has(f.route)) assert.notEqual(to, null, sample);
    else assert.equal(to, null, `${sample} stays on control's page`);
  }
  // Settings but the machine's (your account, passkeys, devices), and Getting started, stay too.
  for (const h of ["#settings", "#settings=account", "#settings=device", "#settings=all-devices", "#getting-started=cloud"]) assert.equal(fromControl(h), null, h);
  for (const h of ["#chat", "#chat=ch_0123456789abcdef", "#deploy=dep_0123", "#teams", "#lend=on", "#nonsense", "#pane=x", "#machine=../x", `#swarm=board:../x`])
    assert.equal(fromControl(h), null, h);
});

test("the app's places go back to control's page as the same address", () => {
  for (const hash of [`pane=${D}.12`, `pane=${D}.12&share`, `pane=${D}.12&follow=sam`, "pane=7", `machine=${D}`, `machine=${D}&agent`, "swarm", `swarm=${D}.4`, "swarm=4", `swarm=board:${B}`, `pop=${D}.9`, "overview", "settings=machine"]) {
    const path = fromControl(`#${hash}`);
    const url = new URL(path, "https://app.arugula.io");
    const r = parse(url.pathname, url.search);
    assert.equal(r.page, "find", path);
    const at = r.at.at === "machines" ? parseMachines([], url.searchParams, url.hash) : r.at;
    assert.equal(controlHash(at), hash, path);
  }
  assert.equal(controlHash({ at: "machines" }), "");
});

test("the machines live in the project: list, a machine's page with its slug, Swarm", () => {
  assert.ok(TOP.includes("machines"));
  const names = (id) => ({ [D]: "Aarons-Mac-mini", [P]: "Arugula" })[id];
  assert.deepEqual(parse(`/p/${P}/machines`), { page: "machines", project: P, at: { at: "machines" } });
  assert.deepEqual(parse(`/p/${P}-arugula/machines/${D}-an-old-name`, "?pane=3&share&follow=sam"), {
    page: "machines",
    project: P,
    at: { at: "machine", machine: D, pane: 3, share: true, follow: "sam" },
  });
  assert.equal(pathOf({ page: "machines", project: P, at: { at: "machine", machine: D, pane: 3 } }, names), `/p/${P}-arugula/machines/${D}-aarons-mac-mini?pane=3`);
  assert.deepEqual(parse(`/p/${P}/machines/swarm`, `?board=${B}`), { page: "machines", project: P, at: { at: "swarm", board: B } });
  assert.equal(pathOf({ page: "machines", project: P, at: { at: "swarm", focus: { machine: D, pane: 4 } } }), `/p/${P}/machines/swarm?focus=${D}.4`);
  assert.deepEqual(parse(`/machines/${D}`), { page: "find", what: "machines", at: { at: "machine", machine: D } });
  assert.equal(pathOf({ page: "find", what: "machines", at: { at: "swarm" } }), "/machines/swarm");
  // Not a machine's id, or too deep: nothing's address.
  assert.equal(parse(`/p/${P}/machines/not-hex`).page, "missing");
  assert.equal(parse(`/p/${P}/machines/${D}/extra`).page, "missing");
  assert.equal(parse("/machines/swarm", "?board=../x").at.board, undefined);
  // The Overview, by its rail name too.
  assert.deepEqual(parse(`/p/${P}/overview`), { page: "project", project: P });
  assert.equal(machinesPath({ at: "machine", machine: D, pane: 1, pop: true, agent: true }), `/machines/${D}?pane=1&pop&agent`);
  // Home machine popped out (#1032): no daemon to name, and it round-trips.
  assert.equal(machinesPath({ at: "machines", pane: 9, pop: true }), "/machines?pane=9&pop");
  assert.deepEqual(parse("/machines", "?pane=9&pop"), { page: "find", what: "machines", at: { at: "machines", pane: 9, pop: true } });
  assert.equal(parse("/machines", "?pop").at.pop, undefined);
  assert.equal(controlHash({ at: "machines", pane: 9, pop: true }), "pop=9");
});

test("Build's home is every machine's panes, with a machine as its filter (#1083)", () => {
  assert.deepEqual(parse(`/p/${P}/machines/panes`), { page: "machines", project: P, at: { at: "panes" } });
  assert.deepEqual(parse(`/p/${P}/machines/panes`, `?machine=${D}`), { page: "machines", project: P, at: { at: "panes", machine: D } });
  assert.equal(pathOf({ page: "machines", project: P, at: { at: "panes", machine: D } }), `/p/${P}/machines/panes?machine=${D}`);
  assert.deepEqual(parse("/machines/panes"), { page: "find", what: "machines", at: { at: "panes" } });
  // A host with no id, by its name; nothing else.
  assert.deepEqual(parse(`/p/${P}/machines/panes`, "?machine=build-01.local").at, { at: "panes", machine: "build-01.local" });
  assert.deepEqual(parse(`/p/${P}/machines/panes`, "?machine=../x").at, { at: "panes" });
  // Control's page has no such list: the machine's, or its own.
  assert.equal(controlHash({ at: "panes" }), "");
  // Agents only, a flag beside the machine (#1082).
  assert.deepEqual(parse(`/p/${P}/machines/panes`, `?machine=${D}&agents`).at, { at: "panes", machine: D, agents: true });
  assert.equal(pathOf({ page: "machines", project: P, at: { at: "panes", machine: D, agents: true } }), `/p/${P}/machines/panes?machine=${D}&agents`);
  assert.equal(pathOf({ page: "machines", project: P, at: { at: "panes", agents: true } }), `/p/${P}/machines/panes?agents`);
  assert.equal(controlHash({ at: "panes", machine: D }), `machine=${D}`);
});

test("the Machines page keeps Agents only in its address (#1346)", () => {
  assert.deepEqual(parse(`/p/${P}/machines`, "?agents").at, { at: "machines", agents: true });
  assert.equal(pathOf({ page: "machines", project: P, at: { at: "machines", agents: true } }), `/p/${P}/machines?agents`);
  // Only on the page itself: not on a pane's or a machine's by name.
  assert.equal(parse(`/p/${P}/machines`, "?pane=9&agents").at.agents, undefined);
  assert.equal(parse(`/p/${P}/machines`, "?machine=mini&agents").at.agents, undefined);
});

test("Pop out opens the pane alone, in the app and on control's page (#1032)", () => {
  // The app names the daemon, even for the home machine: no overview between.
  assert.equal(popAddress(9, D), `/machines/${D}?pane=9&pop`);
  assert.equal(popAddress(9, undefined), "/machines?pane=9&pop");
  // Control's page keeps its fragment.
  assert.equal(popAddress(9, D, false), `/#pop=${D}.9`);
  assert.equal(popAddress(9, undefined, false), "/#pop=9");
  // What the app opens is what an old `#pop=` becomes there.
  assert.equal(fromControl(`#pop=${D}.9`), popAddress(9, D));
  assert.equal(fromControl("#pop=9"), popAddress(9));
});
