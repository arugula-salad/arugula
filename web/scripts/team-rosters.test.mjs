// A team's roster as the owner's page takes it (web/src/shell/team-rosters.ts):
// control's latest must follow the last version this browser checked, as a
// team machine's must (`checkpoint::advance` in crates/e2e; e2e-interop.ts
// checks the two walk alike), and on first sight the whole signed history
// is checked from version 1. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { certBody, revocationBody } from "../src/shell/e2e/cert.ts";
import { generateKeys, signText } from "../src/shell/e2e/keys.ts";
import { ownersAsHeld, signLock, signRoster } from "../src/shell/e2e/team.ts";
import { checkRoster, RosterStore, takeLock, TrustStore } from "../src/shell/team-rosters.ts";

const cert = async (k, account, by, name = account, created = 1) => {
  const c = { v: 1, account, device: k.id, kind: "browser", name, noise: k.noisePub, sign: k.signPub, created, approver: by.id, sig: "" };
  c.sig = await signText(by, certBody(c));
  return c;
};

const alice = await generateKeys();
const alice2 = await generateKeys();
const bob = await generateKeys();
const bob2 = await generateKeys();
const certs = { alice: [[await cert(alice, "alice", alice), await cert(alice2, "alice", alice, "laptop", 2)], []], bob: [[await cert(bob, "bob", bob)], []] };
const pin = { team: "0123456789abcdef", founder: "alice", founder_root: alice.id };
const owner = { account: "alice", root: alice.id, role: "owner", name: "alice" };
const base = { v: 1, team: pin.team, name: "Acme", at: 1 };
const v1 = await signRoster({ ...base, version: 1, members: [owner] }, alice);
const v2 = await signRoster({ ...base, version: 2, members: [owner, { account: "bob", root: bob.id, role: "viewer", name: "bob" }] }, alice);
const v3 = await signRoster({ ...base, version: 3, members: [owner, { account: "bob", root: bob2.id, role: "viewer", name: "bob" }] }, alice);
const none = async () => {
  throw new Error("not asked");
};
const serves = (rosters, more = {}) => async (since) => ({ rosters: rosters.filter((r) => r.version > since), certs: more });
const ok = (roster, extra = {}) => ({ roster, problem: null, changes: [], late: [], keep: roster, ...extra });

test("a first look checks the whole signed history from the founder", async () => {
  assert.deepEqual(await checkRoster(null, v2, pin, certs, serves([v1, v2])), ok(v2));
  // A head no owner signed, or history that can't be fetched: not kept.
  const byBob = await signRoster({ ...base, version: 3, members: [owner, { account: "bob", root: bob.id, role: "owner", name: "bob" }] }, bob);
  const got = await checkRoster(null, byBob, pin, certs, serves([v1, v2, byBob]));
  assert.deepEqual([got.problem, got.keep], ["unchecked", v2]);
  const lost = await checkRoster(null, v2, pin, certs, none);
  assert.deepEqual([got.problem, lost.problem, lost.keep], ["unchecked", "unchecked", null]);
  // A history that doesn't start at the founder's version 1.
  assert.equal((await checkRoster(null, v2, pin, certs, serves([v2]))).keep, null);
});

