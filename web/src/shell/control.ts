// The page served by Arugula control (M17): who you are, this browser's
// place in your account, and the daemons you can reach.
//
// Signing in is the whole flow (aru-141, #1004): a signed-in browser
// reaches its machines with machine tokens control signs, and never asks
// another device to approve it. A browser that is one of the account's
// devices already (from before aru-141) keeps its key: it pins the
// account's root device and believes only certificates that chain back to
// it, signs what daemons from before still check, and uses tokens to
// reach machines unless `tokenMode` says otherwise. Only a control that
// signs no tokens (a dev or test hub), or `tokenMode` "off", still makes
// the account's first browser its first device, with no screen.

import { browserOrLegacyChained, certBody, certifies, fromApp, evaluate, hex, JOIN_SECRET, joinCode, joinMac, oneClickMac, normalizeCode, splitJoinCode, type Cert, type Kind, type Revocation, revocationBody } from "./e2e/cert.ts";
import {
  forget,
  forgetEnrollment,
  loadEnrollment,
  loadKeys,
  saveEnrollment,
  saveWorkerDirectory,
  signText,
  type DeviceKeys,
  type Enrollment,
  type WorkerDaemon,
} from "./e2e/keys.ts";
import { newKeyPair } from "./e2e/noise.ts";
import { controlBlockGrants, controlIssues, controlTokens, gatedTokens, tokenEntry, tokenMode, type BlockGrants, type MachineTokens } from "./e2e/machine-token.ts";
import { acceptHostedHolding, signDelegation, signRevoke, type Delegation, type DelegationRevoke, type HostedProof } from "./e2e/hosted.ts";
import { directUrls } from "../shared/hosted.ts";
import { expiresIn, grantBody, grantRevokeBody, newGrant, newGrantRevoke, passkeyChallenge, type GrantRevoke, type PasskeyAssertion, type ProjectGrant } from "./e2e/run-grant.ts";
import type { E2ETarget } from "./client";
import { desktopApp } from "../shared/desktop";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { automated, counting, startCounting, stopCounting, type PageCounts } from "../shared/telemetry";
import type { PushPrefs } from "./push";
import type { PushStanding } from "./push-health";
import {
  follows,
  inviteKey,
  moveBody,
  redeemInvite,
  askBody,
  askOk,
  changesOwners,
  ownerChallenge,
  ownersAsHeld,
  signAskInvite,
  signInvite,
  signLock,
  signRoster,
  teamJoinBody,
  unsigned,
  NO_ROOT,
  word,
  type AccountCerts,
  type AskInvite,
  type Invite,
  type Roster,
  type TeamLock,
  type TeamPin,
  type TeamRole,
} from "./e2e/team.ts";
import { makeInvite, nextRoster, type MadeInvite, type PresignedInvite } from "../shared/team-roster.ts";
import { movesInto } from "./team-adds.ts";
import { checkRoster, ClosureStore, RosterStore, takeLock, TrustStore, withTimeout, type RootChange, type RosterProblem } from "./team-rosters.ts";

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
  /** Blocks get origins of their own, `https://b-<site_key>.<this>` (M50);
   * absent when control doesn't serve blocks. */
  block_domain?: string;
  /** Where a Doc's prototypes run (#506), `https://proto.<block_domain>`;
   * absent without a block domain. */
  proto?: string;
  /** The platform page's origin (M151), `https://app.arugula.io`: chat and
   * hosted workspaces are there. Null or absent when control has none. */
  platform?: string | null;
}

/** On a URL control is moving away from (#507): where it is now, with the
 * same page there, for a note in the page's layout (ui/moving-note.tsx).
 * This site keeps working for now: its passkeys and approved browsers are
 * its own, so the new one is signed in and approved from here. */
export let movingTo: URL | null = null;

export function showMoving(info: ControlInfo): void {
  if (!info.primary || info.primary === info.url) return;
  movingTo = new URL(location.pathname + location.search + location.hash, info.primary);
}

/** One of the account's devices, for a waiting page to name (#77). */
export interface Approver {
  cert: Cert;
  /** When control last saw it load the page; null if never. */
  used: number | null;
}

export interface Approvers {
  /** Those that may approve what waits. */
  can: Approver[];
  /** The account's other browsers and apps, which may not. */
  cannot: Approver[];
}

export interface DirDaemon {
  id: string;
  name: string;
  urls: string[];
  online: boolean;
  last_seen: number | null;
  /** Its certificate, checked against the account's root; with machine
   * tokens (aru-141), one made of what control's directory says, checked
   * by nothing but control. */
  cert: Cert;
  /** Someone else's (M19): its owner account and their login, and the
   * team it belongs to, if any. */
  account?: string;
  owner_name?: string;
  team?: string | null;
  /** In a team, joined on control's word (Arugula #1182): whether it's
   * the team's (a shared or work machine) rather than its owner's, also
   * used for the team. Absent where that isn't asked (no team, or a
   * machine that pinned a root, or an older control). */
  team_box?: boolean;
  /** M178: a hosted machine, accepted for this project only (labelled
   * "Hosted: the operator can read" wherever it's shown). */
  hosted?: { project: string; workspace: string; role: TeamRole };
}

/** Another account's machine as the directory lists it, with that account's
 * certificates to check it by; or a hosted machine (M178), with its
 * certificate and its project's delegation. */
export interface ForeignEntry extends Omit<DirDaemon, "cert" | "hosted"> {
  chain?: { trust: { account: string; root: string } | null; certs: Cert[]; revocations: Revocation[] };
  hosted?: HostedProof;
  /** #994: its Noise key, hex, for every machine the account reaches: what
   * a channel with a machine token opens to (aru-141). */
  noise?: string;
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
  /** The owner's signed word on whether it's locked, if one checked out
   * (none from before, or on control's word): `locked` is control's word
   * (aru-141). */
  lock?: TeamLock | null;
  roster: Roster;
  role: TeamRole | null;
  /** Who used an invite link and waits for an owner (owners only). Only
   * those whose link's one-time key vouches for them (#762): `by` is the
   * owner whose device made the link, or "" for a link control made
   * (aru-141). */
  requests: { account: string; root: string; name: string; role: TeamRole; created: number; invite: AskInvite; proof: string; by: string }[];
  certs: AccountCerts;
  /** Members' names as they set them, by account (#208): the roster's
   * one-word form ("Sam-Stranger") is only what's signed. */
  names?: Record<string, string>;
  /** The founder is the one this browser pinned on first sight, and the
   * roster follows the last one this browser checked. */
  verified: boolean;
  /** Why control's roster doesn't follow the one this browser last
   * checked (or can't be kept): `roster` is control's all the same
   * (aru-141), and changes to it go on control's word, unsigned. */
  rosterProblem?: RosterProblem;
  /** Members' roots that changed since this browser last looked, until
   * dismissed. */
  rootChanges?: RootChange[];
  /** Versions of its roster that a device removed since had signed, while
   * it still counted: taken, and shown. */
  rosterLate?: number[];
}

/** A one-click invite link control still holds (#134). */
export type { PresignedInvite };

const PINS_KEY = "arugula.control.pins";

/** The last roster this browser checked for each team (`team-rosters.ts`),
 * and members' roots that changed since it looked, until dismissed. */
const ROSTERS_KEY = "arugula.control.rosters";
/** Each account's certificates and removals as this page last held them,
 * and when it first saw each (`TrustStore`). */
const HELD_CERTS_KEY = "arugula.control.held-certs";
const CLOSURES_KEY = "arugula.control.delegations-ended";
const ROOT_CHANGES_KEY = "arugula.control.root-changes";

function readKey<T>(key: string, empty: T): T {
  try {
    return (JSON.parse(localStorage.getItem(key) ?? "null") as T | null) ?? empty;
  } catch {
    return empty;
  }
}

function writeKey(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    // not remembered
  }
}

const rootChangesKept = () => readKey<Record<string, RootChange[]>>(ROOT_CHANGES_KEY, {});
/** Versions taken that a device removed since had signed, by team, until
 * dismissed with the root changes. */
const ROSTER_LATE_KEY = "arugula.control.roster-late";
const rosterLateKept = () => readKey<Record<string, number[]>>(ROSTER_LATE_KEY, {});

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

/** `GET /api/flags` on control (#347). */
export type LabsFlags = {
  account: Record<string, boolean>;
  teams: Record<string, { flags: Record<string, boolean>; owner: boolean }>;
  /** A machine of the account has Developer settings: only then does the page offer these (strangers never see Labs). */
  dev?: boolean;
};

