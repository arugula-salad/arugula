// The page served by Arugula control (M17): who you are, this browser's
// place in your account, and the daemons you can reach.
//
// Signing in (GitHub) only opens control's API. What lets this browser
// reach a daemon is its device key, approved by a device the account
// already trusts (the first one is trusted on enrollment). When this
// browser is enrolled it pins the account's root device, and from then on
// believes only certificates that chain back to it: control hands out the
// directory and the keys, but it can't slip in a daemon of its own (and a
// daemon can't be reached by a device it hasn't approved).

import { certBody, DEVICE_SECRET, deviceId, deviceMac, deviceMacOk, devicePinBody, devicePinOk, evaluate, hex, JOIN_SECRET, joinCode, joinMac, newSecret, normalizeCode, normalizeSecret, splitJoinCode, type Cert, type Revocation, revocationBody, unhex } from "./e2e/cert.ts";
import {
  adoptKeys,
  adoptPrfKeys,
  existingKeys,
  forget,
  forgetEnrollment,
  freshKeys,
  loadEnrollment,
  loadKeys,
  PRF_SALTS,
  prfDevice,
  saveEnrollment,
  saveWorkerDirectory,
  signText,
  type DeviceKeys,
  type Enrollment,
} from "./e2e/keys.ts";
import { newKeyPair } from "./e2e/noise.ts";
import type { E2ETarget } from "./client";
import { desktopApp } from "./desktop";
import { automated, counting, startCounting, stopCounting, type PageCounts } from "./telemetry";
import {
  follows,
  inviteKey,
  moveBody,
  newInviteKey,
  redeemInvite,
  askBody,
  askOk,
  signAskInvite,
  signInvite,
  signRoster,
  teamJoinBody,
  word,
  type AccountCerts,
  type AskInvite,
  type Invite,
  type Roster,
  type TeamPin,
  type TeamRole,
} from "./e2e/team.ts";

export interface ControlInfo {
  control: true;
  cloud?: boolean;
  url: string;
  github: boolean;
  /** WebAuthn works here (control has a domain name, not an IP). */
  passkeys: boolean;
  /** Control's VAPID public key (M21). */
  vapid: string;
  /** Where control is now, when it answers at more than one URL (#507). */
  primary?: string;
  /** Control passes an approving device's MAC to a waiting one (#761),
   * so a waiting browser shows a code rather than a fingerprint. */
  device_secret?: number;
  /** Blocks get origins of their own, `https://b-<site_key>.<this>` (M50);
   * absent when control doesn't serve blocks. */
  block_domain?: string;
}

/** On a URL control is moving away from (#507): a line at the top saying
 * where it is now, with the same page there. This site keeps working for
 * now: its passkeys and approved browsers are its own, so the new one is
 * signed in and approved from here. */
export function showMoving(info: ControlInfo): void {
  if (!info.primary || info.primary === info.url) return;
  const to = new URL(location.pathname + location.search + location.hash, info.primary);
  const bar = document.createElement("div");
  bar.className = "control-dropped";
  bar.setAttribute("role", "status");
  bar.dataset.controlMoving = "";
  const p = document.createElement("p");
  const a = document.createElement("a");
  a.href = to.href;
  a.textContent = to.host;
  p.append("Control is moving to ", a, ". Sign in there, then approve that browser from this one; this address stops working once everyone has moved.");
  bar.append(p);
  document.body.prepend(bar);
}

export interface DirDaemon {
  id: string;
  name: string;
  urls: string[];
  online: boolean;
  last_seen: number | null;
  cert: Cert;
  /** A hosted sandbox's id (M20), if it is one. */
  sandbox?: string | null;
  /** Someone else's (M19): its owner account and their login, and the
   * team it belongs to, if any. */
  account?: string;
  owner_name?: string;
  team?: string | null;
}

/** Another account's machine as the directory lists it, with that account's
 * certificates to check it by. */
interface ForeignEntry extends Omit<DirDaemon, "cert"> {
  chain?: { trust: { account: string; root: string } | null; certs: Cert[]; revocations: Revocation[] };
}

export interface JoinRequest {
  code: string;
  cert: Cert;
  urls: string[];
  team: { team: string; name: string } | null;
  /** What the machine said it understands (#760: `join-secret`, its link
   * carries a secret). An older control leaves it out. */
  features?: string;
}

/** The machine made a join secret (#760): the approval answers it with a
 * MAC, from the whole code, and nothing is compared. */
export const takesSecret = (j: JoinRequest) => (j.features ?? "").split(",").includes(JOIN_SECRET);

export interface Team {
  team: string;
  pin: TeamPin;
  locked: boolean;
  roster: Roster;
  role: TeamRole | null;
  /** Who used an invite link and waits for an owner (owners only). Only
   * those whose link's one-time key vouches for them (#762): `by` is the
   * owner whose device made the link. */
  requests: { account: string; root: string; name: string; role: TeamRole; created: number; invite: AskInvite; proof: string; by: string }[];
  certs: AccountCerts;
  /** Members' names as they set them, by account (#208): the roster's
   * one-word form ("Sam-Stranger") is only what's signed. */
  names?: Record<string, string>;
  /** The founder is the one this browser pinned on first sight. */
  verified: boolean;
}

/** A one-click invite link control still holds (#134). */
export interface PresignedInvite {
  key: string;
  role: TeamRole;
  expires: number;
  by: string;
  by_name: string;
}

const PINS_KEY = "arugula.control.pins";

/** #761: the secret this browser shows while it waits, kept with its key
 * id so a reload shows the same code (one already scanned still works). */
const SECRET_KEY = "arugula.control.device-secret";

function deviceSecretFor(id: string): string {
  try {
    const kept = JSON.parse(localStorage.getItem(SECRET_KEY) ?? "null") as { id?: string; secret?: string } | null;
    const secret = kept?.id === id && kept.secret ? normalizeSecret(kept.secret) : null;
    if (secret) return secret;
  } catch {
    // a new one
  }
  const secret = newSecret();
  try {
    localStorage.setItem(SECRET_KEY, JSON.stringify({ id, secret }));
  } catch {
    // A reload shows a new code.
  }
  return secret;
}

function dropDeviceSecret() {
  try {
    localStorage.removeItem(SECRET_KEY);
  } catch {
    // nothing kept
  }
}

/** First sight of another account's root (trust on first use): pinned
 * here, and a different one later is refused, not believed. */
function pins(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(PINS_KEY) ?? "{}");
  } catch {
    return {};
  }
}

function pin(account: string, root: string, at = Date.now()): boolean {
  const p = pins();
  if (p[account] && p[account] !== root) return false;
  if (!p[account]) {
    p[account] = root;
    try {
      localStorage.setItem(PINS_KEY, JSON.stringify(p));
    } catch {
      // not remembered
    }
    setPinMeta(account, { at });
  }
  return true;
}

/** #763: about a pin of `PINS_KEY`: when it was made, the root its
 * person was verified with (scanned or words compared), and when this
 * browser first saw control name another one. Apart from the pins, so a
 * page from before reads them as it did. */
const PIN_META_KEY = "arugula.control.pin-meta";

interface PinMeta {
  at?: number;
  verified?: string;
  seen?: { root: string; at: number };
}

function pinMetas(): Record<string, PinMeta> {
  try {
    return JSON.parse(localStorage.getItem(PIN_META_KEY) ?? "{}");
  } catch {
    return {};
  }
}

function setPinMeta(account: string, m: PinMeta | null) {
  const all = pinMetas();
  if (m) all[account] = m;
  else delete all[account];
  try {
    localStorage.setItem(PIN_META_KEY, JSON.stringify(all));
  } catch {
    // not remembered
  }
}

/** Someone you share with (#763), as this browser pinned them: their root,
 * when it was pinned (if known) and whether you verified it. */
export interface PersonPin {
  root: string;
  at?: number;
  verified: boolean;
}

export function personPin(account: string): PersonPin | null {
  const root = pins()[account];
  if (!root) return null;
  const m = pinMetas()[account] ?? {};
  return { root, at: m.at, verified: m.verified === root };
}

/** A new key control named for `account` that this browser hasn't taken
 * (#763): the root and when it was first seen, or null. */
export function keyChanged(account: string): { root: string; at: number } | null {
  const root = pins()[account];
  const seen = pinMetas()[account]?.seen;
  return root && seen && seen.root !== root ? seen : null;
}

/** Control names `root` for `account`, which this browser pinned as
 * another: when it first saw that (kept, so the date doesn't move). */
export function keyChangeSeen(account: string, root: string): number {
  const m = pinMetas()[account] ?? {};
  if (m.seen?.root === root) return m.seen.at;
  const at = Date.now();
  setPinMeta(account, { ...m, seen: { root, at } });
  return at;
}

/** Trust on first use, for a share: pin `root` unless one is pinned, from
 * `at` (when a machine's grant says it was first shared). False if
 * another root is pinned. */
export function pinPerson(account: string, root: string, at?: number): boolean {
  return pin(account, root, at);
}

/** Verified with them, by their code or words (#763): `root` is theirs,
 * pinned in place of any other. */
export function verifyPerson(account: string, root: string) {
  const p = pins();
  const was = p[account];
  p[account] = root;
  try {
    localStorage.setItem(PINS_KEY, JSON.stringify(p));
  } catch {
    // not remembered
  }
  const m = pinMetas()[account] ?? {};
  setPinMeta(account, { at: was === root ? (m.at ?? Date.now()) : Date.now(), verified: root });
}

/** Served by control? (A daemon answers 404.) */
export async function detectControl(): Promise<ControlInfo | null> {
  try {
    const res = await fetch("/control.json", { cache: "no-store" });
    if (!res.ok) return null;
    const j = (await res.json()) as Partial<ControlInfo>;
    return j.control ? (j as ControlInfo) : null;
  } catch {
    return null;
  }
}

class HttpError extends Error {
  status: number;
  body: Record<string, unknown>;
  constructor(status: number, message: string, body: Record<string, unknown> = {}) {
    super(message);
    this.status = status;
    this.body = body;
  }
}