test("a version signed by a device removed since is taken if it counted then, and said", async () => {
  // alice2 signs v2 at 5, and is removed at 10; alice signs v3.
  const w2 = await signRoster({ ...base, version: 2, at: 5, members: v2.members }, alice2);
  const w3 = await signRoster({ ...base, version: 3, at: 20, members: v3.members }, alice);
  const r = { v: 1, account: "alice", device: alice2.id, at: 10, by: alice.id, sig: "" };
  r.sig = await signText(alice, revocationBody(r));
  const later = { ...certs, alice: [certs.alice[0], [r]] };
  const yes = { lateOk: () => true };
  const got = await checkRoster(null, w3, pin, later, serves([v1, w2, w3]), yes);
  assert.deepEqual([got.roster, got.problem, got.late], [w3, null, [2]]);
  // Not when the page had seen that removal before this look.
  const seenBefore = { lateOk: (a, root, d) => !(a === "alice" && d === alice2.id) };
  assert.equal((await checkRoster(null, w3, pin, later, serves([v1, w2, w3]), seenBefore)).problem, "unchecked");
  // Not when it's the head, nor when it's dated after the removal.
  assert.equal((await checkRoster(null, w2, pin, later, serves([v1, w2]), yes)).problem, "unchecked");
  const after = await signRoster({ ...base, version: 2, at: 15, members: v2.members }, alice2);
  const w3b = await signRoster({ ...base, version: 3, at: 20, members: v3.members }, alice);
  assert.equal((await checkRoster(null, w3b, pin, later, serves([v1, after, w3b]), yes)).problem, "unchecked");
});

test("the same version again is taken; another at that version, or an older one, isn't", async () => {
  assert.equal((await checkRoster(v2, v2, pin, certs, none)).problem, null);
  const other2 = await signRoster({ ...base, version: 2, members: [owner, { account: "bob", root: bob.id, role: "owner", name: "bob" }] }, alice);
  assert.deepEqual(await checkRoster(v2, other2, pin, certs, none), { roster: v2, problem: "another", changes: [], late: [], keep: v2 });
  assert.deepEqual(await checkRoster(v2, v1, pin, certs, none), { roster: v2, problem: "older", changes: [], late: [], keep: v2 });
});

test("the next version is taken when it follows, and a changed root is said", async () => {
  const got = await checkRoster(v2, v3, pin, certs, none);
  assert.deepEqual(got, ok(v3, { changes: [{ account: "bob", name: "bob", from: bob.id, to: bob2.id, version: 3 }] }));
});

test("a roster no owner signed isn't built on: control's own member added to a real one", async () => {
  const bent = { ...v2, version: 3, members: [...v2.members, { account: "mallory", root: bob2.id, role: "owner", name: "mallory" }] };
  const got = await checkRoster(v2, bent, pin, certs, serves([v1, v2, v3]));
  assert.deepEqual([got.roster, got.problem], [v3, "unchecked"]);
  const byStranger = await signRoster({ ...base, version: 3, members: [owner, { account: "bob", root: bob.id, role: "owner", name: "bob" }] }, bob);
  assert.deepEqual(await checkRoster(v2, byStranger, pin, certs, serves([v1, v2, byStranger])), { roster: v2, problem: "unchecked", changes: [], late: [], keep: v2 });
});

test("versions in between are fetched and checked; one left out or unreachable stops there", async () => {
  const v4 = await signRoster({ ...base, version: 4, members: [owner] }, alice);
  const got = await checkRoster(v1, v4, pin, certs, serves([v1, v2, v3, v4]));
  assert.deepEqual([got.roster, got.problem], [v4, null]);
  const gap = await checkRoster(v1, v4, pin, certs, serves([v1, v2, v4]));
  assert.deepEqual([gap.roster, gap.problem], [v2, "unchecked"]);
  assert.deepEqual((await checkRoster(v1, v4, pin, certs, none)).problem, "unchecked");
});

test("the checked rosters are read once and kept in memory; storage that fails stops changes", () => {
  const box = { v: JSON.stringify({ t1: v2 }) };
  const storage = { getItem: () => box.v, setItem: (_, v) => (box.v = v) };
  const s = new RosterStore(() => storage, "k");
  assert.equal(s.held().t1.version, 2);
  box.v = null; // cleared under the page: it still has v2
  assert.equal(s.held().t1.version, 2);
  s.save();
  assert.equal(JSON.parse(box.v).t1.version, 2, "written back");
  assert.equal(s.kept, true);
  const unreadable = new RosterStore(() => ({ getItem: () => "[not json", setItem: () => {} }), "k");
  assert.deepEqual(unreadable.held(), {});
  assert.equal(unreadable.kept, false);
  const full = new RosterStore(() => ({ getItem: () => null, setItem: () => { throw new Error("quota"); } }), "k");
  full.held();
  full.save();
  assert.equal(full.kept, false);
  const off = new RosterStore(() => { throw new Error("no storage"); }, "k");
  assert.deepEqual([off.held(), off.kept], [{}, false]);
});

