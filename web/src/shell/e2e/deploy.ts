// A deploy's approval (M218): S37's work approval over a deploy's
// operation, signed by this browser's device key. The same bytes as
// `arugula_e2e::deploy` in Rust; the first lines are pinned there
// (`frozen.rs`).
//
//   arugula deploy op v1               arugula deploy approval v1
//   project <prj id>                   deploy <dep id>
//   environment <env id>               op <operation hash, hex>
//   name <environment's name>          account <approver's account>
//   plan <sha-256 hex>                 decision <once|deny>
//   source <commit|artifact> <ref>     expires <ms>
//   where <place>                      by <device id>
//   planned <planned event's hash>
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
  by: string;
  sig: string;
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

export function approvalBody(a: Omit<Approval, "sig">): string {
  return `arugula deploy approval v1\ndeploy ${a.deploy}\nop ${a.op}\naccount ${a.account}\ndecision ${a.decision}\nexpires ${a.expires}\nby ${a.by}\n`;
}

/** Sign an approval with an Ed25519 private key. */
export async function signApproval(a: Omit<Approval, "sig">, key: CryptoKey): Promise<Approval> {
  const sig = await crypto.subtle.sign("Ed25519", key, new TextEncoder().encode(approvalBody(a)));
  return { ...a, sig: hex(new Uint8Array(sig)) };
}
