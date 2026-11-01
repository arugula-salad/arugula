// A deploy's approval (M218): S37's work approval over a deploy's
// operation. Since DK4 (aru-126) it's v2: no device signs it, and its
// signature is a user-verified passkey assertion whose challenge is SHA-256
// of these bytes. The same bytes as `arugula_e2e::deploy` in Rust; the first
// lines are pinned there (`frozen.rs`).
//
//   arugula deploy op v1               arugula deploy approval v2
//   project <prj id>                   deploy <dep id>
//   environment <env id>               op <operation hash, hex>
//   name <environment's name>          account <approver's account>
//   plan <sha-256 hex>                 decision <once|deny>
//   source <commit|artifact> <ref>     expires <ms>
//   where <place>
//   planned <planned event's hash>
//
// v1 (before DK4), which the platform no longer takes, named the signing
// device too (`by <device id>`, last) and carried its Ed25519 signature.
//
// The platform sends the operation's bytes and their hash, and the deploy's
// `planned` event as stored; the page hashes the bytes itself and reads
// them, and hashes the event again (M217's `arugula deploy event v1`), so
// what it shows (where, what, and who planned it) is what it signs.
//
// Shared (DK5, #386): control's `#deploy=` dialog and the platform page's
// deploy card both approve, so the checks and the passkey ceremony live
// here, where the platform page may import them. Nothing here holds a
// device key.

import { plannerText } from "./deploys.ts";

const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

export type Decision = "once" | "deny";

export interface Operation {
  project: string;
  environment: string;
  /** The environment's name: hashed in, so the name shown is the one signed. */
  name: string;
  plan: string;
  source: { kind: string; ref: string; where: string };
  planned: string;
}

export interface Approval {
  deploy: string;
  op: string;
  account: string;
  decision: Decision;
  expires: number;
  /** 2 (DK4) or, absent, 1. */
  v?: number;
  /** v1 only: the signing device's id. */
  by?: string;
  /** v1 only: its Ed25519 signature, hex. */
  sig?: string;
}

/** S37: an approval defaults to 24 hours. */
export const DEFAULT_LIFE_MS = 24 * 60 * 60 * 1000;

const OP =
  /^arugula deploy op v1\nproject (\S+)\nenvironment (\S+)\nname ([a-z0-9][a-z0-9-]{0,39})\nplan ([0-9a-f]{64})\nsource (commit|artifact) (\S+)\nwhere (\S+)\nplanned ([0-9a-f]{64})\n$/;

/** An operation's bytes, read: null unless they're exactly the format. */
export function parseOperation(body: string): Operation | null {
  const m = OP.exec(body);
  return m ? { project: m[1], environment: m[2], name: m[3], plan: m[4], source: { kind: m[5], ref: m[6], where: m[7] }, planned: m[8] } : null;
}

/** A deploy log's event as stored (M217): what its hash is over. */
export interface DeployEvent {
  environment: string;
  seq: number;
  deploy: string;
  kind: string;
  actor: string;
  at: number;
  detail: string;
  prev_hash: string;
  hash: string;
}

const b64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));

/** The bytes an event's hash is over (M217's `Event::canonical`). */
export function eventCanonical(e: DeployEvent): string {
  return `arugula deploy event v1\nprev ${e.prev_hash}\nenvironment ${e.environment}\nseq ${e.seq}\ndeploy ${e.deploy}\nkind ${e.kind}\nactor ${e.actor}\nat ${e.at}\ndetail ${b64(e.detail)}\n`;
}

/** Who an event's actor is, read from its hashed text: a person's
 * account, or an agent's owner and machine. */
export function actorOf(actor: string): { account: string; machine?: string } | null {
  const m = /^(?:person (\S+)|agent (\S+) (\S+))$/.exec(actor);
  return m ? (m[1] ? { account: m[1] } : { account: m[2], machine: m[3] }) : null;
}

export async function sha256Hex(text: string): Promise<string> {
  return hex(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text))));
}

/** The bytes an approval is over: a v2's (DK4), or a v1's, which names its
 * device. */
export function approvalBody(a: Omit<Approval, "sig">): string {
  const head = `deploy ${a.deploy}\nop ${a.op}\naccount ${a.account}\ndecision ${a.decision}\nexpires ${a.expires}\n`;
  return a.v === 2 ? `arugula deploy approval v2\n${head}` : `arugula deploy approval v1\n${head}by ${a.by}\n`;
}