export async function api<T>(path: string, body?: unknown, send: (path: string, init: RequestInit) => Promise<Response> = fetch): Promise<T> {
  const res = await send(path, {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new HttpError(res.status, refusalOf(res.status, j as { error?: string; reason?: string }, `HTTP ${res.status}`), j as Record<string, unknown>);
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

/** This page's counts to control, under the signed-in account. As the
 * page goes away, a beacon, which the browser sends after it's gone (a
 * `keepalive` fetch if it won't take one). Control saying no (the account
 * turned it off elsewhere) stops the counting. */
async function sendPageCounts(c: PageCounts, leaving: boolean): Promise<boolean> {
  const body = JSON.stringify(c);
  if (leaving && navigator.sendBeacon?.("/api/telemetry/page", new Blob([body], { type: "application/json" }))) return true;
  const res = await fetch("/api/telemetry/page", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body,
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

/** Sign in with a passkey (any registered on this control). It signs in
 * and nothing more: since aru-141 (#1004) no browser takes device keys
 * from a passkey's PRF output (#764). */
export async function passkeySignIn(): Promise<void> {
  const o = await api<{ challenge: string; rpId: string; userVerification: UserVerificationRequirement; timeout: number }>("/auth/passkey/login", {});
  const cred = (await navigator.credentials.get({
    publicKey: { challenge: unb64u(o.challenge), rpId: o.rpId, userVerification: o.userVerification, timeout: o.timeout },
  })) as PublicKeyCredential | null;
  if (!cred) throw new Error("no passkey chosen");
  const r = cred.response as AuthenticatorAssertionResponse;
  await api("/auth/passkey/login/finish", {
    id: b64u(cred.rawId),
    clientDataJSON: b64u(r.clientDataJSON),
    authenticatorData: b64u(r.authenticatorData),
    signature: b64u(r.signature),
    prf: false,
  });
}

/** A passkey made for the account signed in, waiting for one the account
 * has to agree before control keeps it: confirmed with a click of its own
 * ([`passkeyConfirm`]), as a browser asks for a passkey only on one. */
export interface PasskeyToConfirm {
  finish: Record<string, unknown>;
  /** What keeps the new one, as control names it from its AAGUID (null
   * when it doesn't know), and when it was made: the prompt says (#471). */
  provider: string | null;
  made: number;
  options: { challenge: string; rpId: string; userVerification: UserVerificationRequirement; timeout: number; allowCredentials: { type: "public-key"; id: string }[] };
}

/** Where a GitHub sign-in comes back to, to add a passkey (an account with
 * none needs a GitHub sign-in from the last few minutes for that). */
export const ADD_PASSKEY_HASH = "#add-passkey";

/** Make a passkey: for the account signed in, or a new account called
 * `name` (#102: what teammates see). For an account with passkeys, the new
 * one waits for one of them to agree ([`PasskeyToConfirm`]). For one with
 * none and no GitHub sign-in from the last few minutes, the page goes
 * through GitHub's sign-in again first, and comes back to `#add-passkey`. */
/** What one of the account's browsers or phones signs to add a first
 * passkey (`add_passkey_device_body`): the account, the new passkey's id
 * and the registration's challenge. A second takes a passkey alone
 * (DK11). */
export const addPasskeyDeviceBody = (account: string, credential: string, challenge: string) =>
  `arugula add passkey device v1\naccount ${account}\ncredential ${credential}\nchallenge ${challenge}\n`;

/** This page's device, to sign for a first passkey added to its account. */
export interface PasskeySigner {
  account: string;
  id: string;
  sign: (text: string) => Promise<string>;
}

export async function passkeyRegister(name?: string, device?: PasskeySigner): Promise<PasskeyToConfirm | null> {
  type Options = {
    challenge: string;
    rp: PublicKeyCredentialRpEntity;
    user: { id: string; name: string; displayName: string };
    pubKeyCredParams: PublicKeyCredentialParameters[];
    authenticatorSelection: AuthenticatorSelectionCriteria;
    attestation: AttestationConveyancePreference;
    timeout: number;
    step_up?: "passkey" | null;
    device_sig?: boolean;
  };
  let o: Options;
  try {
    // This page's device, so control knows whether it may sign for a first
    // passkey or a GitHub sign-in must vouch for it.
    o = await api<Options>("/auth/passkey/register", { ...(name === undefined ? {} : { name }), ...(device ? { device: device.id } : {}) });
  } catch (e) {
    if (e instanceof HttpError && e.body.reason === "reauth") {
      location.assign(`/auth/github?reauth=true&next=${encodeURIComponent(location.pathname + ADD_PASSKEY_HASH)}`);
      return null;
    }
    throw e;
  }
  // `device_sig`: control asks this page's device to sign for a first
  // passkey only when it may; a second takes one of the account's
  // passkeys and nothing else (DK11).
  const { step_up, device_sig, ...opts } = o;
  const cred = (await navigator.credentials.create({
    publicKey: {
      ...opts,
      challenge: unb64u(o.challenge),
      user: { ...o.user, id: unb64u(o.user.id) },
    },
  })) as PublicKeyCredential | null;
  if (!cred) throw new Error("no passkey made");
  const r = cred.response as AuthenticatorAttestationResponse;
  const id = b64u(cred.rawId);
  const finish = {
    id,
    clientDataJSON: b64u(r.clientDataJSON),
    attestationObject: b64u(r.attestationObject),
    ...(device_sig && device ? { device: { id: device.id, sig: await device.sign(addPasskeyDeviceBody(device.account, id, o.challenge)) } } : {}),
  };
  if (step_up === "passkey") {
    // Its attestation too: the step-up signs the new passkey's event in
    // the account's passkey log (DK3), its key and AAGUID in it.
    const made = Date.now();
    const { provider = null, ...options } = await api<PasskeyToConfirm["options"] & { provider?: string | null }>("/auth/passkey/register/step-up", {
      id: finish.id,
      attestationObject: finish.attestationObject,
    });
    return { finish, provider, made, options };
  }
  await finishRegister(finish);
  return null;
}

/** Why a team's owners can't be changed here (DK7): it takes a passkey,
 * and the account has none. */
export const OWNERS_NEED_PASSKEY =
  "Making someone an owner, or removing one, takes your passkey, and this account has none yet. Add one under Sign-in and account (Add a passkey), then try again.";
/** Or this window can't ask for one (the Linux app window has no WebAuthn). */
export const OWNERS_NEED_WEBAUTHN = "This window can't ask for a passkey: change the team's owners from a browser or a phone.";

/** One of the account's passkeys agrees to the one made (`p`): control
 * keeps it then. Call it from a click. */
export async function passkeyConfirm(p: PasskeyToConfirm): Promise<void> {
  const step_up = await agreeWith(p.options);
  await finishRegister({ ...p.finish, step_up });
}

/** One of the account's passkeys `o` offers signs its challenge: the
 * assertion, as control takes it. */
export async function agreeWith(o: PasskeyToConfirm["options"]) {
  const got = (await navigator.credentials.get({
    publicKey: {
      challenge: unb64u(o.challenge),
      rpId: o.rpId,
      userVerification: o.userVerification,
      timeout: o.timeout,
      allowCredentials: o.allowCredentials.map((c) => ({ type: c.type, id: unb64u(c.id) })),
    },
  })) as PublicKeyCredential | null;
  if (!got) throw new Error("no passkey chosen");
  const a = got.response as AuthenticatorAssertionResponse;
  return {
    id: b64u(got.rawId),
    clientDataJSON: b64u(a.clientDataJSON),
    authenticatorData: b64u(a.authenticatorData),
    signature: b64u(a.signature),
  };
}

/** One of the account's passkeys, the person verified, over `challenge`
 * (SHA-256 of a platform action's bytes, aru-126: the standing grant for
 * runs and its revoke, DK6): its assertion, which is the action's
 * signature. Only the account's own passkeys are offered: a password
 * manager may keep others for this site. Call it from a click. */
export async function passkeyOver(challenge: Uint8Array<ArrayBuffer>): Promise<PasskeyAssertion> {
  const mine = await api<{ passkeys: { id: string }[] }>("/api/me/passkeys").then(
    (r) => r.passkeys.map((p) => ({ type: "public-key" as const, id: unb64u(p.id) })),
    () => [],
  );
  const got = (await navigator.credentials.get({
    publicKey: {
      challenge,
      ...(mine.length ? { allowCredentials: mine } : {}),
      // Control's passkeys are made for its page's host (no port).
      rpId: location.hostname,
      userVerification: "required",
      timeout: 5 * 60 * 1000,
    },
  })) as PublicKeyCredential | null;
  if (!got) throw new Error("no passkey chosen");
  const a = got.response as AuthenticatorAssertionResponse;
  return { id: b64u(got.rawId), clientDataJSON: b64u(a.clientDataJSON), authenticatorData: b64u(a.authenticatorData), signature: b64u(a.signature) };
}

/** What another of the account's passkeys signs to remove one (DK10,
 * #393): its removal in the account's passkey log, and the others to
 * choose from. Asked for as the dialog opens, so the click that confirms
 * goes straight to the passkey. */
export type PasskeyRemoval = PasskeyToConfirm["options"];

export const passkeyRemoval = (id: string) => api<PasskeyRemoval>(`/api/me/passkeys/${encodeURIComponent(id)}/remove/start`, {});

/** Another of the account's passkeys agrees, and control removes `id`
 * (a session alone can't). Call it from a click. */
export async function passkeyRemove(id: string, o: PasskeyRemoval): Promise<void> {
  const assertion = await agreeWith(o);
  await api(`/api/me/passkeys/${encodeURIComponent(id)}/remove`, { assertion });
}

async function finishRegister(body: Record<string, unknown>) {
  await api("/auth/passkey/register/finish", body);
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
 * `restoreInvite`; a join comes back with its code alone meanwhile, so the
 * page still says what it's for. */
/** M152: the hub's `/signin/platform` sent someone without a session here
 * to sign in, with its query as `#platform-signin=` (base64url). Once
 * they're signed in, back to it: it sends the browser on to the platform
 * page with a one-time code. M228: the issuer's `/oauth/authorize` and
 * `/oauth/hud` do the same with `#issuer-signin=`, their path and query.
 * Whether the page is leaving. */
export function resumePlatformSignIn(): boolean {
  const m = /^#(platform|issuer)-signin=([A-Za-z0-9_-]{1,4096})$/.exec(location.hash);
  if (!m) return false;
  let to: string;
  try {
    const raw = atob(m[2].replace(/-/g, "+").replace(/_/g, "/"));
    if (m[1] === "platform") to = `/signin/platform?${new URLSearchParams(raw)}`;
    // M228: the hub's issuer sent a product's sign-in here, as the path
    // it came in on: only its own, never anywhere else (M237: a hud box's
    // review, waiting for its author on the hub's page).
    else if (/^\/oauth\/(authorize|hud|hud\/post)\?[^#\\]*$/.test(raw)) to = raw;
    else return false;
  } catch {
    return false;
  }
  history.replaceState(null, "", location.pathname + location.search);
  location.replace(to);
  return true;
}

export function signInNext(): string {
  const join = JOIN_HASH.exec(location.hash);
  const secret = join ? splitJoinCode(join[1])?.secret : null;
  if (!seedInHash(location.hash) && !secret) return location.pathname + location.hash;
  try {
    sessionStorage.setItem(PENDING_INVITE, JSON.stringify({ hash: location.hash, at: Date.now() }));
  } catch {
    // Without storage the link has to be opened again after signing in.
  }
  return location.pathname + (join ? `#join=${joinInHash()}` : "");
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
    // (a retried join has the same code and a new secret, #760).
    const fresh = Date.now() - at < KEPT_JOIN_MS;
    const join = joinInHash(hash) !== null && wholeJoinInHash() === joinInHash(hash) && fresh;
    if ((!location.hash && seedInHash(hash)) || join) history.replaceState(null, "", location.pathname + location.search + hash);
  } catch {
    // Nothing kept, or not in this shape.
  }
}

/** Where the page is: aru-141 (#1004) took out every phase in which a
 * browser waits to be approved, or was turned down, lost its key or stopped
 * being trusted. */
export type Phase = "loading" | "signed-out" | "ready" | "error";

/** What the person reads when control refuses an approval (#327), by its
 * reason code: which check failed, and what to do next. `revoked` isn't
 * here: control's own words say it (#330), and a removed machine asks
 * again with a new key by itself. */
const REFUSED: Record<string, string> = {
  approver_untrusted: "Your account doesn't trust this browser's key any more, so what it signs is refused. Reload: it works on without the key.",
  bad_signature: "This browser's key isn't the one your account has. Reload: it works on without the key.",
  cant_approve: "A machine can't approve devices. Approve from a browser, phone or the Arugula CLI.",
  needs_browser: "Only a browser or a phone approves this. The desktop app and the Arugula CLI approve machines, CLIs and apps.",
  clock: "This device's clock is too far from Arugula cloud's for the approval to count. Check the clock, reload and approve again.",
  no_chain: "Arugula cloud's records for your account don't add up from here (the approval doesn't chain to your first device). Reload and try again.",
  no_devices: "Your account has no devices to sign this with. Reload and try again.",
};

/** Refusals that mean this browser itself isn't trusted. */
const SELF_REFUSED = new Set(["approver_untrusted", "bad_signature"]);

/** Where an allowed sign-in sends this page, if it may go there: the
 * app's or CLI's loopback port, or (#1247) a native app's callback with
 * this ticket and its grant and nothing else. Anything else throws. */
export function followed(redirect: string, id: string, kind?: AppLogin["kind"]): string {
  if (kind === "native") {
    const [to, query = ""] = redirect.split("?", 2);
    const q = new URLSearchParams(query);
    if (
      NATIVE_CALLBACKS.includes(to) &&
      [...q.keys()].join() === "ticket,grant" &&
      q.get("ticket") === id &&
      /^[0-9a-f]{64}$/.test(q.get("grant") ?? "")
    )
      return redirect;
    throw new Error("Arugula cloud sent the sign-in somewhere other than the Arugula app: not following it");
  }
  const url = new URL(redirect);
  if (url.protocol !== "http:" || url.hostname !== "127.0.0.1" || url.pathname !== "/arugula-signin")
    throw new Error("Arugula cloud sent the sign-in somewhere other than the app on this computer: not following it");
  return url.href;
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
  /** Which one asks (#1018): the CLI's `arugula login`, the desktop
   * app, or (#1247) a native app on a phone, in its sign-in sheet. None
   * from a control before then. */
  kind?: "app" | "cli" | "native";
}

/** Where a native app hears its grant (#1247): its own scheme, which the
 * phone's sign-in sheet this page is in hands back to the app that opened
 * it. Control names one of these exactly. */
const NATIVE_CALLBACKS = ["arugula://signin"];

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
 * it is there. A link a waiting browser handed on before #1004 may carry
 * its code too (`&device=LOOKUP.SECRET`, #761): nothing waits for it now,
 * so it's ignored. */
export const JOIN_HASH = /^#join=([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)?)(?:&device=([A-Za-z0-9-]+\.[A-Za-z0-9-]+))?$/;

/** The machine's join code this page was opened to approve
 * (`#join=CODE`), if any: the code only, which is all control is told. */
export function joinInHash(hash: string = location.hash): string | null {
  const m = JOIN_HASH.exec(hash);
  return m ? (splitJoinCode(m[1])?.code ?? null) : null;
}

/** The whole code in the hash, the secret too (#760): `CODE.SECRET`, or
 * just `CODE`. */
export function wholeJoinInHash(hash: string = location.hash): string | null {
  const m = JOIN_HASH.exec(hash);
  const j = m ? splitJoinCode(m[1]) : null;
  return j ? (j.secret ? `${j.code}.${j.secret}` : j.code) : null;
}

/** Whether this page is the desktop app's window (M48). */
export function inApp(): boolean {
  return !!desktopApp();
}

/** Whether this page is the desktop app's window, even without its
 * script's name (#817): it enrolls as an `app` device, not a browser. */
export function appWindow(): boolean {
  return inApp() || "__TAURI_INTERNALS__" in globalThis;
}

/** The desktop app's device from before it had a kind of its own: a
 * `browser` one it named (`Cert::from_app`). */
export { fromApp };

/** M48: the desktop app's window, after the person allowed its sign-in in
 * their browser: `#app-redeem=<ticket>.<grant>.<verifier>` from the app,
 * posted here for the session cookie. Only in the app's own window (a link
 * to this in a browser would sign it in as someone else). An app from
 * #758 on may add the keys it made and a certificate an older browser's
 * page signed for them (`.<noise>.<sign>.<cert>`): not taken since aru-141
 * (#1004), as the window reaches machines with tokens. */
async function redeemAppLogin(): Promise<void> {
  const m = /^#app-redeem=([0-9a-f]+)\.([0-9a-f]+)\.([0-9a-f]+)(?:\.([0-9a-f]+)\.([0-9a-f]+)\.([A-Za-z0-9_-]+))?$/.exec(location.hash);
  if (!m) return;
  history.replaceState(null, "", location.pathname + location.search);
  if (!inApp()) return;
  await api(`/auth/app/${m[1]}/redeem`, { grant: m[2], verifier: m[3] }).catch(() => {});
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
  keys!: DeviceKeys;
  /** The account's root device as control says, or none: an account made
   * after device keys stopped being made (aru-141). What a roster lists
   * this account with when this browser isn't one of its devices. */
  accountRoot: string | null = null;
  /** aru-141: this browser reaches its machines with machine tokens from
   * control, not its device key: it isn't one of the account's devices,
   * or `tokenMode` says tokens first. */
  tokens = false;
  private machineTokens: MachineTokens = gatedTokens(controlTokens());
  /** #1001: block grants, for a page that reaches its machines with tokens. */
  private blockGrants: BlockGrants = controlBlockGrants();
  enrollment: Enrollment | undefined;
  /** Every device the account trusts, by this browser's reckoning. */
  trusted = new Map<string, Cert>();
  /** Every certificate control has for the account, revoked devices' too:
   * the links of a chain back to the root (`canApprove`). */
  private certs: Cert[] = [];
  /** Browsers and phones that the desktop app's windows (from before it
   * had a kind) approved before #1343 shipped, as control says
   * (`legacy_app_children`): such a window counts just above one of them on
   * the way back to the root, as control counts it, and nowhere else. */
  legacyAppChildren = new Set<string>();
  revocations: Revocation[] = [];
  daemons: DirDaemon[] = [];
  /** Shares waiting for this account's yes. */
  offers: ShareOffer[] = [];
  /** The directory is the saved one: control didn't answer. */
  stale = false;
  /** Control says the account's root is a different device than the one
   * this browser pinned: don't trust anything new from it. */
  rootMismatch = false;
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

  /** Sign-in state, this browser's place, then the directory, kept fresh. */
  async boot() {
    try {
      // M48: the desktop app's window redeems its sign-in. Keys an older
      // browser's page handed it with a certificate aren't taken (aru-141):
      // the window reaches machines with tokens, as any new browser does.
      await redeemAppLogin();
      this.keys = await loadKeys();
      this.enrollment = await loadEnrollment(location.origin);
      if (this.enrollment && this.enrollment.cert.device !== this.keys.id) {
        // #94: the key this browser enrolled with didn't come back from
        // its storage (WebKit loses X25519 keys there), so it would sign
        // what nobody trusts. With machine tokens (aru-141) it works on
        // without it, as a browser that was never a device.
        await forgetEnrollment(location.origin);
        this.enrollment = undefined;
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
      this.accountRoot = me.root;
      this.passkeys = me.passkeys;
      // M152: signed in on the way to the platform page: on to it.
      if (resumePlatformSignIn()) return;
      if (this.enrollment && this.enrollment.account !== me.account) {
        // Signed in as someone else: this browser's place was in another
        // account, and it has none in this one.
        this.enrollment = undefined;
      }
      // aru-141 (#997, #1004): signing in is the whole flow. A browser that
      // isn't one of the account's devices stays that way: it reaches its
      // machines with machine tokens and adds a machine's join in one click
      // (#1003). Nothing asks another device to approve it. One that is a
      // device keeps its key. Only a control that signs no tokens (a dev
      // or test hub), or `tokenMode` "off", makes the account's first
      // browser its first device, signed by itself, with no screen.
      const mode = tokenMode();
      if (!this.enrollment && me.root === null && (mode === "off" || !(await controlIssues()))) await this.enrollFirst();
      if (!this.enrollment || mode === "first") this.tokens = true;
      // Which device this sign-in is, for Sign-in and account to name it
      // (a control without the route says 404).
      if (this.enrollment) void api("/api/me/sessions/device", { device: this.keys.id }).catch(() => {});
      await this.refresh();
      this.set("ready");
      this.timer = window.setInterval(() => void this.refresh(), 10_000);
      // M104: the account's answer about usage counts from this page.
      void this.loadTelemetry();
    } catch (e) {
      this.set("error", String((e as Error).message ?? e));
    }
  }

  /** Where control signs no tokens, or `tokenMode` is "off": this browser
   * becomes the account's first device, its certificate signed by its own
   * key, which it pins as the root. Told it isn't the first after all
   * (another browser got there at the same moment), it stays keyless. */
  private async enrollFirst() {
    const k = this.keys;
    const cert: Cert = {
      v: 1,
      account: this.account,
      device: k.id,
      // The desktop app's window is an app device: it approves machines,
      // CLIs and apps, not browsers, phones or deploys.
      kind: appWindow() ? "app" : "browser",
      name: deviceName(),
      noise: k.noisePub,
      sign: k.signPub,
      created: Date.now(),
      approver: k.id,
      sig: "",
    };
    cert.sig = await signText(k, certBody(cert));
    const r = await api<{ approved: boolean }>("/api/devices", { cert });
    if (!r.approved) {
      // Another browser is the first: drop the request this one left.
      await api(`/api/devices/${k.id}/reject`, {}).catch(() => {});
      return;
    }
    const all = await api<{ trust: { account: string; root: string }; certs: Cert[]; revocations: Revocation[] }>("/api/devices");
    // Taken as the account's first device: its root can only be this key.
    if (all.trust.root !== k.id) throw new Error("Arugula cloud took this browser as the account's first device, then named another as its root: not trusting it");
    const mine = (await evaluate(all.trust, all.certs, all.revocations)).get(k.id);
    if (!mine) throw new Error("Arugula cloud took this browser as the account's first device, but its certificate doesn't check out");
    this.enrollment = { control: location.origin, account: this.account, root: all.trust.root, cert: mine };
    await saveEnrollment(this.enrollment);
  }

  /** #327, #1004: the account doesn't trust this browser's key. It works
   * on with machine tokens, as a browser that was never a device. Removed
   * (`forget`), it forgets the key's place for good, since a removed device
   * never comes back; only missing from what control lists, it keeps it for
   * the next page load to check again. */
  private async dropDevice(forget: boolean) {
    if (forget) await forgetEnrollment(location.origin).catch(() => {});
    this.enrollment = undefined;
    this.trusted = new Map();
    this.certs = [];
    this.revocations = [];
    this.rootMismatch = false;
    this.tokens = true;
  }

  /** A passkey made, waiting for one the account has to agree. */
  passkeyToConfirm: PasskeyToConfirm | null = null;

  /** A passkey for the account signed in: kept at once for an account with
   * none, and once one of its own agrees for an account with some. */
  async addPasskey() {
    const signer = this.enrollment ? { account: this.account, id: this.keys.id, sign: (t: string) => signText(this.keys, t) } : undefined;
    const waiting = await passkeyRegister(undefined, signer);
    if (waiting) {
      this.passkeyToConfirm = waiting;
      this.emit();
      return;
    }
    await this.refreshMe();
  }

  /** One of the account's passkeys agrees to the one made (from a click). */
  async confirmPasskey() {
    const p = this.passkeyToConfirm;
    if (!p) return;
    try {
      await passkeyConfirm(p);
    } finally {
      this.passkeyToConfirm = null;
      this.emit();
    }
    await this.refreshMe();
  }

  /** Never mind the passkey made: control never kept it. */
  dropPasskey() {
    this.passkeyToConfirm = null;
    this.emit();
  }

  /** How many passkeys the account has now. */
  async refreshMe() {
    this.passkeys = (await api<{ passkeys: number }>("/api/me")).passkeys;
    this.emit();
  }

  /** Devices and the directory, checked against the pinned root. */
  async refresh() {
    const e = this.enrollment;
    if (!e) return this.tokens ? this.refreshForTokens() : undefined;
    try {
      const [devs, dir] = await Promise.all([
        api<{
          trust: { account: string; root: string } | null;
          certs: Cert[];
          revocations: Revocation[];
          legacy_app_children?: string[];
        }>("/api/devices"),
        api<{ daemons: ForeignEntry[]; offers?: ShareOffer[] }>("/api/directory"),
      ]);
      this.rootMismatch = !!devs.trust && devs.trust.root !== e.root;
      // As this page last held them: removals only add up, and a
      // certificate first seen after its approver's removal doesn't count.
      // One look, with the people whose machines the directory lists (by
      // the roots this browser pinned for them on first sight).
      const ownTrust = { account: e.account, root: e.root };
      const looked = await this.held.look(async (l) => {
        const own = await l.take(ownTrust, devs);
        const theirs = new Map<string, { certs: Cert[]; revocations: Revocation[] }>();
        for (const d of dir.daemons) {
          const t = d.chain?.trust;
          if (!d.hosted && d.account && t && !theirs.has(`${t.account}.${t.root}`) && pin(d.account, t.root)) theirs.set(`${t.account}.${t.root}`, await l.take(t, d.chain!));
        }
        return { own, theirs };
      });
      // Not kept: the view this page held before, not control's answer
      // (control's, on a first look with nothing held).
      const own = looked.kept ? looked.value.own : (this.held.view(ownTrust) ?? looked.value.own);
      devs.certs = own.certs;
      devs.revocations = own.revocations;
      this.trusted = await evaluate({ account: e.account, root: e.root }, devs.certs, devs.revocations);
      this.certs = devs.certs;
      this.revocations = devs.revocations;
      this.legacyAppChildren = new Set(devs.legacy_app_children ?? []);
      // #327, #1004: this browser's own place, checked like any other
      // device's. One the account no longer trusts works on with machine
      // tokens, as a browser that was never a device. (A different root is
      // said elsewhere, and isn't fixed by dropping the key.)
      if (!this.trusted.has(this.keys.id) && !this.rootMismatch) {
        await this.dropDevice(devs.revocations.some((r) => r.device === this.keys.id));
        return this.refreshForTokens();
      }
      const daemons: DirDaemon[] = [];
      const hosted: ForeignEntry[] = [];
      for (const d of dir.daemons) {
        const { chain, hosted: proof, ...entry } = d;
        if (proof) {
          hosted.push(d);
          continue;
        }
        let cert = this.trusted.get(d.id);
        // Someone else's machine: by their account's certificates, from
        // the root this browser pinned for them on first sight.
        // Only once what this browser checked of them is kept.
        const theirs = chain?.trust && looked.value.theirs.get(`${chain.trust.account}.${chain.trust.root}`);
        if (!cert && looked.kept && theirs && chain?.trust) cert = (await evaluate(chain.trust, theirs.certs, theirs.revocations)).get(d.id);
        if (cert?.kind === "daemon") daemons.push({ ...entry, cert });
      }
      this.offers = dir.offers ?? [];
      await this.loadTeams();
      // Hosted machines (M178), checked against what this browser holds:
      // its team's roster, or its own account's chain. Never one of its
      // account's devices, and nothing outside their project.
      for (const d of hosted) daemons.push(...(await this.acceptHosted(d, devs)));
      this.daemons = daemons;
      await this.loadBilling();
      this.stale = false;
      try {
        localStorage.setItem(DIR_KEY, JSON.stringify(this.daemons));
      } catch {
        // not remembered
      }
      // The service worker answers notifications over the same channels
      // (M29), from what this page checked.
      void saveWorkerDirectory(this.workerDirectory()).catch(() => {});
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
    // Billing is the platform's, but every refresh waits for this, so a
    // maintenance window (M141) isn't waited out here: what was read stays
    // until a refresh after it.
    this.billing = await api<NonNullable<ControlSession["billing"]>>("/api/billing").catch((e) =>
      e instanceof HttpError && e.status === 503 ? this.billing : null,
    );
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

  /** The last roster this page checked for each team (`RosterStore`). */
  private rosters = new RosterStore(() => localStorage, ROSTERS_KEY, () => !!this.enrollment?.held);
  /** Each account's certificates and removals as last held (`TrustStore`). */
  private held = new TrustStore(() => localStorage, HELD_CERTS_KEY, () => !!this.enrollment?.held);
  /** Control's word of delegations that ended, as heard (`ClosureStore`). */
  private closures = new ClosureStore(() => localStorage, CLOSURES_KEY);

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
    const held = this.rosters.held();
    const kept = rootChangesKept();
    const lateKept = rosterLateKept();
    // What the look needs from control, fetched before it takes the
    // browser-wide lock (an answer that's slow to come mustn't hold up
    // every tab's look), each given up after FETCH_TIMEOUT_MS: the versions
    // after the one held, for each team control has a newer one of.
    const fetched = new Map<string, { since: number; got: { rosters: Roster[]; certs: AccountCerts } | null }>();
    await Promise.all(
      r.teams
        .filter((t) => t.roster.version > (held[t.team]?.version ?? 0))
        .map(async (t) => {
          const since = held[t.team]?.version ?? 0;
          const path = `/api/teams/${encodeURIComponent(t.team)}/rosters?since=${since}`;
          const got = await api<{ rosters: Roster[]; certs: AccountCerts }>(path, undefined, withTimeout(fetch)).catch(() => null);
          fetched.set(t.team, { since, got });
        }),
    );
    // One look at what this page holds, for every team.
    const looked = await this.held.look(async (l) => {
      // An account's certificates as this page holds them, from `root`.
      const take = async (account: string, root: string, [certs, revocations]: [Cert[], Revocation[]]): Promise<[Cert[], Revocation[]]> => {
        const got = await l.take({ account, root }, { certs, revocations });
        return [got.certs, got.revocations];
      };
      for (const t of r.teams) {
        // The founder pinned on first sight. The team's daemons check each
        // roster version against it; this browser checks the ones it signs.
        const ok = pin(`team:${t.team}`, `${t.pin.founder}.${t.pin.founder_root}`);
        // And each version from the last this browser checked: an owner's
        // change is signed on top of the one shown, so that one must be a
        // version owners wrote, not just what control says is latest.
        // Each step is checked with the certificates this page holds for
        // that version's owners, from the roots it names (history's too);
        // a version a device removed since had signed stands only if this
        // page hadn't seen that removal before.
        const c = await checkRoster(
          held[t.team] ?? null,
          t.roster,
          t.pin,
          t.certs,
          // Only what was fetched before the look: nothing from control
          // while it holds the lock.
          async (since) => {
            const f = fetched.get(t.team);
            if (!f?.got || f.since !== since) throw new Error("the team's history wasn't fetched");
            return f.got;
          },
          {
            vet: (prev, certs) => ownersAsHeld(prev, t.pin, certs, take),
            lateOk: (account, root, device) => !l.removedBefore(account, root, device),
          },
        );
        // The team's members and roles are control's (aru-141, Arugula
        // #1005): a version that doesn't follow the one this page checked
        // shows as control has it, and changes go on top of it on control's
        // word, not signed by this device.
        const roster = c.problem ? t.roster : c.roster;
        // Members' certificates as this page holds them, from the roots the
        // roster shown names.
        for (const m of roster.members) if (t.certs[m.account]) t.certs[m.account] = await take(m.account, m.root, t.certs[m.account]);
        for (const a of Object.keys(t.certs)) if (!roster.members.some((m) => m.account === a)) delete t.certs[a];
        // Locked or not as control says (aru-141). An owner's signed lock
        // that checks out is still held, for the record.
        const lock = ok && !c.problem ? await takeLock(l, t.team, t.lock, c.roster, t.certs) : l.lock(t.team);
        t.lock = lock ?? null;
        if (ok && c.keep && c.keep.version > (held[t.team]?.version ?? 0)) held[t.team] = c.keep;
        if (ok && c.changes.length) kept[t.team] = [...(kept[t.team] ?? []), ...c.changes];
        if (ok && c.late.length) lateKept[t.team] = [...new Set([...(lateKept[t.team] ?? []), ...c.late])].sort((a, b) => a - b);
        // #762: only someone who held an owner's link; anything else control
        // says waits is refused here, never shown.
        const requests: Team["requests"] = [];
        for (const q of t.requests ?? []) {
          const by = await askOk(roster, t.certs, q);
          if (by !== null) requests.push({ ...q, role: q.invite.role, by });
        }
        out.push({
          ...t,
          roster,
          requests,
          verified: ok && !c.problem,
          ...(c.problem ? { rosterProblem: c.problem } : {}),
          ...(kept[t.team]?.length ? { rootChanges: kept[t.team] } : {}),
          ...(lateKept[t.team]?.length ? { rosterLate: lateKept[t.team] } : c.late.length ? { rosterLate: c.late } : {}),
        });
      }
    });
    this.rosters.save();
    // From now on, storage found empty was cleared, not never kept.
    if (this.enrollment && !this.enrollment.held && this.rosters.kept && looked.kept) {
      this.enrollment = { ...this.enrollment, held: true };
      await saveEnrollment(this.enrollment).catch(() => {});
    }
    writeKey(ROOT_CHANGES_KEY, kept);
    writeKey(ROSTER_LATE_KEY, lateKept);
    // Checked, but not kept: no change is signed on what this browser
    // would forget.
    if (!this.rosters.kept || !looked.kept) for (const t of out) t.rosterProblem ??= "unsaved";
    const now = r.asked ?? [];
    const yes = this.asked.find((a) => !now.some((x) => x.team === a.team) && out.some((t) => t.team === a.team));
    if (yes) this.joined = { team: yes.team, name: yes.name };
    this.asked = now;
    this.teams = out;
  }

  /** A hosted machine from the directory, if this browser accepts it for
   * its project (M178): as a machine of that project only. */
  private async acceptHosted(d: ForeignEntry, devs: { certs: Cert[]; revocations: Revocation[] }): Promise<DirDaemon[]> {
    const e = this.enrollment;
    const { chain: _chain, hosted: proof, ...entry } = d;
    if (!e || !proof) return [];
    const owner = proof.delegation.owner;
    const team = owner.startsWith("team:") ? this.teams.find((t) => `team:${t.team}` === owner && t.verified) : undefined;
    const owners = team
      ? { team: team.roster, certs: team.certs }
      : { account: { account: e.account, root: e.root }, certs: devs.certs, revocations: devs.revocations };
    const ok = await acceptHostedHolding(e.account, proof, owners, this.closures);
    if (!ok.ok) return [];
    const c = proof.cert;
    // Its keys, to open a channel with; never in the account's trust.
    const cert: Cert = { v: 1, account: c.project, device: c.device, kind: "daemon", name: c.name, noise: c.noise.toLowerCase(), sign: c.sign.toLowerCase(), created: c.created, approver: "", sig: "" };
    // Reached only through the relay: no direct URLs, whatever the
    // directory said.
    return [{ ...entry, urls: [], cert, hosted: { project: ok.project, workspace: ok.workspace, role: ok.role } }];
  }

  // ---- project delegations (M178)

  /** Control's hosted key and the delegations of this account's projects. */
  async delegations(): Promise<{ key: string; delegations: { id: string; delegation: Delegation; revoke: DelegationRevoke | null; ended?: boolean; by_account: string }[] }> {
    return api("/api/hosted/delegations");
  }

  /** As an owner, let control enroll hosted machines into `project`
   * (owned by `team:<id>` or `account:<id>`), signed by this device. */
  async delegate(project: string, owner: string): Promise<string> {
    const { key } = await this.delegations();
    const d = await signDelegation(project, owner, key, this.keys);
    const r = await api<{ id: string }>("/api/hosted/delegations", { delegation: d });
    return r.id;
  }

  /** As an owner, take a delegation back: nothing more enrolls under it. */
  async undelegate(d: Delegation): Promise<void> {
    await api("/api/hosted/delegations/revoke", { revoke: await signRevoke(d, this.keys) });
  }

  // ---- the standing project grant for runs (M211), kept by the platform

  /** The project's standing grant, and whether one is offered. */
  async runGrant(project: string): Promise<RunGrantView> {
    return api(`/api/projects/${encodeURIComponent(project)}/runs/grant`, undefined, platformFetch);
  }

  /** As an owner, let the platform start and instruct runs on every
   * hosted workspace of `d`'s project for `days`, confirmed with one of
   * this account's passkeys over the grant's bytes (v2, DK6): no device
   * key signs it. It takes the place of the project's grant, if it has one
   * (renewing). Call it from a click. */
  async signRunGrant(d: Delegation, days: number): Promise<void> {
    // Past the project's last grant, whatever this device's clock says: hosted
    // machines go by this time to tell which grant is newest.
    const view = await this.runGrant(d.project);
    const last = [view.grant, ...view.past].reduce((m, g) => Math.max(m, g?.grant.made ?? 0), 0);
    const made = Math.max(Date.now(), last + 1);
    const g = await newGrant(d, this.account, expiresIn(days, made), undefined, made);
    const grant = { ...g, passkey: await passkeyOver(await passkeyChallenge(grantBody(g))) };
    await api(`/api/projects/${encodeURIComponent(d.project)}/runs/grant`, { grant }, platformFetch);
  }

  /** As an owner, take back every grant of `project` under `delegation`
   * made by now, v1 or v2, confirmed with a passkey (v3, DK6): no new runs;
   * `stop` stops the ones going too. A revoke names no grant, so the
   * platform can't point it elsewhere. Call it from a click. */
  async revokeRunGrants(project: string, delegation: string, stop: boolean): Promise<void> {
    const r = newGrantRevoke(project, delegation, stop ? "stop" : "finish", this.account);
    const revoke = { ...r, passkey: await passkeyOver(await passkeyChallenge(grantRevokeBody(r))) };
    await api(`/api/projects/${encodeURIComponent(project)}/runs/grant/revoke`, { revoke }, platformFetch);
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
    return !!d && (!d.account || d.account === this.account) && !d.team && !d.hosted;
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
    return { account: this.account, root: this.myRoot(), name: word(this.name || `account-${this.account.slice(0, 6)}`) };
  }

  /** The root a roster lists this account with: this browser's account
   * root, control's word for it, or none (aru-141). */
  private myRoot(): string {
    return this.enrollment?.root ?? this.accountRoot ?? NO_ROOT;
  }

  /** Whether this browser signs a change to `t` with its device key, as
   * before aru-141, so machines from before it keep following the team:
   * only while it's one of the account's devices and the roster it shows
   * is one it checked. Otherwise the change is control's word, unsigned
   * (Arugula #1005). */
  private signs(t: Team): boolean {
    return !!this.enrollment && t.verified;
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
  /** `/api/me/telemetry` has answered (or failed): `telemetry` is known. */
  private telemetryLoaded = false;

  telemetryNotNow() {
    this.telemetryLater = true;
    this.emit();
  }

  async loadTelemetry() {
    this.telemetry = await api<PageConsent>("/api/me/telemetry").catch(() => null);
    this.telemetryLoaded = true;
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

  /** Whether the account's question is out of the way, so a machine's may
   * come (M103): answered, or never asked by this control. Not while the
   * account's card waits, nor after it was put away on this load. */
  telemetryAsked(): boolean {
    const t = this.telemetry;
    return this.telemetryLoaded && !(t?.ask && t.on === null);
  }

  private countIfAllowed() {
    if (this.telemetry?.on) {
      if (!counting()) startCounting(sendPageCounts);
    } else stopCounting();
  }

  async createTeam(name: string) {
    const named = name.trim().slice(0, 80) || "team";
    if (!this.enrollment) {
      // On control's word (aru-141): control writes the first version.
      await api("/api/teams", { name: named });
      return this.refresh();
    }
    const team = Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => b.toString(16).padStart(2, "0")).join("");
    const roster = await signRoster({ v: 1, team, name: named, version: 1, at: Date.now(), members: [{ ...this.myMember(), role: "owner" }] }, this.keys);
    await api("/api/teams", { roster });
    await this.refresh();
  }

  /** Send the team's next roster, as `change` makes it: signed by this
   * device when it signs for the team (`signs`), else on control's word
   * (aru-141), which control takes from an owner signed in here. */
  async changeTeam(team: string, change: (members: Roster["members"]) => Roster["members"], name?: string) {
    const t = this.teams.find((x) => x.team === team);
    if (!t) throw new Error("no such team");
    const prev = t.roster;
    const draft = nextRoster(prev, change, name);
    let next: Roster = { ...draft, by: "", sig: "" };
    if (this.signs(t)) {
      const signed = await signRoster(draft, this.keys);
      // One this device can't sign as an owner here (it was made an owner
      // on control's word, say) goes on control's word too.
      if (await follows(signed, prev, t.pin, t.certs)) next = signed;
    }
    // One that changes who the owners are takes this owner's passkey over
    // its bytes too (DK7, aru-126), whichever device signed it.
    const passkey = changesOwners(next, prev) ? await this.confirmOwners(next) : undefined;
    await api(`/api/teams/${team}/roster`, passkey ? { roster: next, passkey } : { roster: next });
    await this.refresh();
  }

  /** This account's passkey over a roster version that changes the team's
   * owners (`Roster::owner_challenge`, DK7): control takes such a version
   * only with it. Asked the way a deploy's approval asks (user verification
   * required, only the account's own passkeys). Call it from a click. */
  private async confirmOwners(r: Roster) {
    if (this.passkeys === 0) throw new Error(OWNERS_NEED_PASSKEY);
    if (typeof PublicKeyCredential === "undefined") throw new Error(OWNERS_NEED_WEBAUTHN);
    const mine = await api<{ passkeys: { id: string }[] }>("/api/me/passkeys").then(
      (x) => x.passkeys,
      () => [],
    );
    return agreeWith({
      challenge: b64u(await ownerChallenge(r)),
      // Control's passkeys are made for the host its page is on.
      rpId: location.hostname,
      userVerification: "required",
      timeout: 5 * 60 * 1000,
      allowCredentials: mine.map((p) => ({ type: "public-key" as const, id: p.id })),
    });
  }

  /** The root changes and removed signers shown for `team` were seen:
   * don't show them again. */
  dismissRootChanges(team: string) {
    const kept = rootChangesKept();
    delete kept[team];
    writeKey(ROOT_CHANGES_KEY, kept);
    const late = rosterLateKept();
    delete late[team];
    writeKey(ROSTER_LATE_KEY, late);
    this.teams = this.teams.map((t) => (t.team === team ? { ...t, rootChanges: undefined, rosterLate: undefined } : t));
    this.emit();
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
  async makeInvite(team: string, role: TeamRole, askFirst = false): Promise<MadeInvite> {
    // Signed by this device when it signs for the team, else on control's
    // word (aru-141): control takes it from an owner signed in here.
    const t = this.teams.find((x) => x.team === team);
    const sign = t && this.signs(t) ? { invite: (i: Omit<Invite, "by" | "sig">) => signInvite(i, this.keys), ask: (i: Omit<AskInvite, "by" | "sig">) => signAskInvite(i, this.keys) } : undefined;
    return makeInvite({ team, role, askFirst, origin: location.origin, post: (path, body) => api(path, body), sign });
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
      // A link control made (aru-141) is its word: control checks it holds
      // it. One an owner's device signed is checked here, as before.
      if (!unsigned(p.invite)) {
        const mine = await api<{ certs: Cert[]; revocations: Revocation[] }>("/api/devices");
        const certs: AccountCerts = { ...p.certs, [this.account]: [mine.certs, mine.revocations] };
        if (!(await follows(next, p.roster, p.pin, certs))) throw new Error("that invite doesn't check out");
      }
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
    const proof = await k.sign(askBody({ team, key: k.key }, this.account, this.myRoot()));
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

  /** Lock or unlock a team: signed by this device (`TeamLock`) when it
   * signs for the team, so machines from before aru-141 take it too; else
   * on control's word, which machines take now (Arugula #1005). */
  async lockTeam(team: string, locked: boolean) {
    const t = this.teams.find((x) => x.team === team);
    if (!t) throw new Error("no such team");
    if (!this.signs(t)) {
      await api(`/api/teams/${team}/lock`, { locked });
      return this.refresh();
    }
    // After the lock this page checked and holds (never dated far ahead).
    const lock = await signLock({ team, locked, version: t.roster.version, at: Math.max(Date.now(), (this.held.lock(team)?.at ?? 0) + 1) }, this.keys);
    await api(`/api/teams/${team}/lock`, { locked, lock });
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
    // No root for someone with no device key (aru-141, #1004): shared with
    // on control's word.
    return api<{ account: string; name: string; root: string | null }>(`/api/people?login=${encodeURIComponent(login)}`);
  }

  /** Hand control a push subscription, signed by this device (M21). A live
   * one (the desktop app's) is answered with the token it listens with. */
  async subscribePush(sub: { endpoint: string; p256dh: string; auth: string }): Promise<{ listen?: string }> {
    const s = { v: 1, account: this.account, device: this.keys.id, endpoint: sub.endpoint, p256dh: sub.p256dh, auth: sub.auth, at: Date.now(), sig: "" };
    // Frozen (#504): signed.
    const body = `illogical push v1\naccount ${s.account}\ndevice ${s.device}\nendpoint ${s.endpoint}\np256dh ${s.p256dh}\nauth ${s.auth}\nat ${s.at}\n`;
    s.sig = await signText(this.keys, body);
    return api<{ listen?: string }>("/api/push/subscribe", { sub: s });
  }

  async unsubscribePush(endpoint: string) {
    await api("/api/push/unsubscribe", { endpoint });
  }

  /** What the account is pushed about, on every device it turned on. */
  async pushPrefs() {
    return api<PushPrefs>("/api/push/prefs");
  }

  /** Turn some kinds on or off; the rest stay. */
  async setPushPrefs(prefs: Partial<PushPrefs>) {
    return api<PushPrefs>("/api/push/prefs", prefs);
  }

  /** #468: how the account's push subscriptions stand, and whether control
   * has this device's (`endpoint`) and may use it. */
  async pushCheck(endpoint: string | null) {
    return api<PushStanding>("/api/push/check", { endpoint });
  }

  /** #468: a test notification to every subscription the account has. */
  async pushTest(endpoint: string | null) {
    return api<PushStanding>("/api/push/test", { endpoint });
  }

  /** The account's prefs for its pages (hints seen, Tips, What's new:
   * shared/seen.ts). */
  async prefs() {
    return api<Record<string, unknown>>("/api/me/prefs");
  }

  /** Set some (`null` takes one back); the rest stay. */
  async setPrefs(changes: Record<string, unknown>) {
    return api<Record<string, unknown>>("/api/me/prefs", changes);
  }

  /** What the service worker answers notifications to (M29), and with
   * what: machine tokens when this page uses them. */
  private workerDirectory(): WorkerDaemon[] {
    const ws = this.info.url.replace(/^http/, "ws");
    return this.daemons.map((d) => ({ id: d.id, noise: d.cert.noise, direct: directUrls(d), relay: `${ws}/api/relay/c/${d.id}`, ...(this.tokens ? { tokens: true } : {}) }));
  }

  /** aru-141: the directory for a browser that reaches machines with
   * machine tokens: every machine control lists, by the Noise key control
   * gives (#994), else the one in the certificate it lists beside it,
   * checked against nothing: control decides who reaches a machine now. */
  private async refreshForTokens() {
    try {
      const dir = await api<{ daemons: ForeignEntry[]; offers?: ShareOffer[] }>("/api/directory");
      // A control from before #994: the machines' certificates, as control
      // keeps them for the account.
      const needCerts = dir.daemons.some((d) => !d.noise && !d.hosted && !d.chain?.certs.some((c) => c.device === d.id));
      const own = needCerts ? await api<{ certs: Cert[] }>("/api/devices").catch(() => ({ certs: [] as Cert[] })) : { certs: [] as Cert[] };
      this.offers = dir.offers ?? [];
      await this.loadTeams();
      this.daemons = dir.daemons.flatMap((d) => {
        const entry = tokenEntry(d, own.certs, this.account, this.teams);
        return entry ? [entry] : [];
      });
      await this.loadBilling();
      this.stale = false;
      try {
        localStorage.setItem(DIR_KEY, JSON.stringify(this.daemons));
      } catch {
        // not remembered
      }
      void saveWorkerDirectory(this.workerDirectory()).catch(() => {});
    } catch (err) {
      if (err instanceof HttpError && err.status === 401) {
        window.clearInterval(this.timer);
        return this.set("signed-out");
      }
      this.stale = true;
      try {
        this.daemons = (JSON.parse(localStorage.getItem(DIR_KEY) ?? "[]") as DirDaemon[]).map((d) => ({ ...d, online: false }));
      } catch {
        // nothing saved
      }
    }
    this.emit();
  }

  /** How a Client reaches daemon `id`. */
  target(id: string): E2ETarget | undefined {
    const d = this.daemons.find((x) => x.id === id);
    if (!d) return undefined;
    const ws = this.info.url.replace(/^http/, "ws");
    // Every machine shares the page's one socket to the relay (M25).
    const mux = `${ws}/api/relay/m`;
    // M50: blocks on control's block origins, where control serves them.
    const blockDomain = this.info.block_domain;
    const hosted = d.hosted ? { project: d.hosted.project, workspace: d.hosted.workspace } : undefined;
    // aru-141: machine tokens, or this browser's device key. A device
    // keeps its key for what's still signed with one: blocks' grants on a
    // daemon from before the hub's (#1001).
    const keys = this.enrollment ? this.keys : undefined;
    const tokens = this.tokens ? this.machineTokens : undefined;
    const blockGrants = this.tokens ? this.blockGrants : undefined;
    return { daemon: { id: d.id, noise: d.cert.noise }, direct: directUrls(d), relay: `${ws}/api/relay/c/${d.id}`, mux, keys, tokens, blockGrants, blockDomain, hosted };
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
    // UI audit #149: when it ends rides along, so its page can say so (the
    // daemon ends it then either way). A link made before says only that
    // it's read-only.
    const expires = (await res.json<{ expires?: number }>().catch(() => null))?.expires;
    return `${this.info.url}/#link=${d.id}.${d.cert.noise}.${hex(seed)}.${hex(pub)}${typeof expires === "number" ? `.${expires}` : ""}`;
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

  /** #1003: add a machine's join in one click, as this signed-in person,
   * with no device's signature: control makes it `team`'s (one I own or
   * edit) or this account's, and its poll answers with where it went. The
   * machine pins no account root, so devices don't reach it by their keys
   * (aru-141); machine tokens do. What a browser that isn't one of the
   * account's devices does; a device signs the join, as before.
   *
   * Into this account, or into `team`, as its owner's machine also used
   * for the team or, with `teamBox`, as the team's (Arugula #1182). With
   * the join's `secret` (from its link), the Add carries a MAC under it
   * over this account and the machine's keys (`oneClickMac`), which the
   * machine checks before it saves the account: control never sees the
   * secret, and a machine that made one takes no Add without it. */
  async addJoin(code: string, c: Cert, team: string | null = null, teamBox = false, secret: string | null = null) {
    const mac = secret ? await oneClickMac(secret, this.account, c) : null;
    const body = { team, ...(team ? { team_box: teamBox } : {}), ...(mac ? { mac } : {}) };
    await this.approving(api(`/api/joins/${code}/approve`, body));
    await this.refresh();
  }

  /** Whose a machine in a team is (Arugula #1182): the team's, or its
   * owner's, also used for the team. Its owner or the team's owners
   * change it; the machine takes it at its next refresh. */
  async setTeamBox(daemon: string, teamBox: boolean) {
    await api(`/api/daemons/${daemon}/team-box`, { team_box: teamBox });
    await this.refresh();
  }

  /** Teams I can move my machines into (#332), by control's rule
   * (`movesInto`): those I own or edit, but only those I own while they're
   * locked (#1184).
   * A device page signs the team in, so only one whose roster it checked;
   * with no device key (aru-141) control's word is all there is. */
  addableTeams(): Team[] {
    return this.teams.filter((t) => (t.verified || !this.enrollment) && movesInto(t));
  }

  /** Approve a join (a machine's, or the Arugula CLI's or the phone app's
   * as a device) into this account, or a machine into `team` (one I'm
   * in): this device signs the team in, so control can't pick one (#100).
   * With the join's `secret` (#760), the approval carries a MAC under it
   * over this account and the root this device pinned (not control's word
   * for it), which the machine checks before it pins that root. */
  async approveJoin(code: string, c: Cert, team: string | null = null, secret: string | null = null, teamBox = false) {
    // Dated now: control takes approvals made within minutes of its clock.
    const signed: Cert = { ...c, account: this.account, created: Date.now(), approver: this.keys.id, sig: "" };
    signed.sig = await signText(this.keys, certBody(signed));
    let mac: string | null = null;
    if (secret) {
      if (!this.enrollment || this.rootMismatch) throw new Error("this browser's account root isn't settled: approve from a device you've used before");
      mac = await joinMac(secret, { account: this.account, root: this.enrollment.root }, c);
    }
    let teamSig: string | null = null;
    if (team) {
      const t = this.addableTeams().find((x) => x.team === team);
      if (!t) throw new Error("only the team's owners and editors add machines to it");
      teamSig = await signText(this.keys, teamJoinBody(c.device, t.pin));
    }
    try {
      // Into a team: whose it is (Arugula #1182), control's to keep.
      const whose = team ? { team_box: teamBox } : {};
      await this.approving(api(`/api/joins/${code}/approve`, { cert: signed, team, team_sig: teamSig, ...whose, ...(mac ? { mac } : {}) }));
    } finally {
      await this.refresh();
    }
  }

  /** Move a machine of this account into `team` (one I'm in), or back to
   * the account (null). A device signs it for the daemon to check; a page
   * with no device key (aru-141, #1004) sends it unsigned, on control's
   * word, which control takes from the machine's owner on its page. A
   * team's owner takes someone else's machine out of it the same way. */
  async moveDaemon(daemon: string, team: string | null) {
    let pin: TeamPin | null = null;
    if (team) {
      const t = this.addableTeams().find((x) => x.team === team);
      if (!t) throw new Error("only the team's owners and editors add machines to it");
      pin = t.pin;
    }
    const at = Date.now();
    const signs = !!this.enrollment && !this.rootMismatch;
    const sig = signs ? await signText(this.keys, moveBody(daemon, pin, at)) : "";
    await api(`/api/daemons/${daemon}/team`, { team: pin, at, by: signs ? this.keys.id : "", sig });
    await this.refresh();
  }

  /** Rename a machine (Arugula #500): its owner's, or its team's owners'.
   * Only the display name changes, and `@old` keeps reaching it. Control
   * refuses a name another of the owner's (or the team's) machines has. */
  async renameDaemon(daemon: string, name: string): Promise<string> {
    const r = await api<{ name: string }>(`/api/daemons/${daemon}/name`, { name });
    await this.refresh();
    return r.name;
  }

  /** Turn a daemon's join down: it stops waiting. */
  /** M48: a desktop app asking to sign in as this account (`#app=`):
   * where it asked from, and whether that's this browser's network. An
   * app from #758 on sends its device keys; one from before has a code. */
  async showAppLogin(id: string): Promise<AppLogin> {
    // One lookup per page load: the prompt mounts afresh when the page
    // picks a machine (`ControlRoot` is keyed by the client's base), and
    // asking again would put "Looking it up…" back under the person's eyes.
    let p = this.appLogins.get(id);
    if (!p) {
      p = api<AppLogin>(`/api/app-login/${encodeURIComponent(id)}`);
      this.appLogins.set(id, p);
      p.catch(() => this.appLogins.delete(id));
    }
    return p;
  }
  private appLogins = new Map<string, Promise<AppLogin>>();

  /** Whether this device may approve one of `kind` (`Kind::certifies`):
   * the desktop app, its devices from before it had a kind, and anything
   * they certified approve machines, CLIs and apps, not browsers, phones
   * or recovery codes; but an app window from before it had a kind counts
   * just above a browser it approved back then (`legacyAppChildren`). */
  canApprove(kind: Kind): boolean {
    const me = this.enrollment?.cert;
    if (!me || !certifies(me.kind, kind)) return false;
    if (kind !== "browser" && kind !== "recovery") return true;
    // A browser's own all the way back to the root, as control asks.
    const e = this.enrollment;
    return !!e && browserOrLegacyChained({ account: e.account, root: e.root }, this.certs, me.device, this.legacyAppChildren);
  }

  /** Allow it: control answers with the app's loopback address, which this
   * browser hands the grant to (only the app on this computer hears it).
   * Since aru-141 (#1004) no certificate goes with it: the app's window
   * reaches machines with tokens, and isn't one of the account's devices.
   * A native app's sign-in (#1247) goes to its callback instead, and only
   * a native app's does: the sheet hands it to the app, whose verifier
   * alone redeems it. */
  async allowAppLogin(id: string, kind?: AppLogin["kind"]): Promise<string> {
    const r = await api<{ redirect: string }>(`/api/app-login/${encodeURIComponent(id)}/allow`, {});
    return followed(r.redirect, id, kind);
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

  /** Remove one of the account's machines. A device signs its removal,
   * which machines from before tokens check; with no device key, control
   * takes it on the account's word (`DELETE /api/daemons/{id}`, #1017):
   * the machine leaves the directory and the relay, and its key never joins
   * again. */
  async removeMachine(id: string) {
    if (this.enrollment && !this.rootMismatch && this.trusted.get(id)?.kind === "daemon") return this.revoke(id);
    const res = await fetch(`/api/daemons/${encodeURIComponent(id)}`, { method: "DELETE" });
    if (!res.ok) {
      const j = (await res.json().catch(() => ({}))) as { error?: string; reason?: string };
      throw new HttpError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`), j as Record<string, unknown>);
    }
    await this.refresh();
  }

  async revoke(id: string) {
    const r: Revocation = { v: 1, account: this.account, device: id, at: Date.now(), by: this.keys.id, sig: "" };
    r.sig = await signText(this.keys, revocationBody(r));
    await api("/api/revocations", { revocation: r });
    await this.refresh();
  }

  async signOut(forgetDevice: boolean) {
    await api("/auth/logout", {}).catch(() => {});
    if (forgetDevice) await forget(location.origin);
    location.href = "/";
  }
}

/** `GET /api/projects/{project}/runs/grant` (M211). */
export interface RunGrantView {
  project: string;
  owner: string;
  role: "owner" | "editor" | "viewer";
  grant: { grant: ProjectGrant; body: string; posted_by: string | null; created_at: number } | null;
  past: { grant: ProjectGrant; revoke: GrantRevoke | null; revoked_at: number | null; superseded_at: number | null }[];
  /** The project's live delegation, as the hub says. */
  delegation: string | null;
  /** The grant names another delegation than the live one: sign again. */
  stale: boolean;
  /** The grant expires within two weeks: renew it. */
  renew: boolean;
  workspaces: number;
  offer: boolean;
}
