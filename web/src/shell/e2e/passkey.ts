// An account's passkey log (DK3, aru-126): the page's copy of
// `arugula_e2e::passkey`, byte for byte. The hub publishes each account's
// passkeys as an append-only, hash-chained log
// (`GET /api/accounts/{id}/passkeys/log`), so anyone can check a stored
// passkey assertion (a deploy's approval) against it without asking the
// hub. `contract/examples/logic/passkey/` pins both copies
// (`web/scripts/passkey.test.mjs` runs them here).
//
//   arugula passkey log v1
//   account <account id>
//   seq <0, 1, 2, ...>
//   prev <hex SHA-256 of the event before, or 64 zeros>
//   event added | removed
//   credential <base64url>
//   rp <RP ID; a backfilled event may list several, space-separated>
//   alg <-7 | -8 | -257>
//   key <sec1 | raw | spki> <base64url>
//   aaguid <32 hex>
//   at <ms>
//   by passkey | signup | github | device <id> | session | reset | deleted | backfill <ms>
//
// A `by passkey` event carries a proof line after the body
// (`proof <credential> <clientDataJSON> <authenticatorData> <signature>`):
// another live passkey's assertion over SHA-256 of the body. An event's hash
// is SHA-256 of the body and the proof line. WebCrypto only, so it runs in a
// browser and in Node.

const enc = new TextEncoder();
const subtle = globalThis.crypto.subtle;

export const ZERO = "0".repeat(64);
export const ES256 = -7;
export const EDDSA = -8;
export const RS256 = -257;
const UP = 0x01;
const UV = 0x04;

/** Which origins may use each RP ID: the verifier's own configuration. */
export type Origins = Record<string, string[]>;

export interface Assertion {
  id: string;
  clientDataJSON: string;
  authenticatorData: string;
  signature: string;
}

export interface PasskeyEvent {
  account: string;
  seq: number;
  prev: string;
  event: "added" | "removed";
  credential: string;
  rp: string;
  alg: number;
  keyFormat: string;
  key: string;
  aaguid: string;
  at: number;
  by: string;
  proof?: Assertion;
  hash: string;
}

export interface PasskeyLog {
  account: string;
  events: PasskeyEvent[];
  /** When the hub began keeping logs: removals before then are in none. */
  began?: number;
}

/** Why an assertion doesn't check out: the same codes as Rust's `Refusal`. */
export type Refused =
  | "chain"
  | "not_live"
  | "client_data"
  | "ceremony"
  | "challenge"
  | "framed"
  | "auth_data"
  | "rp"
  | "origin"
  | "user"
  | "key"
  | "signature";

export type Verdict =
  | { ok: true; key: PasskeyEvent; rp: string; flags: number }
  | { ok: false; refused: Refused; event?: number };