/** The WebAuthn challenge an approval's passkey signs: SHA-256 of its bytes. */
export async function approvalChallenge(a: Omit<Approval, "sig">): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(approvalBody(a))));
}

/** A deploy's plan as an owner reads it (M220): its text, checked against
 * the digest the operation names, and what it says of a roll back. */
export interface CheckedPlan {
  text: string;
  /** Its driver, from its `driver` line. */
  driver: string;
  /** A roll back: the deploy it goes back to, and the image it runs again. */
  rollBack: { deploy: string; image: string | null } | null;
}

/** `text` is the plan whose SHA-256 is `digest` (the operation's `plan`),
 * in `arugula deploy plan v1`'s shape: what it says, or why not. Read only
 * from the hashed text, never from what the platform says beside it. */
export async function checkPlan(text: unknown, digest: string): Promise<CheckedPlan | string> {
  if (typeof text !== "string" || !text) return "the platform didn't show its plan";
  if ((await sha256Hex(text)) !== digest) return "its plan's text isn't the plan you'd sign";
  const lines = text.split("\n");
  if (lines[0] !== "arugula deploy plan v1" || !lines[1]?.startsWith("driver ")) return "its plan's text can't be read";
  const driver = lines[1].slice("driver ".length);
  const back = lines.find((l) => l.startsWith("roll back to "));
  const image = lines.find((l) => l.startsWith("back image "));
  return {
    text,
    driver,
    rollBack: back ? { deploy: back.slice("roll back to ".length), image: image ? image.slice("back image ".length) : null } : null,
  };
}

/** What a page reads of a deploy to check it before an approval: the
 * platform's `deploy_json`, either page's copy. */
export interface DeployToCheck {
  id: string;
  project: { id: string; name: string };
  environment: { id: string; name: string; driver: string };
  source: { kind: string; ref: string; where: string };
  plan: string;
  plan_text: string | null;
  planned_by: string;
  planned_by_name: string;
  agent: { machine: string; machine_name: string } | null;
  operation: { body: string; hash: string };
  planned_event?: DeployEvent | null;
}

/** What a page shows, read from hashed bytes: the operation, who planned
 * it from its `planned` event, and the plan's text. */
export interface Checked {
  op: Operation;
  planner: string;
  /** The plan's text, which hashes to the operation's `plan` (M220). */
  plan: CheckedPlan;
}

/** The deploy's operation and its `planned` event check out: the bytes hash
 * to what the platform says and say what the deploy says, and the event
 * hashes to the one the operation names. What to show, or why not: a page
 * never offers Approve over bytes it couldn't check. */
export async function checkDeploy(d: DeployToCheck): Promise<Checked | string> {
  const op = parseOperation(d.operation.body);
  if (!op || (await sha256Hex(d.operation.body)) !== d.operation.hash) return "its operation's bytes don't match their hash";
  if (
    op.project !== d.project.id ||
    op.environment !== d.environment.id ||
    op.name !== d.environment.name ||
    op.plan !== d.plan ||
    op.source.kind !== d.source.kind ||
    op.source.ref !== d.source.ref ||
    op.source.where !== d.source.where
  ) {
    return "its operation says something other than the deploy";
  }
  const e = d.planned_event;
  if (!e || e.kind !== "planned" || e.deploy !== d.id || e.environment !== op.environment) return "its plan isn't in the log";
  if ((await sha256Hex(eventCanonical(e))) !== e.hash || e.hash !== op.planned) return "its plan's event doesn't match the operation";
  let detail: { plan?: string; source?: { kind?: string; ref?: string; where?: string } };
  try {
    detail = JSON.parse(e.detail);
  } catch {
    return "its plan's event can't be read";
  }
  if (detail.plan !== op.plan || detail.source?.kind !== op.source.kind || detail.source?.ref !== op.source.ref || detail.source?.where !== op.source.where) {
    return "its plan's event says another plan";
  }
  const who = actorOf(e.actor);
  if (!who || who.account !== d.planned_by || (who.machine ?? null) !== (d.agent?.machine ?? null)) return "who planned it doesn't match its log";
  const name = d.planned_by_name || "someone";
  // What the driver will do, as the operation's plan digest binds it: the
  // text must hash to it, or it isn't what's being signed (M220).
  const plan = await checkPlan(d.plan_text, op.plan);
  if (typeof plan === "string") return plan;
  if (plan.driver !== d.environment.driver) return "its plan names another driver than its environment's";
  return { op, plan, planner: plannerText(name, who.machine ? { machine: who.machine, machine_name: d.agent?.machine_name } : null) };
}

