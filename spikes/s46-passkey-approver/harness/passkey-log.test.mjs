// node passkey-log.test.mjs: the log format and verify() against synthetic
// authenticators (ES256, Ed25519, RS256), including the cases DK3's "done
// when" names (a key removed before `at` fails).
import assert from "node:assert/strict";
import { append, b64u, body, eventHash, fromCose, sha256, verify } from "./passkey-log.mjs";

const subtle = crypto.subtle;
const RP = "control.arugula.io";
const ORIGINS = { [RP]: ["https://control.arugula.io", "https://app.arugula.io"] };
const ACCOUNT = "acc_test";

function rawToDer(raw) {
  const int = (b) => {
    let i = 0;
    while (i < b.length - 1 && b[i] === 0) i++;
    b = b.slice(i);
    if (b[0] & 0x80) b = Uint8Array.of(0, ...b);
    return [2, b.length, ...b];
  };
  const r = int(raw.slice(0, 32)), s = int(raw.slice(32));
  return Uint8Array.from([0x30, r.length + s.length, ...r, ...s]);
}

async function authenticator(alg) {
  if (alg === -7) {
    const k = await subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
    const jwk = await subtle.exportKey("jwk", k.publicKey);
    const cose = new Map([[1, 2], [3, -7], [-1, 1], [-2, Buffer.from(jwk.x, "base64url")], [-3, Buffer.from(jwk.y, "base64url")]]);
    return { cose, sign: async (m) => rawToDer(new Uint8Array(await subtle.sign({ name: "ECDSA", hash: "SHA-256" }, k.privateKey, m))) };
  }
  if (alg === -8) {
    const k = await subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
    const raw = new Uint8Array(await subtle.exportKey("raw", k.publicKey));
    return { cose: new Map([[1, 1], [3, -8], [-1, 6], [-2, raw]]), sign: async (m) => new Uint8Array(await subtle.sign("Ed25519", k.privateKey, m)) };
  }
  const k = await subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: Uint8Array.of(1, 0, 1), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  );
  const jwk = await subtle.exportKey("jwk", k.publicKey);
  return {
    cose: new Map([[1, 3], [3, -257], [-1, Buffer.from(jwk.n, "base64url")], [-2, Buffer.from(jwk.e, "base64url")]]),
    sign: async (m) => new Uint8Array(await subtle.sign("RSASSA-PKCS1-v1_5", k.privateKey, m)),
  };
}

let n = 0;
async function key(alg) {
  const a = await authenticator(alg);
  return { ...a, id: b64u(Uint8Array.of(++n, 7, 7, 7)), ...(await fromCose(a.cose)) };
}

async function assertion(k, challenge, { origin = "https://app.arugula.io", flags = 0x05, rp = RP, extra = {} } = {}) {
  const cd = new TextEncoder().encode(JSON.stringify({ type: "webauthn.get", challenge, origin, crossOrigin: false, ...extra }));
  const ad = new Uint8Array(37);
  ad.set(await sha256(rp));
  ad[32] = flags;
  const msg = new Uint8Array(37 + 32);
  msg.set(ad);
  msg.set(await sha256(cd), 37);
  return { id: k.id, clientDataJSON: b64u(cd), authenticatorData: b64u(ad), signature: b64u(await k.sign(msg)) };
}

const added = (k, at, by = "github") => ({
  account: ACCOUNT, event: "added", credential: k.id, rp: RP, alg: k.alg, keyFormat: k.keyFormat, key: k.key,
  aaguid: "0".repeat(32), at, by,
});

async function signedBy(log, signer, e) {
  const draft = { ...e, seq: log.length, prev: log.length ? await eventHash(log[log.length - 1]) : "0".repeat(64) };
  const proof = await assertion(signer, b64u(await sha256(body(draft))), { origin: "https://control.arugula.io" });
  return append(log, { ...e, proof });
}

const k1 = await key(-7), k2 = await key(-8), k3 = await key(-257);
const log = [];
await append(log, added(k1, 1000));
await signedBy(log, k1, added(k2, 2000, `passkey ${k1.id}`));
await signedBy(log, k2, added(k3, 2500, `passkey ${k2.id}`));
await signedBy(log, k2, { ...added(k1, 3000, `passkey ${k2.id}`), event: "removed" });

const approval = "arugula deploy approval v2\n...\n";
const ch = b64u(await sha256(approval));
const v = (a, at = 5000, l = log) => verify({ log: l, account: ACCOUNT, assertion: a, challenge: ch, at, origins: ORIGINS });

const cases = [
  ["ES256 from app.arugula.io before its removal", (await v(await assertion(k1, ch), 2900)).ok, true],
  ["ES256 after its removal", (await v(await assertion(k1, ch), 3000)).why, "that passkey wasn't in the account's log then"],
  ["Ed25519 added by k1's proof", (await v(await assertion(k2, ch))).ok, true],
  ["RS256 added by k2's proof", (await v(await assertion(k3, ch))).ok, true],
  ["from control.arugula.io", (await v(await assertion(k2, ch, { origin: "https://control.arugula.io" }))).ok, true],
  ["from a stranger", (await v(await assertion(k2, ch, { origin: "https://evil.example" }))).ok, false],
  ["UV not set", (await v(await assertion(k2, ch, { flags: 0x01 }))).why, "no user verification"],
  ["another RP's", (await v(await assertion(k2, ch, { rp: "arugula.io" }))).ok, false],
  ["in another site's frame", (await v(await assertion(k2, ch, { extra: { crossOrigin: true } }))).ok, false],
  ["over other bytes", (await v(await assertion(k2, b64u(await sha256("other"))))).why, "signed something else"],
  ["a key before it was added", (await v(await assertion(k3, ch), 2400)).ok, false],
];
// The log tampered with: a key swapped in event 0.
const swapped = structuredClone(log);
swapped[0].key = k2.key;
cases.push(["a swapped key in event 0", (await v(await assertion(k2, ch), 5000, swapped)).ok, false]);
// An event signed by a passkey that was already removed.
const late = structuredClone(log);
const k4 = await key(-7);
await signedBy(late, k1, added(k4, 4000, `passkey ${k1.id}`));
cases.push(["an add signed by a removed passkey", (await v(await assertion(k4, ch), 5000, late)).ok, false]);

let bad = 0;
for (const [name, got, want] of cases) {
  const ok = (() => { try { assert.deepEqual(got, want); return true; } catch { return false; } })();
  if (!ok) bad++;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}: ${JSON.stringify(got)}`);
}
console.log(`\nevent 1's body:\n${body(log[1])}`);
process.exit(bad ? 1 : 0);
