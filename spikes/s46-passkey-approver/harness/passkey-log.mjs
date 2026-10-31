// S46 (#382): the passkey log DK3 (#384) builds, as a working sketch.
// Runs in a browser and in Node 20+ (WebCrypto only, no dependencies), so
// the harness checks real providers' keys with the same code the README
// proposes. Not product code: DK3 writes the Rust and TS for real.
//
// An account's passkey log is append-only and hash-chained. Each event's
// body is text, like the deploy bodies (`arugula deploy op v1`), and its
// hash is SHA-256 over the body and its proof, so the next event's `prev`
// covers both.
//
//   arugula passkey log v1
//   account acc_123
//   seq 2
//   prev <hex hash of event seq-1, or 64 zeros for seq 0>
//   event added | removed
//   credential <base64url credential id>
//   rp control.arugula.io
//   alg -7
//   key sec1 <base64url>     (P-256 uncompressed; or `raw` for Ed25519,
//                             `spki` for RSA: what passkey.rs keeps today)
//   aaguid <hex, 32 chars; zeros when the authenticator won't say>
//   at <ms since the epoch>
//   by passkey <credential id> | github | reset | backfill
//
// `by passkey X` events carry a proof: X's assertion over SHA-256(body),
// with user verification. `github` (a first passkey, #1386), `reset`
// (aru-119, after its wait) and `backfill` (rows from before the log) carry
// none: the operator vouches for them, and the log says so.

const enc = new TextEncoder();
const subtle = globalThis.crypto.subtle;

export const ZERO = "0".repeat(64);