/** An approval control refused (#327): its reason code, and what to do. */
export class RefusedError extends Error {
  reason: string;
  constructor(reason: string, message: string) {
    super(message);
    this.reason = reason;
  }
}

/** The machine was approved, but a device approved with it wasn't
 * (#326): the machine's prompt is done, and that device asks on its own. */
export class AlongsideError extends Error {
  constructor(name: string, why: string) {
    super(`The machine is approved, but not ${name}: ${why}`);
  }
}

/** `GET /api/flags` on control (#347). */
export type LabsFlags = {
  account: Record<string, boolean>;
  teams: Record<string, { flags: Record<string, boolean>; owner: boolean }>;
  /** A machine of the account has Developer settings: only then does the page offer these (strangers never see Labs). */
  dev?: boolean;
};

export async function api<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new HttpError(res.status, j.error ?? `HTTP ${res.status}`, j as Record<string, unknown>);
  return j;
}

/** M104: the account's answer for this page (control-wire's
 * `telemetry::PageConsent`). */
export interface PageConsent {
  on: boolean | null;
  at: number | null;
  /** This control forwards reports: only then is the question asked. */
  ask: boolean;
}

/** This page's counts to control, under the signed-in account. A
 * `keepalive` fetch as the page goes away, so it still arrives. Control
 * saying no (the account turned it off elsewhere) stops the counting. */
async function sendPageCounts(c: PageCounts, leaving: boolean): Promise<boolean> {
  const res = await fetch("/api/telemetry/page", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(c),
    keepalive: leaving,
  });
  return res.status !== 403;
}

const DIR_KEY = "arugula.control.directory";

const b64u = (b: ArrayBuffer | Uint8Array) =>
  btoa(String.fromCharCode(...new Uint8Array(b instanceof Uint8Array ? b : new Uint8Array(b))))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
const unb64u = (s: string) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));

/** #764: the `prf` extension, evaluated at both salts in the same
 * ceremony (`first` for the X25519 key, `second` for Ed25519). */
const PRF_ASK = { prf: { eval: { first: PRF_SALTS.noise, second: PRF_SALTS.sign } } } as AuthenticationExtensionsClientInputs;

interface PrfOut {
  enabled?: boolean;
  results?: { first?: ArrayBuffer; second?: ArrayBuffer };
}

const prfOf = (cred: PublicKeyCredential) => (cred.getClientExtensionResults() as { prf?: PrfOut }).prf;

/** #764: a passkey's PRF output as this browser's device keys, the same on
 * every browser with that passkey, so they're one device. Not while this
 * browser is a working device of that account already: it keeps its own.
 * Whether it took them. */
async function takePrfKeys(prf: PrfOut | undefined, account: string, provider: string | null | undefined): Promise<boolean> {
  const r = prf?.results;
  if (!r?.first || !r.second) return false;
  const e = await loadEnrollment(location.origin);
  const have = await existingKeys();
  if (e && e.account === account && have?.id === e.cert.device) return false;
  try {
    await adoptPrfKeys(r.first, r.second, provider ? `${provider} passkey` : "Passkey");
  } catch (err) {
    // This browser can't keep them: it's approved as before.
    console.warn("arugula: not using the passkey's keys:", err);
    return false;
  }
  // Whatever it was enrolled as, it's that device now.
  if (e) await forgetEnrollment(location.origin);
  return true;
}

/** Sign in with a passkey (any registered on this control). Whether this
 * browser took device keys from it (#764). */
export async function passkeySignIn(): Promise<boolean> {
  const o = await api<{ challenge: string; rpId: string; userVerification: UserVerificationRequirement; timeout: number }>("/auth/passkey/login", {});
  const cred = (await navigator.credentials.get({
    publicKey: { challenge: unb64u(o.challenge), rpId: o.rpId, userVerification: o.userVerification, timeout: o.timeout, extensions: PRF_ASK },
  })) as PublicKeyCredential | null;
  if (!cred) throw new Error("no passkey chosen");
  const r = cred.response as AuthenticatorAssertionResponse;
  const prf = prfOf(cred);
  const done = await api<{ account?: string; provider?: string | null }>("/auth/passkey/login/finish", {
    id: b64u(cred.rawId),
    clientDataJSON: b64u(r.clientDataJSON),
    authenticatorData: b64u(r.authenticatorData),
    signature: b64u(r.signature),
    prf: !!(prf?.results?.first && prf.results.second),
  });
  return done.account ? takePrfKeys(prf, done.account, done.provider) : false;
}

/** Make a passkey: for the account signed in, or a new account called
 * `name` (#102: what teammates see). */
export async function passkeyRegister(name?: string): Promise<void> {
  type Options = {
    challenge: string;
    rp: PublicKeyCredentialRpEntity;
    user: { id: string; name: string; displayName: string };
    pubKeyCredParams: PublicKeyCredentialParameters[];
    authenticatorSelection: AuthenticatorSelectionCriteria;
    attestation: AttestationConveyancePreference;
    timeout: number;
  };
  const o = await api<Options>("/auth/passkey/register", name === undefined ? {} : { name });
  const cred = (await navigator.credentials.create({
    publicKey: { ...o, challenge: unb64u(o.challenge), user: { ...o.user, id: unb64u(o.user.id) }, extensions: PRF_ASK },
  })) as PublicKeyCredential | null;
  if (!cred) throw new Error("no passkey made");
  const r = cred.response as AuthenticatorAttestationResponse;
  // #764: some authenticators give PRF output as the passkey is made;
  // others say only that they will, at sign-in.
  const prf = prfOf(cred);
  const done = await api<{ account?: string; provider?: string | null }>("/auth/passkey/register/finish", {
    id: b64u(cred.rawId),
    clientDataJSON: b64u(r.clientDataJSON),
    attestationObject: b64u(r.attestationObject),
    prf: prf?.enabled ?? (prf?.results ? true : undefined),
  });
  if (done.account) await takePrfKeys(prf, done.account, done.provider);
}

/** What a signed-out page may know about an invite (#103): the team's
 * name and who made it. */
export async function previewInvite(i: LinkInvite): Promise<{ name: string; by: string } | null> {
  if (i.old) return null;
  const key = await inviteKey(i.code).then((k) => k.key, () => null);
  if (!key) return null;
  return api<{ name: string; by: string }>(i.presigned ? `/api/presigned/${i.team}/${key}/preview` : `/api/invites/${i.team}/${key}/preview`).catch(() => null);
}

/** A team invite in a link's fragment. Both kinds carry a one-time key's
 * seed (`code`), which control never sees: `#pinvite=<team>.<seed>` is
 * presigned, and `#askinvite=<team>.<seed>` asks an owner first (#762).
 * Pages from before don't know these, so they never send the seed to
 * control as a code. `#invite=<team>.<code>` is a link from before #762
 * (`old`), which no longer works. */
export interface LinkInvite {
  team: string;
  code: string;
  presigned: boolean;
  old: boolean;
}

export function inviteInHash(hash: string): LinkInvite | null {
  const m = /^#(p|ask)?invite=([0-9a-f]+)\.([0-9a-f]+)$/.exec(hash);
  return m ? { team: m[2], code: m[3], presigned: m[1] === "p", old: !m[1] } : null;
}

/** A link whose fragment holds an invite's seed. */
const seedInHash = (hash: string) => {
  const i = inviteInHash(hash);
  return !!i && !i.old;
};

const PENDING_INVITE = "arugula:presigned-invite";
/** How long a kept join link counts (a join lasts ten minutes). */
const KEPT_JOIN_MS = 15 * 60 * 1000;

/** Where signing in with GitHub comes back to: this page, but never with a
 * an invite's seed (#762) or a machine's join secret (#760), which control
 * mustn't see. The link waits in this tab's sessionStorage for
 * `restoreInvite`; a join (or #761, a waiting device's code) comes back
 * with its code alone meanwhile, so the page still says what it's for. */
export function signInNext(): string {
  const join = JOIN_HASH.exec(location.hash);
  // #761: a waiting device's code carries its secret too, on its own
  // (`#device=`) or with the machine it came to approve (`&device=`).
  const secret = join ? splitJoinCode(join[1])?.secret || join[2] : DEVICE_HASH.test(location.hash);
  if (!seedInHash(location.hash) && !secret) return location.pathname + location.hash;
  try {
    sessionStorage.setItem(PENDING_INVITE, JSON.stringify({ hash: location.hash, at: Date.now() }));
  } catch {
    // Without storage the link has to be opened again after signing in.
  }
  const device = join ? null : splitDeviceCode(location.hash);
  return location.pathname + (join ? `#join=${joinInHash()}` : device ? `#device=${device.code}` : "");
}

/** Back from signing in: the invite link with its seed (presigned or, since
 * #762, Ask me first), or the machine's whole join link, that `signInNext`
 * kept. */
export function restoreInvite() {
  try {
    const kept = sessionStorage.getItem(PENDING_INVITE);
    // Used or not, it's read once.
    sessionStorage.removeItem(PENDING_INVITE);
    const { hash, at } = JSON.parse(kept ?? "null") as { hash: string; at: number };
    if (typeof hash !== "string") return;
    // A join comes back as its code alone (`signInNext`), soon: then the
    // kept link has its secret. A link that brings its own secret wins,
    // and so does any link once the kept one is older than a join lasts
    // (a retried join has the same code and a new secret, #760). A
    // waiting device's code (#761) comes back the same way, as
    // `#device=LOOKUP`.
    const fresh = Date.now() - at < KEPT_JOIN_MS;
    const join = joinInHash(hash) !== null && wholeJoinInHash() === joinInHash(hash) && fresh;
    const lookup = DEVICE_HASH.test(hash) ? splitDeviceCode(hash)?.code : null;
    const device = !!lookup && location.hash === `#device=${lookup}` && fresh;
    if ((!location.hash && seedInHash(hash)) || join || device) history.replaceState(null, "", location.pathname + location.search + hash);
  } catch {
    // Nothing kept, or not in this shape.
  }
}

