// Hosted machines (M178): the page's copy of `arugula_e2e::hosted` checks
// the same bytes as Rust's (hosted.fixture.json, which
// `the_pages_fixture_is_these_bytes` in crates/e2e/src/hosted.rs pins), and
// accepts a hosted machine for its project only. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { acceptHosted, delegationId } from "../src/shell/e2e/hosted.ts";

const f = JSON.parse(readFileSync(new URL("./hosted.fixture.json", import.meta.url), "utf8"));
const mine = { account: { account: "acct1", root: f.root.device }, certs: [f.root], revocations: [] };
const proof = (over = {}) => ({ cert: f.cert, delegation: f.delegation, revocations: [], ...over });

test("a delegation's id is Rust's", async () => {
  assert.equal(await delegationId(f.delegation), f.id);
});

test("the project's own account accepts it, and nobody else", async () => {
  assert.deepEqual(await acceptHosted("acct1", proof(), mine), { ok: true, project: "prj_1", workspace: "hw_1", role: "owner" });
  assert.deepEqual(await acceptHosted("acct2", proof(), mine), { ok: false, why: "not_in_project" });
  const theirs = { account: { account: "acct2", root: f.root.device }, certs: [], revocations: [] };
  assert.deepEqual(await acceptHosted("acct2", proof(), theirs), { ok: false, why: "wrong_project" });
});

test("a team's members accept it, and only from an owner's delegation", async () => {
  const team = (role) => ({
    team: { team: "t1", members: [{ account: "acct1", root: f.root.device, role, name: "a" }, { account: "acct3", root: "r", role: "viewer", name: "b" }] },
    certs: { acct1: [[f.root], []] },
  });
  const p = proof(f.team);
  assert.deepEqual(await acceptHosted("acct3", p, team("owner")), { ok: true, project: "prj_1", workspace: "hw_2", role: "viewer" });
  assert.deepEqual(await acceptHosted("acct4", p, team("owner")), { ok: false, why: "not_in_project" });
  // Its signer an editor now: the delegation stops counting.
  assert.deepEqual(await acceptHosted("acct3", p, team("editor")), { ok: false, why: "unsigned" });
  // Not the team's machine as the owner's own, nor the other way round.
  assert.deepEqual(await acceptHosted("acct1", p, mine), { ok: false, why: "wrong_project" });
  assert.deepEqual(await acceptHosted("acct1", proof(), team("owner")), { ok: false, why: "wrong_project" });
});

test("tampering, revoking, and control's own signature are refused", async () => {
  assert.deepEqual(await acceptHosted("acct1", proof({ cert: { ...f.cert, workspace: "hw_2" } }), mine), { ok: false, why: "bad_signature" });
  assert.deepEqual(await acceptHosted("acct1", proof({ cert: { ...f.cert, project: "prj_2" } }), mine), { ok: false, why: "form" });
  assert.deepEqual(await acceptHosted("acct1", proof({ delegation: { ...f.delegation, sig: f.cert.sig } }), mine), { ok: false, why: "unsigned" });
  // Revoked at 4000, made at 3000: it stays. Revoked before it was made: refused.
  assert.equal((await acceptHosted("acct1", proof({ revocations: [f.revoke] }), mine)).ok, true);
  const early = { ...f.revoke, at: 2500 };
  assert.deepEqual(await acceptHosted("acct1", proof({ revocations: [early] }), mine), { ok: true, project: "prj_1", workspace: "hw_1", role: "owner" }, "a revocation whose signature doesn't cover its time counts for nothing");
  // A root control made up: the delegation's signer isn't in this chain.
  const other = { account: { account: "acct1", root: "0000000000000000" }, certs: [f.root], revocations: [] };
  assert.deepEqual(await acceptHosted("acct1", proof(), other), { ok: false, why: "unsigned" });
});