// One browser-wide lock, shared by the "tabs" below (the Web Locks API's
// part): each look runs alone.
const mutex = () => {
  let tail = Promise.resolve();
  return (_name, fn) => {
    const run = tail.then(fn);
    tail = run.catch(() => {});
    return run;
  };
};
const memory = () => {
  const box = { v: null };
  return { box, storage: { getItem: () => box.v, setItem: (_, v) => (box.v = v) } };
};
const revoke = async (account, device, by, at) => {
  const r = { v: 1, account, device, at, by: by.id, sig: "" };
  r.sig = await signText(by, revocationBody(r));
  return r;
};
const { evaluate } = await import("../src/shell/e2e/cert.ts");
// An account: root approves a laptop and a phone; a stranger's
// certificate the laptop signs dated early.
const acct = await (async () => {
  const root = await generateKeys();
  const laptop = await generateKeys();
  const phone = await generateKeys();
  const stranger = await generateKeys();
  return {
    trust: { account: "acct", root: root.id },
    root,
    laptop,
    rc: await cert(root, "acct", root),
    lc: await cert(laptop, "acct", root, "laptop", 2),
    pc: await cert(phone, "acct", root, "phone", 3),
    late: await cert(stranger, "acct", laptop, "stranger", 50),
  };
})();
const trustedIn = async (got, id) => (await evaluate(acct.trust, got.certs, got.revocations)).has(id);

test("an account's removals add up across loads, and a removed device's late certificate doesn't count", async () => {
  const { storage } = memory();
  const locks = mutex();
  const { trust, rc, lc, pc, late } = acct;
  const r = await revoke("acct", lc.device, acct.root, 100);
  // Load 1: the laptop's removal.
  const one = new TrustStore(() => storage, "k", () => false, locks);
  assert.equal((await one.look((l) => l.take(trust, { certs: [rc, lc, pc], revocations: [r] }))).kept, true);
  // Load 2 (a new page): control leaves the removal out, and sends a
  // certificate the removed laptop signed, dated before its removal.
  const two = new TrustStore(() => storage, "k", () => false, locks);
  const { value: got, kept } = await two.look((l) => l.take(trust, { certs: [rc, lc, pc, late], revocations: [] }));
  assert.equal(kept, true);
  assert.equal(await trustedIn(got, lc.device), false, "a removal control left out counts no more");
  assert.equal(await trustedIn(got, late.device), false, "a removed device's late certificate counts");
  assert.equal(await trustedIn(got, pc.device), true);
});

test("two tabs looking at once: one tab's removal and its round hold against the other's late certificate", async () => {
  const { box, storage } = memory();
  const locks = mutex();
  const { trust, rc, lc, pc, late } = acct;
  const a = new TrustStore(() => storage, "k", () => false, locks);
  const b = new TrustStore(() => storage, "k", () => false, locks);
  await b.look((l) => l.take(trust, { certs: [rc, lc, pc], revocations: [] }));
  const gone = await revoke("acct", lc.device, acct.root, 100);
  // A takes the laptop's removal and waits mid-look; B starts meanwhile,
  // with control leaving the removal out and adding the late certificate.
  let release;
  const held = new Promise((r) => (release = r));
  const lookA = a.look(async (l) => {
    const got = await l.take(trust, { certs: [rc, lc, pc], revocations: [gone] });
    await held;
    return got;
  });
  const lookB = b.look((l) => l.take(trust, { certs: [rc, lc, pc, late], revocations: [] }));
  await new Promise((r) => setTimeout(r, 10));
  release();
  await lookA;
  const { value: got } = await lookB;
  assert.equal(await trustedIn(got, late.device), false, "the late certificate counts in the other tab");
  assert.equal(await trustedIn(got, lc.device), false, "the removal was lost");
  const stored = JSON.parse(box.v);
  assert.equal(await trustedIn(stored.accounts[`acct.${acct.trust.root}`], lc.device), false, "the stored view lost the removal");
  // No two looks shared a round.
  assert.notEqual(stored.seen.at[gone.sig], stored.seen.at[late.sig]);
});