/** How long a presigned invite lasts unless the owner says otherwise. */
const PRESIGNED_TTL_MS = 24 * 3600_000;

/** How long an Ask me first link lasts. */
const ASK_TTL_MS = 7 * 24 * 3600_000;

export type Phase = "loading" | "signed-out" | "waiting" | "turned-down" | "lost-key" | "untrusted" | "unvouched" | "ready" | "error";

/** What the person reads when control refuses an approval (#327), by its
 * reason code: which check failed, and what to do next. `revoked` isn't
 * here: control's own words say it (#330), and a removed machine asks
 * again with a new key by itself. */
const REFUSED: Record<string, string> = {
  approver_untrusted:
    "This browser isn't one your account trusts any more, so its approvals are refused. Forget it and enroll it again (another of your devices or a recovery code approves it), then try again.",
  bad_signature: "This browser's key isn't the one your account approved. Forget it and enroll it again, then try again.",
  cant_approve: "A machine can't approve devices. Approve from a browser, phone or the Arugula CLI.",
  recovery_for_machine: "A recovery code approves browsers and phones, not machines. Approve the machine from one of your devices.",
  no_chain: "Control's records for your account don't add up from here (the approval doesn't chain to your first device). Reload and try again; if it keeps happening, approve from another device.",
  no_devices: "This account has no devices yet. Reload: this browser becomes its first.",
};

/** Refusals that mean this browser itself isn't trusted. */
const SELF_REFUSED = new Set(["approver_untrusted", "bad_signature"]);

/** Where `enrollAgain` leaves a note for the next page: open the recovery
 * form (#327). */
const RECOVER_KEY = "arugula.control.recover";

/** Where `allowAppLogin` notes that it signed the app's certificate, for
 * the page the app sends this tab back to (#758). */
const APP_SIGNED_KEY = "arugula.control.app-signed";

let appSigned: boolean | null = null;

/** Whether this tab just signed the desktop app in as a device. Taken out
 * of sessionStorage on the first call, so a later reload doesn't say it
 * again, and kept for the rest of this page load: the prompts mount afresh
 * when the page picks a machine (`ControlRoot` is keyed by the client's
 * base), and the second mount must say the same. */
export function signedAppIn(): boolean {
  if (appSigned !== null) return appSigned;
  try {
    appSigned = sessionStorage.getItem(APP_SIGNED_KEY) === "1";
    sessionStorage.removeItem(APP_SIGNED_KEY);
  } catch {
    appSigned = false;
  }
  return appSigned;
}

/** A desktop app asking to sign in (`GET /api/app-login/{id}`). */
export interface AppLogin {
  name: string;
  allowed: boolean;
  from: string;
  same_network: boolean;
  /** Only from an app before #758: a code it shows too. */
  code?: string;
  /** From #758 on: the public half of the app's device keys, hex. */
  device?: { noise: string; sign: string };
}

/** The page came from forgetting a stale browser: offer the recovery code
 * at once. Read once. */
export function cameToRecover(): boolean {
  try {
    const yes = sessionStorage.getItem(RECOVER_KEY) === "1";
    sessionStorage.removeItem(RECOVER_KEY);
    return yes;
  } catch {
    return false;
  }
}

// ---- recovery codes: an Ed25519 seed each, on paper only.

const B32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const PKCS8_ED25519 = unhex("302e020100300506032b657004220420");

