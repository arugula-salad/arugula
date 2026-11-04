// Hosted machines (M178): the page's copy of `arugula_e2e::hosted` checks
// the same bytes as Rust's (hosted.fixture.json, which
// `the_pages_fixture_is_these_bytes` in crates/e2e/src/hosted.rs pins), and
// accepts a hosted machine for its project only. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { acceptHosted, acceptHostedHolding, closedBody, delegationId } from "../src/shell/e2e/hosted.ts";
import { ClosureStore, HELD_CLOSURES } from "../src/shell/team-rosters.ts";
import { directUrls } from "../src/shared/hosted.ts";

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

test("control's word that a delegation ended counts as Rust's does", async () => {
  const yes = { ok: true, project: "prj_1", workspace: "hw_1", role: "owner" };
  assert.deepEqual(await acceptHosted("acct1", proof({ closed: [f.closed] }), mine), yes, "after the box was made: it stays");
  assert.deepEqual(await acceptHosted("acct1", proof({ closed: [f.closedEarly] }), mine), { ok: false, why: "revoked" });
  const moved = { ...f.closedEarly, at: 2400 };
  assert.deepEqual(await acceptHosted("acct1", proof({ closed: [moved] }), mine), yes, "its signature doesn't cover its time");
  assert.deepEqual(await acceptHosted("acct1", proof({ closed: [{ ...f.closedEarly, sig: f.delegation.sig }] }), mine), yes, "an owner's signature isn't control's word");
  assert.deepEqual(await acceptHosted("acct1", proof({ closed: undefined }), mine), yes, "a control from before sends none");
});

const memory = () => {
  const m = new Map();
  return () => ({ getItem: (k) => m.get(k) ?? null, setItem: (k, v) => m.set(k, v) });
};
// Made at 3000: `closedEarly` (2500) is before it.
const refused = { ok: false, why: "revoked" };
const accepted = { ok: true, project: "prj_1", workspace: "hw_1", role: "owner" };

test("this browser keeps control's word that a delegation ended", async () => {
  const storage = memory();
  const held = new ClosureStore(storage, "k");
  assert.deepEqual(await acceptHostedHolding("acct1", proof(), mine, held), accepted, "it's heard nothing yet");
  assert.deepEqual(await acceptHostedHolding("acct1", proof({ closed: [f.closedEarly, {}] }), mine, held), refused);
  assert.deepEqual(await acceptHostedHolding("acct1", proof(), mine, held), refused, "control left it out");
  // A new page reads it back.
  assert.deepEqual(await acceptHostedHolding("acct1", proof(), mine, new ClosureStore(storage, "k")), refused);
  // The earliest stays.
  await acceptHostedHolding("acct1", proof({ closed: [f.closed] }), mine, held);
  assert.equal(held.get(f.id).at, 2500);
});

test("what one tab heard counts in another", async () => {
  const storage = memory();
  const [a, b] = [new ClosureStore(storage, "k"), new ClosureStore(storage, "k")];
  assert.deepEqual(await acceptHostedHolding("acct1", proof(), mine, b), accepted, "b has looked already");
  assert.deepEqual(await acceptHostedHolding("acct1", proof({ closed: [f.closedEarly] }), mine, a), refused);
  assert.deepEqual(await acceptHostedHolding("acct1", proof(), mine, b), refused, "b hears it from a");
  // Storage cleared under a page: its memory stays. An entry that doesn't read
  // takes nothing else with it.
  storage().setItem("k", "not json");
  assert.deepEqual(await acceptHostedHolding("acct1", proof(), mine, a), refused);
  const other = memory();
  other().setItem("k", JSON.stringify({ [f.id]: f.closedEarly, x: {}, y: { ...f.closed, at: 2.5 } }));
  assert.equal(new ClosureStore(other, "k").get(f.id).at, 2500);
});

