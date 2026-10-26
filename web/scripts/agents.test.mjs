// Agents in conversations (M197): how an agent's message is labelled, the
// line a room shows when an agent grant reaches it, and what the settings'
// switches read of the grants. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { agentLines, agentOwners, authorLabel, chipLabel, isSystemLine } from "../src/shared/chat/agents.ts";
import {
  cellKey,
  cellOf,
  COLUMNS,
  COMMAND_POWERS,
  deployGrant,
  liveOver,
  machinesOf,
  matrixOf,
  powersFor,
  reaching,
  RUN_POWERS,
} from "../src/platform/settings/grants.ts";

const msg = (over) => ({ author: "a1", name: "Jake", agent: null, ...over });

test("an agent's message is its owner's agent's, on its machine", () => {
  assert.equal(authorLabel(msg({})), "Jake");
  assert.equal(authorLabel(msg({ agent: { machine: "d1", machine_name: "geek" } })), "Jake's agent on geek");
  assert.equal(authorLabel(msg({ agent: { machine: "d1", machine_name: "" } })), "Jake's agent on a machine that left");
  assert.equal(authorLabel(msg({ author: "", name: "" })), "a deleted account");
});

test("a room says whose agents reach it, and what they may do", () => {
  const a = (owner, owner_name, machine_name, powers) => ({ owner, owner_name, machine: machine_name, machine_name, powers });
  assert.deepEqual(agentLines([]), []);
  assert.deepEqual(agentLines([a("a1", "Jake", "geek", ["threads.post", "threads.read"])]), [
    "Jake's agents on geek can read and post here",
  ]);
  assert.deepEqual(
    agentLines([
      a("a1", "Jake", "geek", ["threads.post", "threads.read"]),
      a("a1", "Jake", "mini", ["threads.post", "threads.read"]),
      a("a1", "Jake", "box", ["threads.read"]),
      a("a2", "Ana", "lap", ["threads.post"]),
    ]),
    ["Jake's agents on geek and mini can read and post here", "Jake's agents on box can read here", "Ana's agents on lap can post here"],
  );
  // On a native issue's thread, an agent that may only write issues says so.
  assert.deepEqual(
    agentLines([a("a1", "Jake", "geek", ["threads.post", "threads.read"]), a("a1", "Jake", "mini", ["issues.write"])]),
    ["Jake's agents on geek can read and post here", "Jake's agents on mini can edit this issue here"],
  );
});

test("the switches read a machine's live agent grants over one scope", () => {
  const g = (id, machine, power, kind, scope, live = true) => ({
    id,
    subject: { kind: "daemon", id: machine, name: machine === "d1" ? "geek" : null },
    power,
    scope: { kind, id: scope },
    live,
  });
  const grants = [
    g("g1", "d1", "cloud.drive", "account", "a1"),
    g("g2", "d1", "threads.read", "project", "prj_1"),
    g("g3", "d1", "threads.post", "project", "prj_1", false),
    g("g4", "d1", "threads.read", "account", "a1"),
    g("g5", "d2", "threads.read", "project", "prj_1"),
  ];
  const ids = (l) => l.map((x) => x.id);
  assert.deepEqual(ids(liveOver(grants, "d1", { kind: "project", id: "prj_1", name: "" })), ["g2"]);
  assert.deepEqual(ids(liveOver(grants, "d1", { kind: "account", name: "" })), ["g4"], "not cloud.drive");
  assert.deepEqual(ids(liveOver(grants, "d1", { kind: "project", id: "prj_2", name: "" })), []);
  assert.deepEqual(machinesOf(grants), [{ id: "d2", name: "d2" }, { id: "d1", name: "geek" }]);
  assert.deepEqual(powersFor("account"), ["threads.read", "threads.post"]);
  assert.ok(powersFor("project").includes("issues.write"));
});

test("deploys.plan has its own switch (M218), never the conversation one's", () => {
  const g = (id, power, scope, live = true) => ({ id, subject: { kind: "daemon", id: "d1", name: "geek" }, power, scope: { kind: "project", id: scope }, live });
  const grants = [g("g1", "threads.read", "prj_1"), g("g2", "deploys.plan", "prj_1"), g("g3", "deploys.plan", "prj_2", false)];
  assert.deepEqual(liveOver(grants, "d1", { kind: "project", id: "prj_1", name: "" }).map((x) => x.id), ["g1"]);
  assert.equal(deployGrant(grants, "d1", "prj_1")?.id, "g2");
  assert.equal(deployGrant(grants, "d1", "prj_2"), undefined, "a revoked one is off");
  assert.ok(!powersFor("project").includes("deploys.plan"), "turning conversations on doesn't let agents plan deploys");
});

test("the runs and commands switches read only their own grants, and the conversations switch none of them", () => {
  const g = (id, power, live = true) => ({
    id,
    subject: { kind: "daemon", id: "d1", name: "geek" },
    power,
    scope: { kind: "project", id: "prj_1" },
    live,
  });
  const grants = [g("g1", "threads.read"), g("g2", "runs.read"), g("g3", "runs.start", false), g("g4", "runs.command")];
  const p = { kind: "project", id: "prj_1", name: "" };
  const ids = (l) => l.map((x) => x.id);
  assert.deepEqual(ids(liveOver(grants, "d1", p)), ["g1"]);
  assert.deepEqual(ids(liveOver(grants, "d1", p, RUN_POWERS)), ["g2"]);
  assert.deepEqual(ids(liveOver(grants, "d1", p, COMMAND_POWERS)), ["g4"]);
  assert.deepEqual(RUN_POWERS, ["runs.read", "runs.start"]);
});