function toCode(seed: Uint8Array): string {
  let bits = 0;
  let val = 0;
  let out = "";
  for (const b of seed) {
    val = (val << 8) | b;
    bits += 8;
    while (bits >= 5) {
      out += B32[(val >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits) out += B32[(val << (5 - bits)) & 31];
  return out.match(/.{1,4}/g)!.join("-");
}

function fromCode(code: string): Uint8Array<ArrayBuffer> | null {
  const c = code.toUpperCase().replace(/[^A-Z2-7]/g, "");
  if (c.length !== 52) return null;
  const out: number[] = [];
  let bits = 0;
  let val = 0;
  for (const ch of c) {
    val = (val << 5) | B32.indexOf(ch);
    bits += 5;
    if (bits >= 8) {
      out.push((val >>> (bits - 8)) & 255);
      bits -= 8;
    }
  }
  return Uint8Array.from(out.slice(0, 32));
}

const subtle = globalThis.crypto.subtle;

async function recoveryKey(seed: Uint8Array): Promise<CryptoKey> {
  const pkcs8 = new Uint8Array(48);
  pkcs8.set(PKCS8_ED25519);
  pkcs8.set(seed, 16);
  return subtle.importKey("pkcs8", pkcs8, { name: "Ed25519" }, false, ["sign"]);
}

const NO_NOISE = "0".repeat(64);

/** A browser's name in its account's device list. */
export function deviceName(): string {
  // M48: the desktop app says what it is ("Arugula app on jake-air").
  const app0 = desktopApp()?.name;
  if (app0) return app0;
  // The app's window without its script's name (#817): Tauri's own
  // bridge still says it's the app, whatever its WebKit's User-Agent.
  if ("__TAURI_INTERNALS__" in globalThis) return "Arugula app";
  return agentName(navigator.userAgent);
}

/** "Safari on iPhone", from a User-Agent: what a browser calls itself as a
 * device, and what Sign-in calls a sign-in no device has claimed, so the
 * two lists use the same words. */
export function agentName(ua: string): string {
  if (/arugula/i.test(ua) && !/Mozilla/.test(ua)) return "Arugula app";
  const os = /iPhone/.test(ua)
    ? "iPhone"
    : /iPad/.test(ua)
      ? "iPad"
      : /Android/.test(ua)
        ? "Android"
        : /Mac/.test(ua)
          ? "Mac"
          : /Windows/.test(ua)
            ? "Windows"
            : /Linux|X11/.test(ua)
              ? "Linux"
              : "";
  const app = /Edg\//.test(ua)
    ? "Edge"
    : /Firefox\//.test(ua)
      ? "Firefox"
      : /Chrome\//.test(ua)
        ? "Chrome"
        : /Epiphany\//.test(ua)
          ? "GNOME Web"
          : /Safari\//.test(ua)
            ? // Safari's token off Apple's systems is WebKitGTK: the Linux
              // app's window (when it hasn't said its own name) or GNOME
              // Web, never Safari (#817).
              os === "Linux"
              ? "WebKit"
              : "Safari"
            : /node|undici|curl|reqwest/i.test(ua)
              ? "A program"
              : "";
  if (!app && !os) return "A browser";
  return app && os ? `${app} on ${os}` : app || `A browser on ${os}`;
}

/** A device's name as the lists show it. The app was called illogical
 * before 0.26: its devices from then keep the old name in their
 * certificates, so they read as Arugula's. */
export function shownName(name: string): string {
  return name.replace(/^illogical app\b/, "Arugula app");
}

/** The whole join code in `#join=CODE` or `#join=CODE.SECRET` (#760), as
 * it is there; and (#761) the code of the browser that came to approve
 * the machine and waits to be approved with it, `&device=LOOKUP.SECRET`. */
export const JOIN_HASH = /^#join=([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)?)(?:&device=([A-Za-z0-9-]+\.[A-Za-z0-9-]+))?$/;

/** A waiting device's code in a link (#761): `#device=LOOKUP.SECRET`,
 * what its QR code says. */
export const DEVICE_HASH = /^#device=([A-Za-z0-9-]+\.[A-Za-z0-9-]+)$/;

/** A waiting device's code, typed or from a link (`LOOKUP.SECRET`, or the
 * whole link): its lookup and its secret, normalized; null if it isn't
 * one. */
export interface DeviceCode {
  code: string;
  secret: string;
}

export function splitDeviceCode(s: string): DeviceCode | null {
  const t = s.trim();
  const m = /#device=([^&\s]+)/.exec(t) ?? /&device=([^&\s]+)/.exec(t);
  const parts = splitJoinCode(m ? decodeURIComponent(m[1]) : t);
  return parts?.secret ? { code: parts.code, secret: parts.secret } : null;
}

/** The machine's join code this page was opened to approve
 * (`#join=CODE`), if any: the code only, which is all control is told. */
export function joinInHash(hash: string = location.hash): string | null {
  const m = JOIN_HASH.exec(hash);
  return m ? (splitJoinCode(m[1])?.code ?? null) : null;
}

/** The whole code in the hash, the secret too (#760), for the link a
 * waiting browser hands on: `CODE.SECRET`, or just `CODE`. */
export function wholeJoinInHash(hash: string = location.hash): string | null {
  const m = JOIN_HASH.exec(hash);
  const j = m ? splitJoinCode(m[1]) : null;
  return j ? (j.secret ? `${j.code}.${j.secret}` : j.code) : null;
}

/** Whether this page is the desktop app's window (M48). */
export function inApp(): boolean {
  return !!desktopApp();
}

/** What the browser that allowed the desktop app's sign-in hands the app
 * (#758): a certificate for the app's keys, signed by that browser, and
 * the account root it pinned. */
export interface AppEnrollment {
  cert: Cert;
  root: string;
}

/** The app's keys (PKCS#8) and what came with them, from the redeem link. */
interface AppHandOff extends AppEnrollment {
  noise: Uint8Array<ArrayBuffer>;
  sign: Uint8Array<ArrayBuffer>;
}

const b64url = (s: string) => btoa(String.fromCharCode(...new TextEncoder().encode(s))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const unb64url = (s: string) => new TextDecoder().decode(Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)));

/** M48: the desktop app's window, after the person allowed its sign-in in
 * their browser: `#app-redeem=<ticket>.<grant>.<verifier>` from the app,
 * posted here for the session cookie. Only in the app's own window (a link
 * to this in a browser would sign it in as someone else). #758: when the
 * browser that allowed it signed the app's certificate, the link also
 * carries the keys the app made (`.<noise>.<sign>.<cert>`): the window
 * takes them as its own. */
async function redeemAppLogin(): Promise<AppHandOff | null> {
  const m = /^#app-redeem=([0-9a-f]+)\.([0-9a-f]+)\.([0-9a-f]+)(?:\.([0-9a-f]+)\.([0-9a-f]+)\.([A-Za-z0-9_-]+))?$/.exec(location.hash);
  if (!m) return null;
  history.replaceState(null, "", location.pathname + location.search);
  if (!inApp()) return null;
  await api(`/auth/app/${m[1]}/redeem`, { grant: m[2], verifier: m[3] }).catch(() => {});
  if (!m[6]) return null;
  try {
    const e = JSON.parse(unb64url(m[6])) as AppEnrollment;
    if (typeof e.root !== "string" || typeof e.cert?.device !== "string") return null;
    return { cert: e.cert, root: e.root, noise: unhex(m[4]), sign: unhex(m[5]) };
  } catch {
    return null;
  }
}

/** Someone offering to share a session on their machine with this
 * account: it isn't listed, or let in, until accepted. */
export interface ShareOffer {
  daemon: string;
  /** The machine's name, as its owner's machine says. */
  name: string;
  account: string;
  owner_name: string;
  owner_login: string;
  /** #857: the conversation they were added to, when that's what it is. */
  conv?: string | null;
}

export class ControlSession {
  phase: Phase = "loading";
  error = "";
  /** What to call the account here: its name, else its login. */
  login = "";
  /** What other people see (#102): the name it chose, or its GitHub
   * login. Empty for a passkey account made before names. */
  name = "";
  account = "";
  /** Passkeys registered to the account. */
  passkeys = 0;
  /** Shown once, right after the account's first device enrolls (or new
   * ones are made). */
  recoveryCodes: string[] | null = null;
  /** While turned down: the name of the device that did it. */
  turnedDownBy = "";
  /** Asks again after a turn-down. */
  private askAgain: (() => void) | null = null;
  keys!: DeviceKeys;
  /** #764: this browser's keys came from a passkey's PRF, and this is
   * the name its certificate takes ("iCloud Keychain passkey"). */
  fromPasskey: string | null = null;
  enrollment: Enrollment | undefined;
  /** Every device the account trusts, by this browser's reckoning. */
  trusted = new Map<string, Cert>();
  /** Devices asking to join, for this one to approve. */
  pending: Cert[] = [];
  /** #326: the machine's join code a waiting device came to approve, by
   * the device's id, while that join is open: approved together. */
  pendingJoins = new Map<string, string>();
  /** #761: each waiting device's lookup (its keys' code), by its id, and
   * those that show a code with a secret (approved with a MAC, from the
   * code scanned or typed, not by comparing a fingerprint). */
  pendingCodes = new Map<string, string>();
  pendingSealed = new Set<string>();
  /** #761: while this browser waits, the code it shows
   * (`LOOKUP.SECRET`); null when control doesn't take one (it shows its
   * fingerprint instead). */
  deviceCode: string | null = null;
  /** #761, in the `unvouched` phase: approved, but not with this
   * browser's code. `missing`: by a page from before codes (the person
   * checks the account's fingerprint instead); `wrong`: a MAC that doesn't
   * check out, refused. */
  vouch: { why: "missing" | "wrong"; root: string; by: string } | null = null;
  private vouchOk: (() => void) | null = null;
  /** When each device last loaded the page, as control saw (none before it
   * kept it, or from an older control). */
  used = new Map<string, number>();
  /** #764: devices whose keys come from a passkey, as control says (they
   * pinned their root): one device on every browser with that passkey. */
  passkeyDevices = new Set<string>();
  revocations: Revocation[] = [];
  daemons: DirDaemon[] = [];
  /** Shares waiting for this account's yes. */
  offers: ShareOffer[] = [];
  /** The directory is the saved one: control didn't answer. */
  stale = false;
  /** Control says the account's root is a different device than the one
   * this browser pinned: don't trust anything new from it. */
  rootMismatch = false;
  /** Why the account doesn't trust this browser (#327), in the
   * `untrusted` phase: a device removed it, or it's missing. */
  untrustedWhy: "removed" | "missing" = "missing";
  private listeners = new Set<() => void>();
  private timer: number | undefined;
  readonly info: ControlInfo;

  constructor(info: ControlInfo) {
    this.info = info;
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private emit() {
    for (const fn of this.listeners) fn();
  }

  private set(phase: Phase, error = "") {
    this.phase = phase;
    this.error = error;
    this.emit();
  }

  /** Sign-in state, enrollment, then the directory, kept fresh. */
  async boot() {
    try {
      const handed = await redeemAppLogin();
      this.keys = await loadKeys();
      this.enrollment = await loadEnrollment(location.origin);
      // #758: the app's window, handed its keys and a certificate for them
      // by the browser that allowed its sign-in. Taken unless the window is
      // already a working device of that account.
      const e = this.enrollment;
      if (handed && (!e || e.account !== handed.cert.account || e.cert.device !== this.keys.id)) {
        try {
          this.keys = await adoptKeys(handed.noise, handed.sign, handed.cert.noise, handed.cert.sign);
          this.handed = handed;
          if (e?.cert.device !== this.keys.id) this.enrollment = undefined;
        } catch (err) {
          // Not keys for that certificate (or this webview can't keep
          // them): ask to be approved, as before.
          console.warn("arugula: not using the keys the sign-in handed over:", err);
          this.keys = await loadKeys();
        }
      }
      const prf = await prfDevice();
      this.fromPasskey = prf?.device === this.keys.id ? prf.name : null;
      if (this.enrollment && this.enrollment.cert.device !== this.keys.id) {
        // #94: the key this browser enrolled with didn't come back from
        // its storage (WebKit loses X25519 keys there), so it would sign
        // approvals nobody trusts. Say so, not fail later.
        return this.set("lost-key");
      }
      const me = await api<{ account: string; login: string; name?: string; root: string | null; passkeys: number }>("/api/me").catch((e) => {
        if (e instanceof HttpError && e.status === 401) return null;
        throw e;
      });
      if (!me) {
        // Signed out, but still enrolled: the cached directory keeps known
        // daemons reachable directly while control is down.
        return this.set("signed-out");
      }
      this.name = me.name ?? "";
      this.login = this.name || me.login || "you";
      this.account = me.account;
      this.passkeys = me.passkeys;
      if (this.enrollment && this.enrollment.account !== me.account) {
        // Signed in as someone else: this browser's place was in another
        // account. Start over as a new device of this one.
        this.enrollment = undefined;
      }
      if (!this.enrollment) await this.enroll(me.root);
      if (!this.enrollment) return;
      if (this.usedRecovery) {
        // Spent: nobody gets in with it again.
        await this.revoke(this.usedRecovery).catch(() => {});
        this.usedRecovery = null;
      }
      // Which device this sign-in is, for Sign-in and Devices to name
      // it the same (a control without the route says 404).
      void api("/api/me/sessions/device", { device: this.keys.id }).catch(() => {});
      await this.refresh();
      // #327: a browser the account no longer trusts says so at once.
      this.set(this.phase === "untrusted" ? "untrusted" : "ready");
      this.timer = window.setInterval(() => void this.refresh(), 10_000);
      // M104: the account's answer about usage counts from this page.
      void this.loadTelemetry();
    } catch (e) {
      this.set("error", String((e as Error).message ?? e));
    }
  }

  private async enroll(root: string | null) {
    // #764: keys from a passkey another browser enrolled already: this
    // browser is that device, with no approval.
    if (this.fromPasskey && root !== null && (await this.rejoin())) return;
    const k = this.keys;
    const cert: Cert = {
      v: 1,
      account: this.account,
      device: k.id,
      kind: "browser",
      name: this.fromPasskey ?? deviceName(),
      noise: k.noisePub,
      sign: k.signPub,
      created: Date.now(),
      approver: "",
      sig: "",
    };
    if (root === null) {
      // The account's first device signs itself.
      cert.approver = k.id;
      cert.sig = await signText(k, certBody(cert));
    }
    // #758: signed already by the browser that allowed the app's sign-in.
    const handed = this.handed;
    this.handed = null;
    const signed = root !== null && handed?.cert.device === k.id && handed.cert.account === this.account ? handed : null;
    this.request = cert;
    // #761: a code to show while it waits, whose secret control never
    // sees: the approving device answers it, and this browser checks the
    // answer before it pins the root. Not for the first device (nothing
    // approves it), nor one the app's sign-in signed already.
    const secret = root !== null && !signed && this.info.device_secret ? deviceSecretFor(k.id) : null;
    this.deviceCode = secret ? `${await joinCode(cert)}.${secret}` : null;
    // #326: here to approve a machine, which this browser can't until it's
    // one of the account's devices: the device that approves it is shown
    // the machine alongside, and approves both at once.
    let join = joinInHash();
    const ask = () => api<{ approved: boolean; cert?: Cert }>("/api/devices", { cert, join, ...(secret ? { features: DEVICE_SECRET } : {}) });
    let r = signed ? await api<{ approved: boolean; cert?: Cert }>("/api/devices", { cert: signed.cert }) : await ask();
    if (signed && !r.approved) {
      // An older control keeps it waiting: approve it with the same
      // signature (a control that refused it leaves it to a device).
      await api(`/api/devices/${k.id}/approve`, { cert: signed.cert }).catch(() => {});
      r = await api<{ approved: boolean; cert: Cert }>(`/api/devices/${k.id}`).catch(() => r);
    }
    if (!r.approved) this.brought = join;
    while (!r.approved) {
      this.set("waiting");
      await new Promise((res) => setTimeout(res, 2000));
      // Another machine's link followed in this tab (only the hash changes,
      // so nothing reloads), or the link left: ask again with that join, so
      // the approving device sees this machine alongside, not the last one.
      const now = joinInHash();
      if (now !== join) {
        const was = join;
        join = now;
        try {
          r = await ask();
          this.brought = join;
        } catch {
          // Asked again on the next turn.
          join = was;
        }
        continue;
      }
      try {
        r = await api<{ approved: boolean; cert: Cert }>(`/api/devices/${k.id}`);
      } catch (e) {
        if (!(e instanceof HttpError && e.status === 404)) continue;
        // Turned down (#105): say so, until this browser asks again.
        this.turnedDownBy = typeof e.body.by === "string" ? e.body.by : "";
        this.set("turned-down");
        await new Promise<void>((res) => (this.askAgain = res));
        this.askAgain = null;
        r = await ask();
      }
    }
    if (root === null) await this.makeRecoveryCodes();
    // Pin the root as control reports it now, and check that our own
    // certificate chains back to it.
    const all = await api<{ trust: { account: string; root: string }; certs: Cert[]; revocations: Revocation[] }>("/api/devices");
    // #758: the app's window takes the root the browser that signed it
    // pinned, not control's word for it.
    if (signed && all.trust.root !== signed.root) throw new Error("control says this account's root is a different device than the browser that signed this app in has: not trusting it");
    // #761: the device that approved it answered its code, for this root.
    // A recovery code approves it without one (this browser holds that).
    if (secret && !this.usedRecovery && !(await this.vouched(secret, cert, all))) return;
    const trusted = await evaluate(all.trust, all.certs, all.revocations);
    const mine = trusted.get(k.id);
    if (!mine) throw new Error("control approved this device, but the approval doesn't check out");
    this.enrollment = { control: location.origin, account: this.account, root: all.trust.root, cert: mine };
    await saveEnrollment(this.enrollment);
    dropDeviceSecret();
    this.deviceCode = null;
    if (this.fromPasskey) await this.savePin();
  }

  /** #764: this browser's keys came from a passkey. If the account has
   * that device (another browser with the passkey enrolled it), this
   * browser is it, once the device's pin shows the root is the one that
   * device pinned, not just control's word for it. True if so. A device
   * that was removed, or whose pin doesn't check out, gives way to keys of
   * this browser's own, approved as any new browser is; a device the
   * account doesn't have yet is approved once, then pinned. */
  private async rejoin(): Promise<boolean> {
    const k = this.keys;
    const all = await api<{ trust: { account: string; root: string } | null; certs: Cert[]; revocations: Revocation[] }>("/api/devices");
    const removed = all.revocations.some((r) => r.device === k.id);
    if (!removed && !all.certs.some((c) => c.device === k.id)) return false;
    const pin = removed ? undefined : (await api<{ approved: boolean; pin?: string }>(`/api/devices/${k.id}`).catch(() => null))?.pin;
    const pinned = !!all.trust && (all.trust.root === k.id || (!!pin && (await devicePinOk(all.trust, k.id, k.signPub, pin))));
    const mine = pinned && all.trust ? (await evaluate(all.trust, all.certs, all.revocations)).get(k.id) : undefined;
    if (!mine || !all.trust) {
      console.warn(`arugula: not this account's passkey device (${removed ? "removed" : "its pin doesn't check out"}): asking to be approved instead`);
      this.keys = await freshKeys();
      this.fromPasskey = null;
      return false;
    }
    this.enrollment = { control: location.origin, account: this.account, root: all.trust.root, cert: mine };
    await saveEnrollment(this.enrollment);
    return true;
  }

  /** #764: this device's signature over the root it pinned, for the next
   * browser that derives its keys from the same passkey. Without it that
   * browser asks to be approved, so a failure here costs only that. */
  private async savePin() {
    const e = this.enrollment;
    if (!e) return;
    const pin = await signText(this.keys, devicePinBody({ account: e.account, root: e.root }, this.keys.id));
    await api(`/api/devices/${this.keys.id}/pin`, { pin }).catch((err) => console.warn("arugula: couldn't save this device's pin:", err));
  }

  /** #764: while waiting, sign in with a passkey instead. If it gives PRF
   * output this browser takes its keys: the device another browser with
   * that passkey enrolled, or one approved once for all of them. */
  async usePasskey() {
    const old = this.keys.id;
    if (!(await passkeySignIn())) throw new Error("That passkey can't stand in for an approval. Approve this browser instead.");
    // The request with this browser's old keys goes from the list.
    await api(`/api/devices/${old}/reject`, {}).catch(() => {});
    location.reload();
  }

  /** #761: whether to pin the root control names, by the approving
   * device's MAC under this browser's secret. A MAC that checks out: yes.
   * A wrong one (control's own account, or the code mistyped): no, and
   * the secret goes, so there's one guess per code; the `unvouched` screen
   * says to enroll again. None (approved from a page from before codes):
   * the person checks the account's fingerprint on that screen, the old
   * way, and says so with `vouchFor`. */
  private async vouched(secret: string, mine: Cert, all: { trust: { account: string; root: string }; certs: Cert[] }): Promise<boolean> {
    const got = await api<{ cert: Cert; mac?: string }>(`/api/devices/${mine.device}`);
    if (got.mac && (await deviceMacOk(secret, all.trust, mine, got.mac))) return true;
    const by = all.certs.find((c) => c.device === got.cert.approver)?.name ?? "";
    this.vouch = { why: got.mac ? "wrong" : "missing", root: all.trust.root, by };
    if (got.mac) {
      dropDeviceSecret();
      this.set("unvouched");
      return false;
    }
    this.set("unvouched");
    await new Promise<void>((res) => (this.vouchOk = res));
    this.vouchOk = null;
    this.vouch = null;
    this.set("loading");
    return true;
  }

  /** #761: the person checked the account's fingerprint on the approving
   * device, and it's the one this browser was given. */
  vouchFor() {
    this.vouchOk?.();
  }

  /** Two recovery codes, signed by this device (the first, or one making
   * new ones). */
  private async makeRecoveryCodes(revocations: Revocation[] = []) {
    const codes: string[] = [];
    const certs: Cert[] = [];
    for (let i = 1; i <= 2; i++) {
      const kp = await newKeyPair("Ed25519", true, ["sign", "verify"]);
      const seed = new Uint8Array(await subtle.exportKey("pkcs8", kp.privateKey)).slice(16);
      const sign = new Uint8Array(await subtle.exportKey("raw", kp.publicKey));
      const c: Cert = {
        v: 1,
        account: this.account,
        device: await deviceId(unhex(NO_NOISE), sign),
        kind: "recovery",
        name: `recovery code ${i}`,
        noise: NO_NOISE,
        sign: hex(sign),
        created: Date.now(),
        approver: this.keys.id,
        sig: "",
      };
      c.sig = await signText(this.keys, certBody(c));
      certs.push(c);
      codes.push(toCode(seed));
    }
    await api("/api/recovery", { certs, revocations });
    this.recoveryCodes = codes;
  }

  /** This browser's request, as it asks to join. */
  private request: Cert | null = null;

  /** #758: the app's window's certificate, as the browser that allowed
   * its sign-in signed it, until it enrolls with it. */
  private handed: AppHandOff | null = null;

  /** #326: the machine's join code this browser waited with, which the
   * device that approved it may have approved alongside. */
  brought: string | null = null;

  /** After a turn-down: ask again. */
  tryAgain() {
    this.askAgain?.();
  }

  /** Recovery codes still good. */
  get recoveryLeft(): number {
    return [...this.trusted.values()].filter((c) => c.kind === "recovery").length;
  }

  /** New recovery codes, signed by this device; the old ones are revoked
   * in the same request, so they stop working (#106). */
  async newRecoveryCodes() {
    const old = [...this.trusted.values()].filter((c) => c.kind === "recovery");
    const revocations: Revocation[] = [];
    for (const c of old) {
      const r: Revocation = { v: 1, account: this.account, device: c.device, at: Date.now(), by: this.keys.id, sig: "" };
      r.sig = await signText(this.keys, revocationBody(r));
      revocations.push(r);
    }
    await this.makeRecoveryCodes(revocations);
    await this.refresh();
  }

  /** A passkey for the account signed in. */
  async addPasskey() {
    await passkeyRegister();
    await this.refreshMe();
  }

  /** How many passkeys the account has now. */
  async refreshMe() {
    this.passkeys = (await api<{ passkeys: number }>("/api/me")).passkeys;
    this.emit();
  }

  savedRecoveryCodes() {
    this.recoveryCodes = null;
    this.emit();
  }

  /** While waiting for approval: approve this browser with a recovery
   * code instead, then retire the code. */
  async useRecoveryCode(code: string) {
    const seed = fromCode(code);
    if (!seed) throw new Error("a recovery code is 52 letters and digits");
    const key = await recoveryKey(seed);
    const devs = await api<{ trust: { account: string; root: string } | null; certs: Cert[]; revocations: Revocation[] }>("/api/devices");
    // Only codes still good (a used one was revoked).
    const live = devs.trust ? [...(await evaluate(devs.trust, devs.certs, devs.revocations)).values()] : [];
    const probe = new TextEncoder().encode("arugula recovery probe");
    const sig = new Uint8Array(await subtle.sign("Ed25519", key, probe));
    let mine: Cert | undefined;
    for (const c of live.filter((c) => c.kind === "recovery")) {
      const pub = await subtle.importKey("raw", unhex(c.sign), { name: "Ed25519" }, false, ["verify"]);
      if (await subtle.verify("Ed25519", pub, sig, probe)) mine = c;
    }
    if (!mine) throw new Error("that isn't one of this account's recovery codes (or it was used)");
    // Turned down: ask again first, so there's a request to approve.
    if (this.phase === "turned-down" && this.request) await api("/api/devices", { cert: this.request });
    const k = this.keys;
    const cert: Cert = {
      v: 1,
      account: this.account,
      device: k.id,
      kind: "browser",
      // As it asked (#764: a passkey's device is named for the passkey).
      name: this.request?.name ?? this.fromPasskey ?? deviceName(),
      noise: k.noisePub,
      sign: k.signPub,
      created: Date.now(),
      approver: mine.device,
      sig: "",
    };
    cert.sig = hex(new Uint8Array(await subtle.sign("Ed25519", key, new TextEncoder().encode(certBody(cert)))));
    await api(`/api/devices/${k.id}/approve`, { cert });
    this.usedRecovery = mine.device;
    this.tryAgain();
  }

  /** The recovery code that let this browser in, to retire once enrolled. */
  private usedRecovery: string | null = null;

  /** Devices and the directory, checked against the pinned root. */
  async refresh() {
    const e = this.enrollment;
    if (!e) return;
    try {
      const [devs, dir] = await Promise.all([
        api<{
          trust: { account: string; root: string } | null;
          certs: Cert[];
          revocations: Revocation[];
          pending: Cert[];
          joins?: Record<string, string>;
          features?: Record<string, string>;
          used?: Record<string, number>;
          passkeys?: string[];
        }>("/api/devices"),
        api<{ daemons: ForeignEntry[]; offers?: ShareOffer[] }>("/api/directory"),
      ]);
      this.rootMismatch = !!devs.trust && devs.trust.root !== e.root;
      this.trusted = await evaluate({ account: e.account, root: e.root }, devs.certs, devs.revocations);
      this.revocations = devs.revocations;
      // #327: this browser's own place, checked like any other device's.
      // (A different root is said elsewhere, and isn't fixed by enrolling
      // again under control's.)
      const mine = this.trusted.has(this.keys.id) || this.rootMismatch;
      if (!mine) this.untrustedWhy = devs.revocations.some((r) => r.device === this.keys.id) ? "removed" : "missing";
      if (!mine && (this.phase === "ready" || this.phase === "loading")) this.phase = "untrusted";
      else if (mine && this.phase === "untrusted") this.phase = "ready";
      this.pending = devs.pending;
      this.pendingJoins = new Map(Object.entries(devs.joins ?? {}));
      this.pendingCodes = new Map(await Promise.all(devs.pending.map(async (c) => [c.device, await joinCode(c)] as const)));
      this.pendingSealed = new Set(
        Object.entries(devs.features ?? {})
          .filter(([, f]) => f.split(",").includes(DEVICE_SECRET))
          .map(([d]) => d),
      );
      this.used = new Map(Object.entries(devs.used ?? {}));
      this.passkeyDevices = new Set(devs.passkeys ?? []);
      const daemons: DirDaemon[] = [];
      for (const d of dir.daemons) {
        const { chain, ...entry } = d;
        let cert = this.trusted.get(d.id);
        // Someone else's machine: by their account's certificates, from
        // the root this browser pinned for them on first sight.
        if (!cert && d.account && chain?.trust && pin(d.account, chain.trust.root)) {
          cert = (await evaluate(chain.trust, chain.certs, chain.revocations)).get(d.id);
        }
        if (cert?.kind === "daemon") daemons.push({ ...entry, cert });
      }
      this.daemons = daemons;
      this.offers = dir.offers ?? [];
      await this.loadTeams();
      await this.loadSandboxes();
      await this.loadBilling();
      this.stale = false;
      try {
        localStorage.setItem(DIR_KEY, JSON.stringify(this.daemons));
      } catch {
        // not remembered
      }
      // The service worker answers notifications over the same channels
      // (M29), from what this page checked.
      const ws = this.info.url.replace(/^http/, "ws");
      void saveWorkerDirectory(
        this.daemons.map((d) => ({ id: d.id, noise: d.cert.noise, direct: d.urls, relay: `${ws}/api/relay/c/${d.id}` })),
      ).catch(() => {});
    } catch (err) {
      if (err instanceof HttpError && err.status === 401) {
        window.clearInterval(this.timer);
        return this.set("signed-out");
      }
      this.stale = true;
      try {
        // Daemons this browser already checked stay reachable directly.
        this.daemons = (JSON.parse(localStorage.getItem(DIR_KEY) ?? "[]") as DirDaemon[]).map((d) => ({ ...d, online: false }));
      } catch {
        // nothing saved
      }
    }
    this.emit();
  }

  // ---- billing (M22)

  billing: {
    billing: boolean;
    plan: string;
    relay: { bytes: number; allowance: number; warning: boolean; slowed: boolean };
    sandbox_minutes: number;
    teams: { team: string; name: string; owner: boolean; seats: number; plan: string; sandbox_minutes: number }[];
  } | null = null;

  async loadBilling() {
    this.billing = await api<NonNullable<ControlSession["billing"]>>("/api/billing").catch(() => null);
  }

  /** Off to Stripe Checkout to upgrade (a team, or this account). */
  async upgrade(team?: string) {
    const r = await api<{ url: string }>("/api/billing/checkout", { team });
    location.href = r.url;
  }

  // ---- hosted sandboxes (M20)

  sandboxes: { id: string; state: string; device: string; join: { code: string; cert: Cert } | null }[] = [];
  /** Hosted sandboxes are open to this account. */
  sandboxesOpen = false;
  /** One this browser asked for, to show when it's up. */
  starting: string | null = null;

  async loadSandboxes() {
    const r = await api<{ sandboxes: ControlSession["sandboxes"]; open: boolean }>("/api/sandboxes").catch(() => null);
    if (!r) return;
    this.sandboxes = r.sandboxes;
    this.sandboxesOpen = r.open;
    // The device that asked approves its sandbox's daemon by itself: the
    // code is recomputed from the key, as for any join.
    for (const s of r.sandboxes) {
      if (!s.join || s.device !== this.keys.id) continue;
      if ((await joinCode(s.join.cert)) !== s.join.code) continue;
      const signed: Cert = { ...s.join.cert, account: this.account, approver: this.keys.id, sig: "" };
      signed.sig = await signText(this.keys, certBody(signed));
      await api(`/api/joins/${s.join.code}/approve`, { cert: signed }).catch(() => {});
    }
  }

  /** A hosted VM (M20); resolves with its id once asked for. */
  async startSandbox(): Promise<string> {
    const r = await api<{ id: string }>("/api/sandboxes", { device: this.keys.id });
    this.starting = r.id;
    this.emit();
    // Poll quickly while it starts: approve its join, then wait for its
    // daemon in the directory.
    void (async () => {
      for (let i = 0; i < 180 && this.starting === r.id; i++) {
        await new Promise((res) => setTimeout(res, 1000));
        await this.refresh();
        if (this.daemons.some((d) => d.sandbox === r.id)) break;
        const s = this.sandboxes.find((x) => x.id === r.id);
        if (s?.state.startsWith("failed")) break;
      }
      this.emit();
    })();
    return r.id;
  }

  async deleteSandbox(id: string) {
    await fetch(`/api/sandboxes/${id}`, { method: "DELETE" });
    await this.refresh();
  }

  // ---- teams and people (M19)

  teams: Team[] = [];
  /** Teams this account asked to join, waiting on an owner (#103). */
  asked: { team: string; name: string; owners: string[] }[] = [];
  /** A team that took this account in while this page watched, to say so. */
  joined: { team: string; name: string } | null = null;
  /** What control kept to tell this account once (#206: a team it was in
   * was deleted), oldest first. */
  notices: { id: number; title: string; body: string }[] = [];

  /** Whether the last load of the teams worked: an empty list is then
   * this account's, not a failure. */
  teamsLoaded = false;

  async loadTeams() {
    let loaded = true;
    const r = await api<{ teams: Omit<Team, "verified">[]; asked?: ControlSession["asked"]; notices?: ControlSession["notices"] }>(
      "/api/teams",
    ).catch(() => {
      loaded = false;
      return { teams: [] as Omit<Team, "verified">[], asked: this.asked, notices: this.notices };
    });
    this.teamsLoaded = loaded;
    this.notices = r.notices ?? [];
    const out: Team[] = [];
    for (const t of r.teams) {
      // The founder pinned on first sight. The team's daemons check each
      // roster version against it; this browser checks the ones it signs.
      const ok = pin(`team:${t.team}`, `${t.pin.founder}.${t.pin.founder_root}`);
      // #762: only someone who held an owner's link; anything else control
      // says waits is refused here, never shown.
      const requests: Team["requests"] = [];
      for (const q of t.requests ?? []) {
        const by = await askOk(t.roster, t.certs, q);
        if (by) requests.push({ ...q, role: q.invite.role, by });
      }
      out.push({ ...t, requests, verified: ok });
    }
    const now = r.asked ?? [];
    const yes = this.asked.find((a) => !now.some((x) => x.team === a.team) && out.some((t) => t.team === a.team));
    if (yes) this.joined = { team: yes.team, name: yes.name };
    this.asked = now;
    this.teams = out;
  }

  /** The teams this browser pinned and checked, for a machine of this
   * account's to check their rosters by (#233): `<founder>.<founder's
   * root>` by team. */
  teamPins(): Record<string, string> {
    const out: Record<string, string> = {};
    for (const t of this.teams) if (t.verified && t.role !== null) out[t.team] = `${t.pin.founder}.${t.pin.founder_root}`;
    return out;
  }

  /** A machine of this account's own: not a team's, not someone else's. */
  owns(id: string): boolean {
    const d = this.daemons.find((x) => x.id === id);
    return !!d && (!d.account || d.account === this.account) && !d.team;
  }

  sawJoined() {
    this.joined = null;
    this.emit();
  }

  /** Seen: control forgets it. */
  async sawNotice(id: number) {
    this.notices = this.notices.filter((n) => n.id !== id);
    this.emit();
    await api(`/api/me/notices/${id}/seen`, {}).catch(() => {});
  }

  private myMember(): { account: string; root: string; name: string } {
    // Never "you": teammates see this.
    return { account: this.account, root: this.enrollment!.root, name: word(this.name || `account-${this.account.slice(0, 6)}`) };
  }

  /** Change what other people see (#102). */
  async setName(name: string) {
    const r = await api<{ name: string }>("/api/me/name", { name });
    this.name = r.name;
    this.login = r.name;
    this.emit();
  }

  /** M104: whether the account lets this page send usage counts (`on`:
   * null until asked), and whether this control asks at all. Null until
   * known, and from a control without it. */
  telemetry: PageConsent | null = null;
  /** The card was put away unanswered on this load: asked again next. */
  private telemetryLater = false;

  telemetryNotNow() {
    this.telemetryLater = true;
    this.emit();
  }

  async loadTelemetry() {
    this.telemetry = await api<PageConsent>("/api/me/telemetry").catch(() => null);
    this.countIfAllowed();
    this.emit();
  }

  async setTelemetry(on: boolean) {
    this.telemetry = await api<PageConsent>("/api/me/telemetry", { on });
    this.countIfAllowed();
    this.emit();
  }

  /** Whether to show the card: asked by a control that forwards them, not
   * answered, not put away on this load, and not under a test runner. */
  askTelemetry(): boolean {
    const t = this.telemetry;
    return !!t && t.ask && t.on === null && !this.telemetryLater && !automated();
  }

  private countIfAllowed() {
    if (this.telemetry?.on) {
      if (!counting()) startCounting(sendPageCounts);
    } else stopCounting();
  }

  async createTeam(name: string) {
    const team = Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => b.toString(16).padStart(2, "0")).join("");
    const roster = await signRoster(
      { v: 1, team, name: name.trim().slice(0, 80) || "team", version: 1, at: Date.now(), members: [{ ...this.myMember(), role: "owner" }] },
      this.keys,
    );
    await api("/api/teams", { roster });
    await this.refresh();
  }

  /** Sign and send the team's next roster, as `change` makes it. */
  async changeTeam(team: string, change: (members: Roster["members"]) => Roster["members"], name?: string) {
    const t = this.teams.find((x) => x.team === team);
    if (!t) throw new Error("no such team");
    const prev = t.roster;
    // Used presigned invites stay listed until they'd have expired anyway,
    // so none works twice.
    const now = Date.now();
    const spent = (prev.spent ?? []).filter((x) => x.expires > now);
    const next = await signRoster(
      {
        v: spent.length ? 2 : 1,
        team,
        name: name ?? prev.name,
        version: prev.version + 1,
        at: now,
        members: change(prev.members.map((m) => ({ ...m }))),
        ...(spent.length ? { spent } : {}),
      },
      this.keys,
    );
    if (!(await follows(next, prev, t.pin, t.certs))) throw new Error("that change doesn't check out (are you an owner here?)");
    await api(`/api/teams/${team}/roster`, { roster: next });
    await this.refresh();
  }

  async admit(team: string, req: Team["requests"][number]) {
    // The role and root the link's proof vouches for, not control's word.
    await this.changeTeam(team, (ms) => [...ms.filter((m) => m.account !== req.account), { account: req.account, root: req.root, role: req.invite.role, name: word(req.name) }]);
  }

  /** An invite link. Presigned unless `askFirst` (or for an owner): this
   * device signs it now, and whoever opens it is in as soon as they accept.
   * Either kind's one-time key seed goes only into the link's fragment. */
  async invite(team: string, role: TeamRole, askFirst = false): Promise<string> {
    return (await this.makeInvite(team, role, askFirst)).link;
  }

  /** As `invite`, saying whether the link asks an owner first, and why
   * when it does though it wasn't asked to: control makes a presigned one
   * only once every machine checking the team's rosters understands it. */
  async makeInvite(team: string, role: TeamRole, askFirst = false): Promise<{ link: string; asks: boolean; why?: string }> {
    // #762: an Ask me first link carries a one-time key too, so a request
    // to join proves it came from the link.
    const old = async (why?: string) => {
      const { seed, key } = await newInviteKey();
      const made = Date.now();
      const ask = await signAskInvite({ team, role, made, expires: made + ASK_TTL_MS, key }, this.keys);
      await api(`/api/teams/${team}/invites`, { role, ask });
      return { link: `${location.origin}/#askinvite=${team}.${seed}`, asks: true, why };
    };
    if (askFirst || role === "owner") return old();
    const { seed, key } = await newInviteKey();
    const presigned = await signInvite({ team, role, expires: Date.now() + PRESIGNED_TTL_MS, key }, this.keys);
    try {
      await api(`/api/teams/${team}/invites`, { role, presigned });
    } catch (e) {
      if (e instanceof HttpError && e.status === 409) return old(e.message);
      throw e;
    }
    return { link: `${location.origin}/#pinvite=${team}.${seed}`, asks: false };
  }

  /** What a presigned invite link says, as control has it. */
  async showPresigned(team: string, seed: string) {
    const { key } = await inviteKey(seed);
    return api<{ invite: Invite; name: string; pin: TeamPin; roster: Roster; certs: AccountCerts }>(`/api/presigned/${team}/${key}`);
  }

  /** Join a team with a presigned invite: write its next version, adding
   * this account, signed here and by the link's one-time key. */
  async redeem(team: string, seed: string) {
    for (let attempt = 0; ; attempt++) {
      const p = await this.showPresigned(team, seed);
      if (!pin(`team:${team}`, `${p.pin.founder}.${p.pin.founder_root}`)) throw new Error("this team isn't the one this browser saw before");
      const next = await redeemInvite(p.roster, p.invite, seed, this.myMember());
      const mine = await api<{ certs: Cert[]; revocations: Revocation[] }>("/api/devices");
      const certs: AccountCerts = { ...p.certs, [this.account]: [mine.certs, mine.revocations] };
      if (!(await follows(next, p.roster, p.pin, certs))) throw new Error("that invite doesn't check out");
      try {
        await api(`/api/teams/${team}/roster`, { roster: next });
        break;
      } catch (e) {
        // Someone else joined first: build on their version.
        if (!(e instanceof HttpError && e.status === 409) || attempt >= 2) throw e;
      }
    }
    await this.refresh();
  }

  /** What an Ask me first link says, as control has it. */
  async showInvite(team: string, seed: string) {
    const { key } = await inviteKey(seed);
    return api<{ team: string; name: string; role: TeamRole }>(`/api/invites/${team}/${key}`);
  }

  /** Ask to join with an Ask me first link: its one-time key signs for
   * this account and its root, which owners' pages check (#762). */
  async acceptInvite(team: string, seed: string) {
    const k = await inviteKey(seed);
    const proof = await k.sign(askBody({ team, key: k.key }, this.account, this.enrollment!.root));
    await api(`/api/invites/${team}/${k.key}/accept`, { proof });
    await this.refresh();
  }

  async rejectRequest(team: string, account: string) {
    await api(`/api/teams/${team}/requests/${account}/reject`, {});
    await this.refresh();
  }

  /** A team's one-click links nobody has used yet (owners only, #134). */
  async presignedInvites(team: string) {
    return (await api<{ invites: PresignedInvite[] }>(`/api/teams/${team}/presigned`)).invites;
  }

  /** Cancel one of them: control refuses it from now on. */
  async cancelPresigned(team: string, key: string) {
    const res = await fetch(`/api/teams/${team}/presigned/${key}`, { method: "DELETE" });
    if (!res.ok && res.status !== 404) {
      const j = (await res.json().catch(() => ({}))) as { error?: string };
      throw new HttpError(res.status, j.error ?? `HTTP ${res.status}`, j);
    }
  }

  async lockTeam(team: string, locked: boolean) {
    await api(`/api/teams/${team}/lock`, { locked });
    await this.refresh();
  }

  /** Labs flags this account set for its machines, and each of its teams'
   * for the team's machines (#347). A flag not named is unset. */
  async labsFlags() {
    return api<LabsFlags>("/api/flags");
  }

  /** Set one of the account's flags, or (an owner) a team's; `null` unsets it. */
  async setLabsFlag(name: string, on: boolean | null, team?: string) {
    return api<{ flags: Record<string, boolean> }>("/api/flags", { name, on, team });
  }

  /** Someone on control, by their login or name: whom to share with, and the root
   * device to pin for them (compare its fingerprint with them). */
  async person(login: string) {
    return api<{ account: string; name: string; root: string }>(`/api/people?login=${encodeURIComponent(login)}`);
  }

  /** Hand control a push subscription, signed by this device (M21). */
  async subscribePush(sub: { endpoint: string; p256dh: string; auth: string }) {
    const s = { v: 1, account: this.account, device: this.keys.id, endpoint: sub.endpoint, p256dh: sub.p256dh, auth: sub.auth, at: Date.now(), sig: "" };
    // Frozen (#504): signed.
    const body = `illogical push v1\naccount ${s.account}\ndevice ${s.device}\nendpoint ${s.endpoint}\np256dh ${s.p256dh}\nauth ${s.auth}\nat ${s.at}\n`;
    s.sig = await signText(this.keys, body);
    await api("/api/push/subscribe", { sub: s });
  }

  async unsubscribePush(endpoint: string) {
    await api("/api/push/unsubscribe", { endpoint });
  }

  /** How a Client reaches daemon `id`. */
  target(id: string): E2ETarget | undefined {
    const d = this.daemons.find((x) => x.id === id);
    if (!d) return undefined;
    const ws = this.info.url.replace(/^http/, "ws");
    // Hosted sandboxes go through their provider, a socket each; the rest
    // share the page's one socket to the relay (M25).
    const mux = d.sandbox ? undefined : `${ws}/api/relay/m`;
    // M50: blocks on control's block origins, where control serves them.
    // A hosted sandbox's daemon doesn't dial control's relay, so not there.
    const blockDomain = d.sandbox ? undefined : this.info.block_domain;
    return { daemon: { id: d.id, noise: d.cert.noise }, direct: d.urls, relay: `${ws}/api/relay/c/${d.id}`, mux, keys: this.keys, blockDomain };
  }

  /** A read-only link to a session on daemon `id` (M19): a one-off key, its
   * private half only in the link's fragment. */
  async makeLink(request: (m: string, p: string, b?: unknown) => Promise<{ ok: boolean; json<T>(): Promise<T> }>, id: string, session: number, ttlSecs: number, history: boolean): Promise<string> {
    const d = this.daemons.find((x) => x.id === id);
    if (!d) throw new Error("no such machine");
    const kp = await newKeyPair("X25519", true, ["deriveBits"]);
    const seed = new Uint8Array(await crypto.subtle.exportKey("pkcs8", kp.privateKey)).slice(16);
    const pub = new Uint8Array(await crypto.subtle.exportKey("raw", kp.publicKey));
    const res = await request("POST", "/api/links", { session, key: hex(pub), ttl_secs: ttlSecs, history });
    if (!res.ok) throw new Error((await res.json<{ error?: string }>().catch(() => null))?.error ?? "couldn't make a link");
    return `${this.info.url}/#link=${d.id}.${d.cert.noise}.${hex(seed)}.${hex(pub)}`;
  }

  /** Approve another device's request: sign its certificate. With the
   * code it shows (#761, scanned or typed), the lookup must be its keys'
   * code, and the approval carries a MAC under the secret over this
   * account, the root this device pinned and its keys, which it checks
   * before it pins that root. */
  async approve(c: Cert, code: DeviceCode | null = null) {
    await this.approveDevice(c, code);
    await this.refresh();
  }

  /** Whether a waiting device shows a code (#761): approved from it, not
   * by comparing its fingerprint. */
  sealed(c: Cert): boolean {
    return this.pendingSealed.has(c.device);
  }

  private async approveDevice(c: Cert, code: DeviceCode | null = null) {
    if (this.sealed(c) && !code) throw new Error(`${c.name} shows a code: scan it or type it to approve it`);
    const signed: Cert = { ...c, account: this.account, approver: this.keys.id, sig: "" };
    signed.sig = await signText(this.keys, certBody(signed));
    let mac: string | null = null;
    if (code) {
      // Recomputed from the key control shows, so control can't swap it.
      if ((await joinCode(c)) !== code.code) throw new Error(`that isn't the code ${c.name} shows: not approving it`);
      if (!this.enrollment || this.rootMismatch) throw new Error("this browser's account root isn't settled: approve from a device you've used before");
      mac = await deviceMac(code.secret, { account: this.account, root: this.enrollment.root }, c);
    }
    await this.approving(api(`/api/devices/${c.device}/approve`, { cert: signed, ...(mac ? { mac } : {}) }));
  }

  /** An approval's request: refused, it says which check failed and what
   * to do (#327); one that says this browser isn't trusted shows that
   * screen. */
  private async approving(req: Promise<unknown>) {
    try {
      await req;
    } catch (e) {
      const reason = e instanceof HttpError && typeof e.body.reason === "string" ? e.body.reason : "";
      if (!reason) throw e;
      // Checked here too: if this browser's own reckoning agrees, refresh
      // switches to the screen that says so.
      if (SELF_REFUSED.has(reason)) await this.refresh();
      throw new RefusedError(reason, REFUSED[reason] ?? (e as Error).message);
    }
  }

  async reject(c: Cert) {
    await api(`/api/devices/${c.device}/reject`, { by: this.keys.id });
    await this.refresh();
  }

  /** A daemon's join request, by the code it printed. The code is
   * recomputed from the key control shows, so control can't swap it.
   * `team` is the one it asked for (`--team`), if any. */
  async showJoin(code: string): Promise<JoinRequest> {
    const c = normalizeCode(code);
    if (!c) throw new Error("a code is ten letters and digits");
    const j = await api<JoinRequest>(`/api/joins/${c}`);
    if ((await joinCode(j.cert)) !== c) throw new Error("that request doesn't match its code: not approving it");
    return j;
  }

  /** Teams I can put my machines in (#332): any I'm in, but only those I
   * own while they're locked. */
  addableTeams(): Team[] {
    return this.teams.filter((t) => t.verified && (t.role === "owner" || !t.locked));
  }

  /** Approve a daemon into this account, or into `team` (one I'm in): this
   * device signs the team in, so control can't pick one (#100). `devices`
   * waiting with its code (#326) are approved with it, before the page
   * hears of either, so its prompt stays up until both are done. With the
   * machine's join `secret` (#760), the approval carries a MAC under it
   * over this account and the root this device pinned (not control's word
   * for it), which the machine checks before it pins that root. A device
   * alongside that shows a code (#761) is approved with it, from
   * `deviceCodes` by its id. */
  async approveJoin(code: string, c: Cert, team: string | null = null, devices: Cert[] = [], secret: string | null = null, deviceCodes: Map<string, DeviceCode> = new Map()) {
    const signed: Cert = { ...c, account: this.account, approver: this.keys.id, sig: "" };
    signed.sig = await signText(this.keys, certBody(signed));
    let mac: string | null = null;
    if (secret) {
      if (!this.enrollment || this.rootMismatch) throw new Error("this browser's account root isn't settled: approve from a device you've used before");
      mac = await joinMac(secret, { account: this.account, root: this.enrollment.root }, c);
    }
    let teamSig: string | null = null;
    if (team) {
      const t = this.addableTeams().find((x) => x.team === team);
      if (!t) throw new Error("only the team's members add machines to it");
      teamSig = await signText(this.keys, teamJoinBody(c.device, t.pin));
    }
    await this.approving(api(`/api/joins/${code}/approve`, { cert: signed, team, team_sig: teamSig, ...(mac ? { mac } : {}) }));
    try {
      for (const d of devices) {
        try {
          await this.approveDevice(d, deviceCodes.get(d.device) ?? null);
        } catch (e) {
          throw new AlongsideError(d.name, (e as Error).message);
        }
      }
    } finally {
      await this.refresh();
    }
  }

  /** Move a machine of this account into `team` (one I'm in), or back to
   * the account (null). This device signs it for the daemon to check. A
   * team's owner takes someone else's machine out of it the same way. */
  async moveDaemon(daemon: string, team: string | null) {
    let pin: TeamPin | null = null;
    if (team) {
      const t = this.addableTeams().find((x) => x.team === team);
      if (!t) throw new Error("only the team's members add machines to it");
      pin = t.pin;
    }
    const at = Date.now();
    const sig = await signText(this.keys, moveBody(daemon, pin, at));
    await api(`/api/daemons/${daemon}/team`, { team: pin, at, by: this.keys.id, sig });
    await this.refresh();
  }

  /** Turn a daemon's join down: it stops waiting. */
  /** M48: a desktop app asking to sign in as this account (`#app=`):
   * where it asked from, and whether that's this browser's network. An
   * app from #758 on sends its device keys; one from before has a code. */
  async showAppLogin(id: string): Promise<AppLogin> {
    return api(`/api/app-login/${encodeURIComponent(id)}`);
  }

  /** Whether allowing `a` also makes the app one of the account's devices
   * (#758): it sent keys, and this browser is a device that can sign. */
  signsApp(a: AppLogin): boolean {
    return !!a.device && !!this.enrollment && this.phase === "ready" && !this.rootMismatch;
  }

  /** Allow it: control answers with the app's loopback address, which this
   * browser hands the grant to (only the app on this computer hears it).
   * #758: with a certificate for the app's keys, signed here, and the root
   * this browser pinned. They go only to the app: if control showed keys
   * of its own, the app finds the certificate isn't for its keys. */
  async allowAppLogin(id: string, a: AppLogin): Promise<string> {
    const r = await api<{ redirect: string }>(`/api/app-login/${encodeURIComponent(id)}/allow`, {});
    const url = new URL(r.redirect);
    if (url.protocol !== "http:" || url.hostname !== "127.0.0.1" || url.pathname !== "/arugula-signin")
      throw new Error("control sent the sign-in somewhere other than the app on this computer: not following it");
    if (a.device && this.signsApp(a)) {
      try {
        const cert: Cert = {
          v: 1,
          account: this.account,
          device: await deviceId(unhex(a.device.noise), unhex(a.device.sign)),
          kind: "browser",
          name: a.name.slice(0, 64),
          noise: a.device.noise,
          sign: a.device.sign,
          created: Date.now(),
          approver: this.keys.id,
          sig: "",
        };
        cert.sig = await signText(this.keys, certBody(cert));
        const handOff: AppEnrollment = { cert, root: this.enrollment!.root };
        url.searchParams.set("cert", b64url(JSON.stringify(handOff)));
        try {
          sessionStorage.setItem(APP_SIGNED_KEY, "1");
        } catch {
          // The page after says less.
        }
      } catch (e) {
        // Signed in all the same; the app asks to be approved instead.
        console.warn("arugula: couldn't sign the app's certificate:", e);
      }
    }
    return url.href;
  }

  /** Answer a share someone offered: accepted, it's listed and reachable. */
  async answerShare(daemon: string, accept: boolean) {
    await api(`/api/shares/${encodeURIComponent(daemon)}`, { accept });
    this.offers = this.offers.filter((o) => o.daemon !== daemon);
    this.emit();
    if (accept) {
      // The machine fetches this account's certificates when control
      // nudges it; the directory lists it once it lets this account in.
      setTimeout(() => void this.refresh(), 1500);
    }
  }

  async rejectJoin(code: string) {
    await api(`/api/joins/${code}/reject`, { device: this.keys.id });
  }

  async revoke(id: string) {
    const r: Revocation = { v: 1, account: this.account, device: id, at: Date.now(), by: this.keys.id, sig: "" };
    r.sig = await signText(this.keys, revocationBody(r));
    await api("/api/revocations", { revocation: r });
    await this.refresh();
  }

  /** After a lost key (#94): forget this browser's place in the account
   * and ask to join again, as a new device. One the account stopped
   * trusting (#327) asks with the recovery form open; a lost key asks as
   * before, the form a button away. */
  async enrollAgain(recover = false) {
    await forget(location.origin);
    if (recover)
      try {
        sessionStorage.setItem(RECOVER_KEY, "1");
      } catch {
        // The form stays a button away.
      }
    location.reload();
  }

  async signOut(forgetDevice: boolean) {
    await api("/auth/logout", {}).catch(() => {});
    if (forgetDevice) await forget(location.origin);
    location.href = "/";
  }
}
