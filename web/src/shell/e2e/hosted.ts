// Hosted machines (M178): the browser's copy of `arugula_e2e::hosted`,
// byte for byte (fixed line formats, not JSON). An owner's device signs a
// project delegation naming control's hosted key; control signs each
// hosted machine's certificate with that key; this browser accepts such a
// machine only for that project, checked against the roster or the
// account it holds itself, never as one of its account's devices.

import { sha256 } from "./noise.ts";
import { evaluate, hex, unhex, verify, deviceId, type Cert, type Revocation } from "./cert.ts";
import { signText, type DeviceKeys } from "./keys.ts";
import type { AccountCerts, Roster, TeamRole } from "./team.ts";

export { HOSTED_LABEL } from "../../shared/hosted.ts";

export interface Delegation {
  project: string;
  /** `team:<id>` or `account:<id>`. */
  owner: string;
  /** Control's hosted key, Ed25519 hex. */
  key: string;
  at: number;
  by: string;
  sig: string;
}

export interface DelegationRevoke {
  project: string;
  delegation: string;
  at: number;
  by: string;
  sig: string;
}

/** Control's word that a delegation ended, signed with the key it names. */
export interface DelegationClosed {
  project: string;
  delegation: string;
  at: number;
  sig: string;
}

export interface HostedCert {
  project: string;
  owner: string;
  workspace: string;
  delegation: string;
  device: string;
  name: string;
  noise: string;
  sign: string;
  created: number;
  sig: string;
}

/** What the directory lists beside a hosted machine. */
export interface HostedProof {
  cert: HostedCert;
  delegation: Delegation;
  revocations: DelegationRevoke[];
  /** Absent from a control from before. */
  closed?: DelegationClosed[];
}

const enc = new TextEncoder();
const token = (s: string) => /^[A-Za-z0-9_-]{1,64}$/.test(s);
// eslint-disable-next-line no-control-regex
const plain = (s: string) => s.length > 0 && [...s].length <= 64 && !/[\u0000-\u001f\u007f-\u009f]/.test(s);
// Hex on these papers reads in either case, as Rust's does; what's signed is
// the text as given.
const isKey = (s: string) => /^[0-9a-fA-F]{64}$/.test(s);
const low = (s: string) => s.toLowerCase();

const text = (v: unknown) => typeof v === "string";
/** A time, as `arugula_e2e::hosted::read_time`: a JSON number whose value is a
 * non-negative integer up to 2^53 - 1, however it's written. */
const time = (v: unknown) => Number.isSafeInteger(v) && (v as number) >= 0;
const isRevoke = (v: unknown): v is DelegationRevoke => {
  const r = v as DelegationRevoke | null;
  return !!r && typeof r === "object" && text(r.project) && text(r.delegation) && time(r.at) && text(r.by) && text(r.sig);
};
export const isClosed = (v: unknown): v is DelegationClosed => {
  const c = v as DelegationClosed | null;
  return !!c && typeof c === "object" && text(c.project) && text(c.delegation) && time(c.at) && text(c.sig);
};
/** The entries of a list that read as `is`, each on its own, as
 * `arugula_e2e::hosted::entries`: what isn't a list is none. */
const entries = <T,>(list: unknown, is: (v: unknown) => v is T): T[] => (Array.isArray(list) ? list.filter(is) : []);

export const delegationBody = (d: Delegation) =>
  `arugula project delegation v1\nproject ${d.project}\nowner ${d.owner}\nkey ${d.key}\nat ${d.at}\nby ${d.by}\n`;

export const revokeBody = (r: DelegationRevoke) =>
  `arugula project delegation revoke v1\nproject ${r.project}\ndelegation ${r.delegation}\nat ${r.at}\nby ${r.by}\n`;

export const closedBody = (c: DelegationClosed) =>
  `arugula project delegation closed v1\nproject ${c.project}\ndelegation ${c.delegation}\nat ${c.at}\n`;

export const hostedBody = (c: HostedCert) =>
  `arugula hosted machine v1\nproject ${c.project}\nowner ${c.owner}\nworkspace ${c.workspace}\ndelegation ${c.delegation}\n` +
  `device ${c.device}\nname ${c.name}\nnoise ${c.noise}\nsign ${c.sign}\ncreated ${c.created}\n`;

/** A delegation's id: the first 16 bytes of SHA-256 over its body. */
export async function delegationId(d: Delegation): Promise<string> {
  return hex((await sha256(enc.encode(delegationBody(d)))).subarray(0, 16));
}

/** `team:<id>` or `account:<id>`, or null. */
export function parseOwner(s: string): { team: string } | { account: string } | null {
  const i = s.indexOf(":");
  const [kind, id] = [s.slice(0, i), s.slice(i + 1)];
  if (i < 0 || !token(id)) return null;
  return kind === "team" ? { team: id } : kind === "account" ? { account: id } : null;
}

/** An owner's device delegates `project` to control's `key`. */
export async function signDelegation(project: string, owner: string, key: string, keys: DeviceKeys): Promise<Delegation> {
  const d: Delegation = { project, owner, key, at: Date.now(), by: keys.id, sig: "" };
  return { ...d, sig: await signText(keys, delegationBody(d)) };
}