test("boards grants (given by `arugula grants add`) are not the conversations switch's, so turning it off leaves them", () => {
  const g = (id, power) => ({ id, subject: { kind: "daemon", id: "d1", name: "geek" }, power, scope: { kind: "project", id: "prj_1" }, live: true });
  const grants = [g("g1", "threads.read"), g("g2", "boards.read"), g("g3", "boards.write")];
  assert.deepEqual(liveOver(grants, "d1", { kind: "project", id: "prj_1", name: "" }).map((x) => x.id), ["g1"]);
});

test("a project's matrix on /settings: a row per machine, a switch per column, partial grants said", () => {
  const g = (id, machine, power, kind = "project", scope = "prj_1", live = true) => ({
    id,
    subject: { kind: "daemon", id: machine, name: machine },
    power,
    scope: { kind, id: scope },
    live,
  });
  const grants = [
    g("g1", "d1", "threads.read"),
    g("g2", "d1", "threads.post"),
    g("g3", "d1", "issues.write"),
    g("g4", "d1", "runs.read"),
    g("g5", "d1", "deploys.plan"),
    g("g6", "d2", "threads.read"),
    g("g7", "d2", "runs.command"),
    g("g8", "d2", "runs.start", "project", "prj_1", false),
    g("g9", "d2", "threads.read", "account", "a1"),
    g("g10", "d2", "threads.post", "account", "a1"),
    g("g11", "d3", "cloud.drive", "account", "a1"),
  ];
  const p = { kind: "project", id: "prj_1", name: "Acme" };
  assert.deepEqual(COLUMNS, ["threads", "runs", "commands", "deploys"]);
  // All three conversation powers is on; one of the two runs powers is partial.
  assert.deepEqual(cellOf(grants, "d1", p, "threads"), { on: true, only: null });
  assert.deepEqual(cellOf(grants, "d1", p, "runs"), { on: true, only: ["runs.read"] });
  assert.deepEqual(cellOf(grants, "d1", p, "commands"), { on: false, only: null });
  assert.deepEqual(cellOf(grants, "d1", p, "deploys"), { on: true, only: null });
  assert.deepEqual(cellOf(grants, "d2", p, "threads"), { on: true, only: ["threads.read"] });
  assert.deepEqual(cellOf(grants, "d2", p, "runs"), { on: false, only: null }, "a revoked runs.start is off");
  assert.deepEqual(cellOf(grants, "d2", p, "commands"), { on: true, only: null });
  // Your DMs: read and post is all there is.
  const dms = { kind: "account", name: "Your DMs" };
  assert.deepEqual(cellOf(grants, "d2", dms, "threads"), { on: true, only: null });
  assert.deepEqual(cellOf(grants, "d3", dms, "threads"), { on: false, only: null }, "cloud.drive isn't a DM switch");

  const machines = machinesOf(grants);
  const rows = matrixOf(grants, machines, p, COLUMNS);
  assert.deepEqual(
    rows.map((r) => r.machine.id),
    ["d1", "d2", "d3"],
  );
  assert.deepEqual(
    rows[2].cells.map((c) => c.on),
    [false, false, false, false],
  );
  assert.equal(reaching(rows), 2, "d3 has nothing on in prj_1");
  assert.equal(reaching(matrixOf(grants, machines, dms, ["threads"])), 1);
  assert.equal(reaching(matrixOf(grants, machines, { kind: "project", id: "prj_2", name: "" }, COLUMNS)), 0);

  assert.equal(cellKey("d1", p, "deploys"), "d1/prj_1/deploys");
  assert.equal(cellKey("d1", dms, "threads"), "d1/dms/threads");
});

test("the header chip counts machines and the popover lists them all", () => {
  const a = (owner, owner_name, machine, powers) => ({ owner, owner_name, machine, machine_name: `${machine}.local`, powers });
  const agents = [
    a("a2", "Alec", "m2", ["threads.read", "threads.post"]),
    a("a1", "Jake", "m1", ["threads.read"]),
    a("a2", "Alec", "m3", ["threads.read"]),
    a("a1", "Jake", "m0", ["issues.write"]),
  ];
  assert.equal(chipLabel(agents), "4 machines' agents");
  assert.equal(chipLabel(agents.slice(1, 2)), "1 machine's agents");
  const owners = agentOwners(agents);
  assert.deepEqual(
    owners.map((o) => [o.name, o.machines.map((m) => `${m.name}: ${m.does}`)]),
    [
      ["Alec", ["m2.local: read and post", "m3.local: read"]],
      ["Jake", ["m0.local: edit this issue", "m1.local: read"]],
    ],
  );
  // A grant with no power this room names still shows: nothing is hidden.
  assert.equal(agentOwners([a("a1", "Jake", "m9", [])])[0].machines[0].does, "nothing here");
});

test("a line the platform wrote isn't a person's", () => {
  const line = msg({ author: "", name: "", via: "system", text: "Alec's agents on geek can now read here" });
  assert.equal(isSystemLine(line), true);
  assert.equal(authorLabel(line), "Arugula");
  assert.equal(isSystemLine(msg({ via: "notes" })), false);
  assert.equal(isSystemLine(msg({ author: "", name: "" })), false);
});