test("two tabs: a removal stays with the certificate it rests on", async () => {
  const { box, storage } = memory();
  const locks = mutex();
  const { trust, rc, lc, pc, laptop } = acct;
  const a = new TrustStore(() => storage, "k", () => false, locks);
  const b = new TrustStore(() => storage, "k", () => false, locks);
  await b.look((l) => l.take(trust, { certs: [rc, lc, pc], revocations: [] }));
  // A: the laptop removes the phone. B, after: control leaves out the
  // laptop's certificate and the removal.
  // B's look starts while A's is under way.
  const phoneGone = await revoke("acct", pc.device, laptop, 10);
  let release;
  const held = new Promise((r) => (release = r));
  const lookA = a.look(async (l) => {
    await l.take(trust, { certs: [rc, lc, pc], revocations: [phoneGone] });
    await held;
  });
  const lookB = b.look((l) => l.take(trust, { certs: [rc, pc], revocations: [] }));
  await new Promise((r) => setTimeout(r, 10));
  release();
  await lookA;
  const { value: got } = await lookB;
  assert.equal(await trustedIn(got, pc.device), false, "the removed phone is back");
  const stored = JSON.parse(box.v).accounts[`acct.${acct.trust.root}`];
  assert.equal(await trustedIn(stored, pc.device), false, "the stored view has the removed phone back");
});

test("a look that can't be kept changes nothing held; the page goes on with what it held", async () => {
  const { trust, rc, lc, pc } = acct;
  const r = await revoke("acct", lc.device, acct.root, 100);
  const box = { v: null, full: false };
  const storage = {
    getItem: () => box.v,
    setItem: (_, v) => {
      if (box.full) throw new Error("quota");
      box.v = v;
    },
  };
  const s = new TrustStore(() => storage, "k", () => false, mutex());
  await s.look((l) => l.take(trust, { certs: [rc, lc, pc], revocations: [r] }));
  box.full = true;
  const { kept } = await s.look((l) => l.take(trust, { certs: [rc, lc, pc], revocations: [] }));
  assert.equal(kept, false);
  assert.equal(s.kept, false);
  assert.equal(await trustedIn(s.view(trust), lc.device), false, "what it held before");
  // Nothing to lock with: not kept either.
  const nolock = new TrustStore(() => memory().storage, "k", () => false, null);
  assert.equal((await nolock.look((l) => l.take(trust, { certs: [rc], revocations: [] }))).kept, false);
  // Unreadable: not kept.
  const odd = new TrustStore(() => ({ getItem: () => "{", setItem: () => {} }), "k", () => false, mutex());
  assert.equal((await odd.look((l) => l.take(trust, { certs: [rc], revocations: [] }))).kept, false);
});

test("history's certificates go through what the page holds: a removal it saw stops a version the removed device signed", async () => {
  const { storage } = memory();
  const s = new TrustStore(() => storage, "k", () => false, mutex());
  // Look 1: alice's laptop (alice2) is removed.
  const gone = await revoke("alice", alice2.id, alice, 10);
  await s.look((l) => l.take({ account: "alice", root: alice.id }, { certs: certs.alice[0], revocations: [gone] }));
  // Look 2: control offers v2 signed by the removed laptop, and leaves the
  // removal out of history's certificates.
  const byLaptop = await signRoster({ ...base, version: 2, at: 20, members: v2.members }, alice2);
  const history = async (since) => ({ rosters: [v1, byLaptop].filter((r) => r.version > since), certs: { alice: [certs.alice[0], []] } });
  const { value: got } = await s.look(async (l) => {
    const take = async (account, root, [c, r]) => {
      const out = await l.take({ account, root }, { certs: c, revocations: r });
      return [out.certs, out.revocations];
    };
    return checkRoster(v1, byLaptop, pin, { bob: certs.bob }, history, { vet: (prev, c) => ownersAsHeld(prev, pin, c, take) });
  });
  assert.deepEqual([got.roster, got.problem], [v1, "unchecked"]);
  // Taken as control sends them (no vet), it would stand.
  assert.equal((await checkRoster(v1, byLaptop, pin, { bob: certs.bob }, history)).problem, null);
});