/** An owner's device takes a delegation back. */
export async function signRevoke(d: Delegation, keys: DeviceKeys): Promise<DelegationRevoke> {
  const r: DelegationRevoke = { project: d.project, delegation: await delegationId(d), at: Date.now(), by: keys.id, sig: "" };
  return { ...r, sig: await signText(keys, revokeBody(r)) };
}

/** Who may sign for a project, as this browser holds them: a team's
 * roster and members' certificates, or its own account's chain. */
export type Owners =
  | { team: Roster; certs: AccountCerts }
  | { account: { account: string; root: string }; certs: Cert[]; revocations: Revocation[] };

const ownerOf = (o: Owners) => ("team" in o ? `team:${o.team.team}` : `account:${o.account.account}`);

/** `sig` is over `body` by `device`, a browser, CLI or app of one of the
 * project's owners, trusted now. */
async function maySign(o: Owners, owner: string, device: string, body: string, sig: string): Promise<boolean> {
  if (owner !== ownerOf(o) || !parseOwner(owner)) return false;
  const ok = async (c: Cert | undefined) => !!c && (c.kind === "browser" || c.kind === "cli" || c.kind === "app") && (await verify(low(c.sign), body, low(sig)));
  if ("team" in o) {
    for (const m of o.team.members.filter((m) => m.role === "owner")) {
      const [c, rv] = o.certs[m.account] ?? [[], []];
      if (await ok((await evaluate({ account: m.account, root: m.root }, c, rv)).get(device))) return true;
    }
    return false;
  }
  return ok((await evaluate(o.account, o.certs, o.revocations)).get(device));
}

/** `x` ends `d`, whose id is `id`, signed with the key `d` names. */
const closes = async (x: DelegationClosed, d: Delegation, id: string) =>
  x.project === d.project && x.delegation === id && (await verify(low(d.key), closedBody(x), low(x.sig)));

export type HostedRefusal = "form" | "wrong_project" | "not_in_project" | "unsigned" | "bad_signature" | "revoked";

/** Whether `me` (an account) accepts the machine, for its project only:
 * `Hosted::accept`'s rules. */
export async function acceptHosted(
  me: string,
  p: HostedProof,
  o: Owners,
): Promise<{ ok: true; project: string; workspace: string; role: TeamRole } | { ok: false; why: HostedRefusal }> {
  const { cert: c, delegation: d } = p;
  const no = (why: HostedRefusal) => ({ ok: false as const, why });
  const id = await delegationId(d);
  const formed =
    token(c.project) && token(c.workspace) && token(c.delegation) && !!parseOwner(c.owner) && plain(c.name) &&
    time(c.created) && time(d.at) &&
    isKey(c.noise) && isKey(c.sign) && (await deviceId(unhex(low(c.noise)), unhex(low(c.sign)))) === c.device &&
    token(d.project) && !!parseOwner(d.owner) && isKey(d.key) && token(d.by) &&
    c.project === d.project && c.owner === d.owner && c.delegation === id;
  if (!formed) return no("form");
  if (d.owner !== ownerOf(o)) return no("wrong_project");
  const role: TeamRole | undefined =
    "team" in o ? o.team.members.find((m) => m.account === me)?.role : o.account.account === me ? "owner" : undefined;
  if (!role) return no("not_in_project");
  if (!(await maySign(o, d.owner, d.by, delegationBody(d), d.sig))) return no("unsigned");
  if (!(await verify(low(d.key), hostedBody(c), low(c.sig)))) return no("bad_signature");
  // The earliest of a revoke an owner signed, as this browser holds them now,
  // and control's word. Either that doesn't check out is ignored.
  let revoked = Infinity;
  for (const r of entries(p.revocations, isRevoke)) {
    if (r.project === d.project && r.delegation === id && (await maySign(o, d.owner, r.by, revokeBody(r), r.sig))) revoked = Math.min(revoked, r.at);
  }
  for (const x of entries(p.closed, isClosed)) {
    if (await closes(x, d, id)) revoked = Math.min(revoked, x.at);
  }
  if (c.created < d.at || revoked <= c.created) return no("revoked");
  return { ok: true, project: d.project, workspace: c.workspace, role };
}

/** What a client keeps of control's word that delegations ended. */
export interface ClosuresHeld {
  get(id: string): DelegationClosed | undefined;
  take(c: DelegationClosed): void;
}

/** `acceptHosted`, by a client that keeps what it hears, as
 * `arugula_e2e::hosted::accept_holding`: word that checks out goes into
 * `held` when an owner signed the delegation, and what `held` has of it counts
 * as though control sent it again. */
export async function acceptHostedHolding(me: string, p: HostedProof, o: Owners, held: ClosuresHeld) {
  const d = p.delegation;
  const id = await delegationId(d);
  const heard = entries(p.closed, isClosed);
  const checked = await Promise.all(heard.map((x) => closes(x, d, id)));
  if (checked.some(Boolean) && isKey(d.key) && token(d.project) && !!parseOwner(d.owner) && token(d.by) && (await maySign(o, d.owner, d.by, delegationBody(d), d.sig))) {
    heard.filter((_, i) => checked[i]).forEach((x) => held.take(x));
  }
  const kept = held.get(id);
  return acceptHosted(me, { ...p, closed: kept ? [...heard, kept] : heard }, o);
}