test("a time is its value, however it's written", async () => {
  const text = (o) => JSON.parse(JSON.stringify(o).replace(`"at":2500`, `"at":2.5e3`));
  assert.equal(JSON.stringify(f.closedEarly).includes(`"at":2500`), true);
  assert.deepEqual(await acceptHosted("acct1", proof({ closed: [text(f.closedEarly)] }), mine), refused);
  assert.deepEqual(await acceptHosted("acct1", proof({ closed: [{ ...f.closedEarly, at: 2 ** 53 }] }), mine), accepted);
});

test("nothing that doesn't check out is kept", async () => {
  const held = new ClosureStore(memory(), "k");
  await acceptHostedHolding("acct1", proof({ closed: [{ ...f.closedEarly, at: 2400 }, {}, null] }), mine, held);
  assert.equal(held.get(f.id), undefined);
  // Nor of a delegation no owner of these signed.
  const other = { account: { account: "acct1", root: "0000000000000000" }, certs: [f.root], revocations: [] };
  await acceptHostedHolding("acct1", proof({ closed: [f.closedEarly] }), other, held);
  assert.equal(held.get(f.id), undefined);
});

test("storage that fails leaves memory, and what's kept is bounded", async () => {
  const broken = () => ({ getItem: () => { throw new Error("no"); }, setItem: () => { throw new Error("no"); } });
  const held = new ClosureStore(broken, "k");
  assert.deepEqual(await acceptHostedHolding("acct1", proof({ closed: [f.closedEarly] }), mine, held), refused);
  assert.deepEqual(await acceptHostedHolding("acct1", proof(), mine, held), refused);
  const many = new ClosureStore(memory(), "k");
  const one = (n) => ({ project: "p", delegation: `d${n}`, at: 100 + n, sig: "s" });
  for (let n = 0; n < HELD_CLOSURES + 3; n++) many.take(one(n));
  assert.equal(many.get("d0"), undefined, "the earliest-ended goes first");
  assert.ok(many.get(`d${HELD_CLOSURES + 2}`));
  assert.ok(many.get("d3"));
});

// Rust's example file (contract/examples/logic/hosted/accept.json, written by
// crates/e2e/tests/logic/hosted.rs): every case, from its JSON alone, gets
// the same answer here. Rust's checks and this copy can't drift apart.
const examples = JSON.parse(readFileSync(new URL("../../contract/examples/logic/hosted/accept.json", import.meta.url), "utf8"));
const named = (labels = []) => labels.map((l) => examples.objects[l]);
const ownersOf = (o) =>
  "team" in o
    ? {
        team: examples.objects[o.team.roster],
        certs: Object.fromEntries(Object.entries(o.team.certs).map(([a, c]) => [a, [named(c.certs), named(c.revocations)]])),
      }
    : { account: { account: o.account.account, root: o.account.root }, certs: named(o.account.certs), revocations: named(o.account.revocations) };

for (const c of examples.cases) {
  test(`accept.json: ${c.about}`, async () => {
    const i = c.input;
    const got = await acceptHosted(
      i.me,
      { cert: examples.objects[i.cert], delegation: examples.objects[i.delegation], revocations: named(i.revokes), closed: named(i.closed) },
      ownersOf(i.owners),
    );
    if (c.output.refused) assert.deepEqual(got, { ok: false, why: c.output.refused });
    else {
      const { project, workspace, role } = c.output.accepted;
      assert.deepEqual(got, { ok: true, project, workspace, role });
    }
  });
}

test("the bytes of control's word are Rust's", async () => {
  const b = JSON.parse(readFileSync(new URL("../../contract/examples/logic/hosted/bodies.json", import.meta.url), "utf8"));
  const c = b.cases.find((x) => x.input.of === "closed");
  assert.equal(closedBody(b.objects[c.input.closed]), c.output.body);
});

test("a hosted machine has no direct URLs to try; a joined one keeps its own", () => {
  const hosted = { urls: ["http://10.0.0.5:7681"], hosted: { project: "prj_1", workspace: "hw_1", role: "owner" } };
  assert.deepEqual(directUrls(hosted), []);
  assert.deepEqual(directUrls({ urls: ["http://geek:7681"] }), ["http://geek:7681"]);
  assert.deepEqual(directUrls({ urls: ["http://geek:7681"], hosted: undefined }), ["http://geek:7681"]);
});