export function b64u(bytes) {
  const b = bytes instanceof ArrayBuffer ? new Uint8Array(bytes) : bytes;
  let s = "";
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function unb64u(s) {
  const t = atob(s.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((s.length + 3) % 4));
  return Uint8Array.from(t, (c) => c.charCodeAt(0));
}

export const hex = (b) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");

export async function sha256(data) {
  return new Uint8Array(await subtle.digest("SHA-256", typeof data === "string" ? enc.encode(data) : data));
}

/** An event's body, from its fields. */
export function body(e) {
  return (
    "arugula passkey log v1\n" +
    `account ${e.account}\nseq ${e.seq}\nprev ${e.prev}\nevent ${e.event}\ncredential ${e.credential}\n` +
    `rp ${e.rp}\nalg ${e.alg}\nkey ${e.keyFormat} ${e.key}\naaguid ${e.aaguid}\nat ${e.at}\nby ${e.by}\n`
  );
}

/** The proof's lines (none for an operator-vouched event). */
function proofLines(p) {
  if (!p) return "";
  return `proof ${p.clientDataJSON} ${p.authenticatorData} ${p.signature}\n`;
}

/** An event's hash: SHA-256 of its body and proof, hex. */
export async function eventHash(e) {
  return hex(await sha256(body(e) + proofLines(e.proof)));
}

/** Append an event to `log` (filling seq and prev). */
export async function append(log, e) {
  const last = log[log.length - 1];
  const full = { ...e, seq: log.length, prev: last ? await eventHash(last) : ZERO };
  log.push(full);
  return full;
}

// ---------------------------------------------------------------- keys

/** A COSE key (decoded CBOR map) as passkey.rs keeps it: alg, format, bytes. */
export async function fromCose(m) {
  const g = (k) => m.get(k);
  const kty = g(1), alg = g(3);
  if (kty === 2 && alg === -7 && g(-1) === 1) {
    const x = g(-2), y = g(-3);
    const sec1 = new Uint8Array(65);
    sec1[0] = 4;
    sec1.set(x, 1);
    sec1.set(y, 33);
    return { alg, keyFormat: "sec1", key: b64u(sec1) };
  }
  if (kty === 1 && alg === -8 && g(-1) === 6) return { alg, keyFormat: "raw", key: b64u(g(-2)) };
  if (kty === 3 && alg === -257) {
    const k = await subtle.importKey(
      "jwk",
      { kty: "RSA", n: b64u(g(-1)), e: b64u(g(-2)), alg: "RS256", ext: true },
      { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
      true,
      ["verify"],
    );
    return { alg, keyFormat: "spki", key: b64u(await subtle.exportKey("spki", k)) };
  }
  throw new Error(`unsupported COSE key: kty ${kty} alg ${alg}`);
}

async function importKey(alg, keyFormat, key) {
  const bytes = unb64u(key);
  if (alg === -7 && keyFormat === "sec1")
    return subtle.importKey("raw", bytes, { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]);
  if (alg === -8 && keyFormat === "raw") return subtle.importKey("raw", bytes, { name: "Ed25519" }, false, ["verify"]);
  if (alg === -257 && keyFormat === "spki")
    return subtle.importKey("spki", bytes, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["verify"]);
  throw new Error(`unsupported key: alg ${alg} ${keyFormat}`);
}

/** DER ECDSA signature to WebCrypto's r‖s. */
function derToRaw(der) {
  let i = 2;
  if (der[1] & 0x80) i += der[1] & 0x7f;
  const int = () => {
    if (der[i++] !== 2) throw new Error("bad DER signature");
    const len = der[i++];
    let v = der.slice(i, i + len);
    i += len;
    while (v.length > 32 && v[0] === 0) v = v.slice(1);
    const out = new Uint8Array(32);
    out.set(v, 32 - v.length);
    return out;
  };
  const r = int(), s = int();
  const raw = new Uint8Array(64);
  raw.set(r);
  raw.set(s, 32);
  return raw;
}

/** Whether `sig` is the key's over authData ‖ SHA-256(clientDataJSON). */
export async function signatureOk(k, authData, clientData, sig) {
  const msg = new Uint8Array(authData.length + 32);
  msg.set(authData);
  msg.set(await sha256(clientData), authData.length);
  const key = await importKey(k.alg, k.keyFormat, k.key);
  if (k.alg === -7) return subtle.verify({ name: "ECDSA", hash: "SHA-256" }, key, derToRaw(sig), msg);
  if (k.alg === -8) return subtle.verify({ name: "Ed25519" }, key, sig, msg);
  return subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, key, sig, msg);
}

// ---------------------------------------------------------------- verify

export const FLAGS = { UP: 0x01, UV: 0x04, BE: 0x08, BS: 0x10, AT: 0x40, ED: 0x80 };

export function flagsOf(byte) {
  return Object.fromEntries(Object.entries(FLAGS).map(([k, v]) => [k, (byte & v) !== 0]));
}

/**
 * Check an assertion against an account's log, with nothing else trusted:
 * - the chain holds (hashes, seq, account) and every `by passkey` event's
 *   proof is that passkey's, live at the time, over the event's body;
 * - the credential was in the log at `at` (added at or before, not removed
 *   at or before);
 * - clientData is a `webauthn.get` over exactly `challenge`, from an origin
 *   `origins[rp]` allows, not inside another site's frame;
 * - authData is for the key's RP, with UP and UV;
 * - the signature checks out.
 * `Ok` is `{ ok: true, key, flags }`, else `{ ok: false, why }`.
 */
export async function verify({ log, account, assertion, challenge, at, origins }) {
  // `live`: the keys as of each event, for its proof. `then`: as of `at`.
  // The whole chain is checked either way.
  const live = new Map();
  let then = null;
  for (let i = 0; i < log.length; i++) {
    const e = log[i];
    if (e.account !== account) return no(`event ${i} is another account's`);
    if (e.seq !== i) return no(`event ${i} has seq ${e.seq}`);
    if (e.prev !== (i ? await eventHash(log[i - 1]) : ZERO)) return no(`event ${i} doesn't follow the one before`);
    if (e.by.startsWith("passkey ")) {
      const signer = live.get(e.by.slice(8));
      if (!signer || !e.proof) return no(`event ${i} is signed by a passkey that wasn't live`);
      const r = await check(signer, e.proof, b64u(await sha256(body(e))), origins);
      if (!r.ok) return no(`event ${i}'s proof: ${r.why}`);
    } else if (!["github", "reset", "backfill"].includes(e.by)) {
      return no(`event ${i}: by ${e.by}?`);
    }
    if (i && e.at < log[i - 1].at) return no(`event ${i} is older than the one before`);
    if (e.at > at && !then) then = new Map(live);
    if (e.event === "added") live.set(e.credential, e);
    else if (e.event === "removed") live.delete(e.credential);
    else return no(`event ${i}: ${e.event}?`);
  }
  const k = (then || live).get(assertion.id);
  if (!k) return no("that passkey wasn't in the account's log then");
  return check(k, assertion, challenge, origins);
}

async function check(k, a, challenge, origins) {
  const cdRaw = unb64u(a.clientDataJSON);
  let cd;
  try {
    cd = JSON.parse(new TextDecoder().decode(cdRaw));
  } catch {
    return no("bad clientDataJSON");
  }
  if (cd.type !== "webauthn.get") return no("wrong ceremony");
  if (cd.challenge !== challenge) return no("signed something else");
  if (!(origins[k.rp] || []).includes(cd.origin)) return no(`origin ${cd.origin} isn't one of ${k.rp}'s`);
  if (cd.crossOrigin === true || (cd.topOrigin && cd.topOrigin !== cd.origin)) return no("from inside another site's frame");
  const ad = unb64u(a.authenticatorData);
  if (ad.length < 37) return no("short authenticator data");
  if (hex(ad.slice(0, 32)) !== hex(await sha256(k.rp))) return no(`rpIdHash isn't ${k.rp}'s`);
  const flags = flagsOf(ad[32]);
  if (!flags.UP || !flags.UV) return no("no user verification");
  if (!(await signatureOk(k, ad, cdRaw, unb64u(a.signature)))) return no("signature doesn't check out");
  return { ok: true, key: k, flags };
}

const no = (why) => ({ ok: false, why });