export function b64u(bytes: Uint8Array): string {
  let s = "";
  for (const x of bytes) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** Base64url, padded or not; null when it isn't. */
export function unb64u(s: string): Uint8Array | null {
  const t = s.replace(/=+$/, "");
  if (!/^[A-Za-z0-9_-]*$/.test(t) || t.length % 4 === 1) return null;
  try {
    const bin = atob(t.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((t.length + 3) % 4));
    return Uint8Array.from(bin, (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
}

const hex = (b: Uint8Array) => [...b].map((x) => x.toString(16).padStart(2, "0")).join("");

async function sha256(data: string | Uint8Array): Promise<Uint8Array> {
  const bytes = typeof data === "string" ? enc.encode(data) : data;
  return new Uint8Array(await subtle.digest("SHA-256", bytes as BufferSource));
}

/** An event's text, hashed and (on a `by passkey` event) signed. */
export function body(e: PasskeyEvent): string {
  return (
    "arugula passkey log v1\n" +
    `account ${e.account}\nseq ${e.seq}\nprev ${e.prev}\nevent ${e.event}\ncredential ${e.credential}\n` +
    `rp ${e.rp}\nalg ${e.alg}\nkey ${e.keyFormat} ${e.key}\naaguid ${e.aaguid}\nat ${e.at}\nby ${e.by}\n`
  );
}

export function proofLine(e: PasskeyEvent): string {
  const p = e.proof;
  return p ? `proof ${p.id} ${p.clientDataJSON} ${p.authenticatorData} ${p.signature}\n` : "";
}

/** SHA-256 of the body and the proof line, hex. */
export async function eventHash(e: PasskeyEvent): Promise<string> {
  return hex(await sha256(body(e) + proofLine(e)));
}

/** What a `by passkey` proof signs: SHA-256 of the body, base64url. */
export async function eventChallenge(e: PasskeyEvent): Promise<string> {
  return b64u(await sha256(body(e)));
}

const KEY_FORMAT: Record<number, string> = { [ES256]: "sec1", [EDDSA]: "raw", [RS256]: "spki" };

type By = { kind: "passkey" | "signup" | "github" | "session" | "reset" | "deleted" | "device" | "backfill" };

function parseBy(s: string): By | null {
  if (["passkey", "signup", "github", "session", "reset", "deleted"].includes(s)) return { kind: s as By["kind"] };
  const sp = s.indexOf(" ");
  if (sp < 0) return null;
  const [k, v] = [s.slice(0, sp), s.slice(sp + 1)];
  if (k === "device" && word(v)) return { kind: "device" };
  if (k === "backfill" && /^[1-9][0-9]*$/.test(v)) return { kind: "backfill" };
  return null;
}

/** A token that can sit on a line: printable ASCII, no space. */
const word = (s: string) => s.length > 0 && /^[\x21-\x7e]+$/.test(s);
const b64Word = (s: string) => s.length > 0 && /^[A-Za-z0-9_=-]+$/.test(s);
const hexOf = (s: string, n: number) => s.length === n && /^[0-9a-f]+$/.test(s);

function wellFormed(e: PasskeyEvent): boolean {
  return (
    word(e.account) &&
    b64Word(e.credential) &&
    e.rp.length > 0 &&
    e.rp.split(" ").every(word) &&
    KEY_FORMAT[e.alg] === e.keyFormat &&
    b64Word(e.key) &&
    hexOf(e.aaguid, 32) &&
    hexOf(e.prev, 64) &&
    (!e.proof || [e.proof.id, e.proof.clientDataJSON, e.proof.authenticatorData, e.proof.signature].every(b64Word))
  );
}

type Live = Map<string, PasskeyEvent>;

/** The passkeys live at `at`, having checked the whole chain; or where
 * it breaks. */
async function liveAt(log: PasskeyLog, at: number, origins: Origins): Promise<Live | number> {
  const live: Live = new Map();
  let then: Live | null = null;
  for (let i = 0; i < log.events.length; i++) {
    const e = log.events[i];
    const prev = i === 0 ? ZERO : log.events[i - 1].hash;
    if (e.account !== log.account || e.seq !== i || e.prev !== prev || e.hash !== (await eventHash(e))) return i;
    if (i > 0 && e.at < log.events[i - 1].at) return i;
    if (!wellFormed(e)) return i;
    const by = parseBy(e.by);
    if (!by) return i;
    if (by.kind === "passkey") {
      const signer = e.proof && live.get(e.proof.id);
      if (!signer || !e.proof) return i;
      if (!(await check(signer, e.proof, await eventChallenge(e), origins)).ok) return i;
    } else if (e.proof) {
      return i;
    }
    if (e.rp.includes(" ") && by.kind !== "backfill") return i;
    if (e.at > at && !then) then = new Map(live);
    if (e.event === "added") {
      if (live.has(e.credential)) return i;
      live.set(e.credential, e);
    } else if (e.event === "removed") {
      const k = live.get(e.credential);
      if (!k || k.alg !== e.alg || k.keyFormat !== e.keyFormat || k.key !== e.key) return i;
      live.delete(e.credential);
    } else {
      return i;
    }
  }
  return then ?? live;
}

/** Whether the whole log holds together (`arugula_e2e::passkey::check_chain`):
 * null, or the index of the first event that doesn't. */
export async function checkChain(log: PasskeyLog, origins: Origins): Promise<number | null> {
  const r = await liveAt(log, Number.MAX_SAFE_INTEGER, origins);
  return typeof r === "number" ? r : null;
}

/** Check `assertion` against `log`, trusting nothing else: the chain, the
 * passkey live at `at` (the verifier's own record of when it arrived), a
 * `webauthn.get` over exactly `challenge`, not framed, for the passkey's
 * RP from an origin `origins` allows for it, UP and UV, and the signature. */
export async function verify(
  log: PasskeyLog,
  assertion: Assertion,
  challenge: string,
  at: number,
  origins: Origins,
): Promise<Verdict> {
  const then = await liveAt(log, at, origins);
  if (typeof then === "number") return { ok: false, refused: "chain", event: then };
  const key = then.get(assertion.id);
  if (!key) return { ok: false, refused: "not_live" };
  return check(key, assertion, challenge, origins);
}

const no = (refused: Refused): Verdict => ({ ok: false, refused });

async function check(key: PasskeyEvent, a: Assertion, challenge: string, origins: Origins): Promise<Verdict> {
  const raw = unb64u(a.clientDataJSON);
  if (!raw) return no("client_data");
  let cd: { type?: unknown; challenge?: unknown; origin?: unknown; crossOrigin?: unknown; topOrigin?: unknown };
  try {
    cd = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(raw));
  } catch {
    return no("client_data");
  }
  if (!cd || typeof cd !== "object" || typeof cd.type !== "string" || typeof cd.challenge !== "string" || typeof cd.origin !== "string") {
    return no("client_data");
  }
  if ((cd.crossOrigin !== undefined && cd.crossOrigin !== null && typeof cd.crossOrigin !== "boolean") ||
      (cd.topOrigin !== undefined && cd.topOrigin !== null && typeof cd.topOrigin !== "string")) {
    return no("client_data");
  }
  if (cd.type !== "webauthn.get") return no("ceremony");
  if (cd.challenge !== challenge) return no("challenge");
  if (cd.crossOrigin === true || (typeof cd.topOrigin === "string" && cd.topOrigin !== cd.origin)) return no("framed");
  const data = unb64u(a.authenticatorData);
  if (!data || data.length < 37) return no("auth_data");
  const rpHash = hex(data.slice(0, 32));
  let rp: string | null = null;
  for (const h of key.rp.split(" ")) {
    if (hex(await sha256(h)) === rpHash) {
      rp = h;
      break;
    }
  }
  if (rp === null) return no("rp");
  if (!(origins[rp] ?? []).includes(cd.origin)) return no("origin");
  const flags = data[32];
  if (!(flags & UP) || !(flags & UV)) return no("user");
  const sig = unb64u(a.signature);
  if (!sig) return no("signature");
  const pub = unb64u(key.key);
  if (!pub) return no("key");
  const msg = new Uint8Array(data.length + 32);
  msg.set(data);
  msg.set(await sha256(raw), data.length);
  const r = await signed(key.alg, pub, msg, sig);
  if (r !== true) return no(r === false ? "signature" : r);
  return { ok: true, key, rp, flags };
}

async function signed(alg: number, pub: Uint8Array, msg: Uint8Array, sig: Uint8Array): Promise<boolean | "key"> {
  let k: CryptoKey;
  try {
    if (alg === ES256) k = await subtle.importKey("raw", pub as BufferSource, { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]);
    else if (alg === EDDSA) k = await subtle.importKey("raw", pub as BufferSource, { name: "Ed25519" }, false, ["verify"]);
    else if (alg === RS256) k = await subtle.importKey("spki", pub as BufferSource, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["verify"]);
    else return "key";
  } catch {
    return "key";
  }
  try {
    if (alg === ES256) {
      const rs = derToRaw(sig);
      return rs !== null && (await subtle.verify({ name: "ECDSA", hash: "SHA-256" }, k, rs as BufferSource, msg as BufferSource));
    }
    if (alg === EDDSA) return sig.length === 64 && (await subtle.verify({ name: "Ed25519" }, k, sig as BufferSource, msg as BufferSource));
    return await subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, k, sig as BufferSource, msg as BufferSource);
  } catch {
    return false;
  }
}

/** A DER ECDSA signature as WebCrypto's r‖s, or null. */
function derToRaw(der: Uint8Array): Uint8Array | null {
  if (der.length < 8 || der[0] !== 0x30 || der[1] !== der.length - 2) return null;
  let i = 2;
  const int = (): Uint8Array | null => {
    if (der[i++] !== 2) return null;
    const len = der[i++];
    if (len === undefined || len === 0 || i + len > der.length) return null;
    let v = der.slice(i, i + len);
    i += len;
    while (v.length > 32 && v[0] === 0) v = v.slice(1);
    if (v.length > 32) return null;
    const out = new Uint8Array(32);
    out.set(v, 32 - v.length);
    return out;
  };
  const r = int();
  const s = int();
  if (!r || !s || i !== der.length) return null;
  const raw = new Uint8Array(64);
  raw.set(r);
  raw.set(s, 32);
  return raw;
}