test("stored stamps are checked strictly, and storage found empty when it was kept is a failure", async () => {
  const look = (s) => s.look(async () => {});
  const bad = (v) => new TrustStore(() => ({ getItem: () => JSON.stringify(v), setItem: () => {} }), "k", () => false, mutex());
  for (const v of [
    { accounts: {}, seen: { round: 1, at: { x: "1" } } },
    { accounts: {}, seen: { round: 1, at: { x: -1 } } },
    { accounts: {}, seen: { round: "1", at: {} } },
    { accounts: { a: { certs: [] } }, seen: { round: 0, at: {} } },
    { accounts: {}, seen: { round: 0, at: {} }, locks: { t: { at: "x" } } },
  ]) {
    assert.equal((await look(bad(v))).kept, false, JSON.stringify(v));
  }
  assert.equal((await look(bad({ accounts: {}, seen: { round: 2, at: { x: 1 } } }))).kept, true);
  // Nothing stored: fresh, unless this browser kept one before.
  const fresh = new TrustStore(() => ({ getItem: () => null, setItem: () => {} }), "k", () => false, mutex());
  assert.equal((await look(fresh)).kept, true);
  const cleared = new TrustStore(() => ({ getItem: () => null, setItem: () => {} }), "k", () => true, mutex());
  assert.equal((await look(cleared)).kept, false);
  const roster = new RosterStore(() => ({ getItem: () => null, setItem: () => {} }), "k", () => true);
  roster.held();
  assert.equal(roster.kept, false);
  const odd = new RosterStore(() => ({ getItem: () => JSON.stringify({ t: { version: "2" } }), setItem: () => {} }), "k");
  odd.held();
  assert.equal(odd.kept, false);
});

test("two tabs' rosters: the newer version of a team stays", () => {
  const r = memory();
  const ra = new RosterStore(() => r.storage, "r");
  const rb = new RosterStore(() => r.storage, "r");
  ra.held().t1 = v3;
  rb.held().t1 = v2;
  ra.save();
  rb.save();
  assert.equal(JSON.parse(r.box.v).t1.version, 3);
});

test("a team's lock is taken from an owner in the roster checked, newer than the one held, and not dated far ahead", async () => {
  const { storage } = memory();
  const s = new TrustStore(() => storage, "k", () => false, mutex());
  const lock = (locked, at, by, team = pin.team, version = 2) => signLock({ team, locked, version, at }, by);
  const now = 1_000_000;
  const l1 = await lock(true, 900_000, alice);
  const l2 = await lock(false, 950_000, alice);
  const refused = [
    await lock(false, 950_000, bob),
    { ...(await lock(false, 950_000, alice)), at: 950_001 },
    await lock(false, 950_000, alice, "ffffffffffffffff"),
    await lock(false, 950_000, alice, pin.team, 3),
    await lock(false, 800_000, alice),
    await lock(false, now + 86_400_000, alice),
  ];
  await s.look(async (l) => {
    assert.deepEqual(await takeLock(l, pin.team, l1, v2, certs, now), l1);
    // Signed by a member who isn't an owner, bent, for another team,
    // against a newer version, older than the one held, dated far ahead.
    for (const x of refused) assert.deepEqual(await takeLock(l, pin.team, x, v2, certs, now), l1, JSON.stringify(x));
    assert.deepEqual(await takeLock(l, pin.team, l2, v2, certs, now), l2);
  });
  assert.deepEqual(s.lock(pin.team), l2, "kept");
});
