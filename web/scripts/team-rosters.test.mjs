// A team's roster as the owner's page takes it (web/src/shell/team-rosters.ts):
// control's latest must follow the last version this browser checked, as a
// team machine's must (`checkpoint::advance` in crates/e2e; e2e-interop.ts
// checks the two walk alike). Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { certBody } from "../src/shell/e2e/cert.ts";
import { generateKeys, signText } from "../src/shell/e2e/keys.ts";
import { signRoster } from "../src/shell/e2e/team.ts";
import { checkRoster } from "../src/shell/team-rosters.ts";

const self = async (k, account) => {
  const c = { v: 1, account, device: k.id, kind: "browser", name: account, noise: k.noisePub, sign: k.signPub, created: 1, approver: k.id, sig: "" };
  c.sig = await signText(k, certBody(c));
  return c;
};

const alice = await generateKeys();
const bob = await generateKeys();
const bob2 = await generateKeys();
const certs = { alice: [[await self(alice, "alice")], []], bob: [[await self(bob, "bob")], []] };
const pin = { team: "0123456789abcdef", founder: "alice", founder_root: alice.id };
const owner = { account: "alice", root: alice.id, role: "owner", name: "alice" };
const base = { v: 1, team: pin.team, name: "Acme", at: 1 };
const v1 = await signRoster({ ...base, version: 1, members: [owner] }, alice);
const v2 = await signRoster({ ...base, version: 2, members: [owner, { account: "bob", root: bob.id, role: "viewer", name: "bob" }] }, alice);
const v3 = await signRoster({ ...base, version: 3, members: [owner, { account: "bob", root: bob2.id, role: "viewer", name: "bob" }] }, alice);
const none = async () => {
  throw new Error("not asked");
};
const serves = (rosters) => async (since) => ({ rosters: rosters.filter((r) => r.version > since), certs: {} });

test("a first look takes control's, as the founder is", async () => {
  assert.deepEqual(await checkRoster(null, v2, pin, certs, none), { roster: v2, problem: null, changes: [] });
});

test("the same version again is taken; another at that version, or an older one, isn't", async () => {
  assert.equal((await checkRoster(v2, v2, pin, certs, none)).problem, null);
  const other2 = await signRoster({ ...base, version: 2, members: [owner, { account: "bob", root: bob.id, role: "owner", name: "bob" }] }, alice);
  assert.deepEqual(await checkRoster(v2, other2, pin, certs, none), { roster: v2, problem: "another", changes: [] });
  assert.deepEqual(await checkRoster(v2, v1, pin, certs, none), { roster: v2, problem: "older", changes: [] });
});

test("the next version is taken when it follows, and a changed root is said", async () => {
  const got = await checkRoster(v2, v3, pin, certs, none);
  assert.equal(got.problem, null);
  assert.equal(got.roster, v3);
  assert.deepEqual(got.changes, [{ account: "bob", name: "bob", from: bob.id, to: bob2.id, version: 3 }]);
});

test("a roster no owner signed isn't built on: control's own member added to a real one", async () => {
  const bent = { ...v2, version: 3, members: [...v2.members, { account: "mallory", root: bob2.id, role: "owner", name: "mallory" }] };
  const got = await checkRoster(v2, bent, pin, certs, serves([v1, v2, v3]));
  assert.deepEqual([got.roster, got.problem], [v3, "unchecked"]);
  const byStranger = await signRoster({ ...base, version: 3, members: [owner, { account: "bob", root: bob.id, role: "owner", name: "bob" }] }, bob);
  assert.deepEqual(await checkRoster(v2, byStranger, pin, certs, serves([v1, v2, byStranger])), { roster: v2, problem: "unchecked", changes: [] });
});

test("versions in between are fetched and checked; one left out or unreachable stops there", async () => {
  const v4 = await signRoster({ ...base, version: 4, members: [owner] }, alice);
  const got = await checkRoster(v1, v4, pin, certs, serves([v1, v2, v3, v4]));
  assert.deepEqual([got.roster, got.problem], [v4, null]);
  // Control leaves v3 out: v2 is as far as it checks.
  const gap = await checkRoster(v1, v4, pin, certs, serves([v1, v2, v4]));
  assert.deepEqual([gap.roster, gap.problem], [v2, "unchecked"]);
  assert.deepEqual(await checkRoster(v1, v4, pin, certs, none), { roster: v1, problem: "unchecked", changes: [] });
});