const unb64u = (s: string) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
const b64u = (b: ArrayBuffer) =>
  btoa(String.fromCharCode(...new Uint8Array(b)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");

/** What the platform takes of a passkey's assertion: the approval's
 * signature, checked against the account's passkey log. */
export interface Assertion {
  id: string;
  clientDataJSON: string;
  authenticatorData: string;
  signature: string;
}

/** The browser can't use control's passkeys on this page: its message
 * says why, beside the way round (control's page). */
export class NotHere extends Error {}

/** Faster than this, a refusal was the browser's or a password manager's,
 * not the person's (S46 q1). */
const NOBODY_ANSWERS_MS = 1500;

/** `rpId` is this page's host or a parent of it: no related origins needed. */
export const sameSite = (rpId: string, host: string = globalThis.location?.hostname ?? "") => host === rpId || host.endsWith(`.${rpId}`);

/** Ask for a passkey bound to `rpId` (control's host, aru-078), user
 * verification required, its challenge SHA-256 of the approval's own bytes:
 * its assertion. `allow`: the account's own passkeys' credential ids, so a
 * password manager doesn't offer another account's. From a page on another
 * host than `rpId` it's a related-origin request (control's
 * `/.well-known/webauthn` lists the platform page); where the browser or
 * the provider can't make one, it throws {@link NotHere}. */
export async function passkeyOver(a: Omit<Approval, "sig">, rpId: string, allow: string[] = []): Promise<Assertion> {
  const challenge = await approvalChallenge(a);
  const asked = Date.now();
  let cred: PublicKeyCredential | null;
  try {
    cred = (await navigator.credentials.get({
      publicKey: {
        challenge,
        ...(allow.length ? { allowCredentials: allow.map((id) => ({ type: "public-key" as const, id: unb64u(id) })) } : {}),
        rpId,
        userVerification: "required",
        timeout: 5 * 60 * 1000,
      },
    })) as PublicKeyCredential | null;
  } catch (e) {
    throw refusedHow((e as { name?: string }).name, Date.now() - asked, sameSite(rpId)) ?? e;
  }
  if (!cred) throw new Error("no passkey chosen");
  const r = cred.response as AuthenticatorAssertionResponse;
  return { id: b64u(cred.rawId), clientDataJSON: b64u(r.clientDataJSON), authenticatorData: b64u(r.authenticatorData), signature: b64u(r.signature) };
}

/** What a refused `navigator.credentials.get` means, by its error's name and
 * how soon it came: {@link NotHere} when the RP refused this origin (no
 * related origins in this browser or provider: the 1Password extension,
 * Firefox before 152, or control's list doesn't name this page), or a
 * refusal faster than a person could have answered from a page that needs
 * related origins; an error to show when the person turned it down; else
 * null (the browser's own error stands). */
export function refusedHow(name: string | undefined, afterMs: number, same: boolean): Error | null {
  if (name === "SecurityError") return new NotHere("this browser or password manager can't use Control's passkeys on this page");
  if (name === "NotAllowedError" && afterMs < NOBODY_ANSWERS_MS && !same) {
    return new NotHere("the passkey prompt was turned down before anyone could answer it, as a password manager without related origins does");
  }
  if (name === "NotAllowedError") return new Error("no passkey was used: try again when you're ready");
  return null;
}

/** Why this page can't ask for control's passkeys at all, found before
 * trying, or null when it may try: no WebAuthn here (the Linux app's
 * window: WebKitGTK has none), or a browser that says it has no related
 * origins while the page needs them. */
export async function passkeysNotHere(rpId: string): Promise<string | null> {
  const P = (globalThis as { PublicKeyCredential?: { getClientCapabilities?: () => Promise<Record<string, boolean>> } }).PublicKeyCredential;
  if (!P || !globalThis.navigator?.credentials?.get) return "this window has no passkeys (WebAuthn)";
  if (sameSite(rpId)) return null;
  const caps = await P.getClientCapabilities?.().catch(() => null);
  if (caps && caps.relatedOrigins === false) return "this browser can't use Control's passkeys on this page (no related origins)";
  return null;
}
