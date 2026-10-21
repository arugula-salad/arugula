// Agents in conversations (M197): how an agent's message is labelled, the
// line a room shows when an agent grant reaches it, and what the settings'
// switches read of the grants. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { agentLines, authorLabel } from "../src/shared/chat/agents.ts";
import { liveOver, machinesOf, powersFor } from "../src/platform/settings/grants.ts";

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
