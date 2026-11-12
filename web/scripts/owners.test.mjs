// Whose each project is (Arugula #895): the switcher's owners, Personal
// first, each one's projects with the archived ones apart, and who may
// make one where. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { makers, ownerKey, ownerNamed, owners, PERSONAL } from "../src/platform/owners.ts";

const roster = [{ account: "a1", name: "Aaron", role: "owner" }];
const acme = { id: "tm_acme", name: "Acme", role: "owner", members: roster };
const beta = { id: "tm_beta", name: "Beta", role: "viewer", members: [] };

const mine = { id: "prj_1", name: "Notes", team: null, owner: { kind: "personal", account: "a1" } };
const acmeHome = { id: "prj_2", name: "Acme", team: acme, owner: { kind: "team", id: "tm_acme", name: "Acme" } };
// A team's other project carries no `team`: its owner says whose it is.
const acmeWeb = { id: "prj_3", name: "Web", team: null, owner: { kind: "team", id: "tm_acme", name: "Acme" } };
const acmeOld = { id: "prj_4", name: "Old site", team: null, owner: { kind: "team", id: "tm_acme", name: "Acme" }, archived: true };
const betaHome = { id: "prj_5", name: "Beta", team: beta };
const mineOld = { id: "prj_6", name: "Scratch", team: null, archived: true };

test("an older control's project goes by its team", () => {
  assert.equal(ownerKey({ team: null }), PERSONAL);
  assert.equal(ownerKey({ team: beta }), "tm_beta");
  assert.equal(ownerKey({ team: null, owner: { kind: "team", id: "tm_acme", name: "Acme" } }), "tm_acme");
  assert.equal(ownerKey({ team: acme, owner: { kind: "personal", account: "a1" } }), PERSONAL, "the owner wins");
});

test("Personal comes first, then each team in the order its first project came", () => {
  const all = owners([acmeWeb, betaHome, mine, acmeHome]);
  assert.deepEqual(
    all.map((o) => [o.key, o.kind, o.name]),
    [
      [PERSONAL, "personal", "Personal"],
      ["tm_acme", "team", "Acme"],
      ["tm_beta", "team", "Beta"],
    ],
  );
  assert.deepEqual(
    all[1].projects.map((p) => p.id),
    ["prj_3", "prj_2"],
    "a team's projects in the order they came",
  );
});

test("Personal is there with no project of its own", () => {
  const all = owners([betaHome]);
  assert.equal(all[0].key, PERSONAL);
  assert.deepEqual(all[0].projects, []);
  assert.deepEqual(owners([]).map((o) => o.key), [PERSONAL]);
});

test("archived projects are kept apart", () => {
  const all = owners([mine, mineOld, acmeHome, acmeWeb, acmeOld]);
  assert.deepEqual(
    all[0].projects.map((p) => p.id),
    ["prj_1"],
  );
  assert.deepEqual(
    all[0].archived.map((p) => p.id),
    ["prj_6"],
  );
  assert.deepEqual(
    all[1].projects.map((p) => p.id),
    ["prj_2", "prj_3"],
  );
  assert.deepEqual(
    all[1].archived.map((p) => p.id),
    ["prj_4"],
  );
  // A team whose projects are all archived is still an owner.
  const only = owners([acmeOld]);
  assert.deepEqual(
    only.map((o) => [o.key, o.projects.length, o.archived.length]),
    [
      [PERSONAL, 0, 0],
      ["tm_acme", 0, 1],
    ],
  );
});

test("a team's role and roster come from the project that carries its team", () => {
  const acmeOwner = ownerNamed([acmeWeb, acmeHome], "tm_acme");
  assert.equal(acmeOwner.role, "owner");
  assert.deepEqual(acmeOwner.members, roster);
  // None of its projects says: no role, so nothing is offered.
  assert.equal(ownerNamed([acmeWeb], "tm_acme").role, "");
  assert.equal(ownerNamed([acmeWeb], "tm_nope"), undefined);
  assert.equal(ownerNamed([acmeWeb], null), undefined);
});

test("new projects are made, and moved, only where the reader owns", () => {
  const all = owners([mine, acmeHome, betaHome, acmeWeb]);
  assert.deepEqual(
    makers(all).map((o) => o.key),
    [PERSONAL, "tm_acme"],
  );
});
