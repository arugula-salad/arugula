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

import { hex } from "./cert.ts";

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
