// Team rosters (M19): the browser's copy of `arugula_e2e::team`. Owners'
// browsers sign each new version; the same rules as daemons decide which
// follow.

import { approves, browserChained, evaluate, fromApp, hex, unhex, type Cert, type Revocation } from "./cert.ts";
import { signText, type DeviceKeys } from "./keys.ts";
import { newKeyPair } from "./noise.ts";

export type TeamRole = "owner" | "editor" | "viewer";

export interface Member {
  account: string;
  root: string;
  role: TeamRole;
  name: string;
}

export interface Roster {
  v: number;
  team: string;
  name: string;
  version: number;
  at: number;
  members: Member[];
  /** Invites already redeemed (v2), so each works once. */
  spent?: Spent[];
  /** The presigned invite this version redeems (v2). */
  redeem?: Redeem;
  by: string;
  sig: string;
}

/** An invite an owner's device signed ahead of time (`team::Invite`):
 * whoever holds the one-time key's private half adds themselves, once. */
export interface Invite {
  team: string;
  role: TeamRole;
  expires: number;
  key: string;
  by: string;
  sig: string;
}

/** An *Ask me first* invite (`team::AskInvite`, #762): signed by an
 * owner's device like a presigned one, with its own one-time key, but an
 * owner still says yes to each person who uses it. */
export interface AskInvite {
  team: string;
  role: TeamRole;
  made: number;
  expires: number;
  key: string;
  by: string;
  sig: string;
}

export interface Spent {
  key: string;
  expires: number;
}

export interface Redeem {
  invite: Invite;
  proof: string;
}

export interface TeamPin {
  team: string;
  founder: string;
  founder_root: string;
}

export type AccountCerts = Record<string, [Cert[], Revocation[]]>;

export function rosterBody(r: Roster): string {
  // Frozen (#504): this and the signed bodies below; see crates/e2e/src/frozen.rs.
  let b = `illogical team v${r.v}\nteam ${r.team}\nname ${r.name}\nversion ${r.version}\nat ${r.at}\n`;
  for (const m of r.members) b += `member ${m.account} ${m.root} ${m.role} ${m.name}\n`;
  for (const x of r.spent ?? []) b += `spent ${x.key} ${x.expires}\n`;
  if (r.redeem) b += `redeem ${r.redeem.invite.key} ${r.redeem.proof}\n`;
  return b + `by ${r.by}\n`;
}

export const inviteBody = (i: Invite) =>
  `illogical team invite v1\nteam ${i.team}\nrole ${i.role}\nexpires ${i.expires}\nkey ${i.key}\nby ${i.by}\n`;

/** What the one-time key signs: this member at this version. */
export const redeemBody = (i: Invite, version: number, m: Member) =>
  `illogical team redeem v1\nteam ${i.team}\nversion ${version}\nmember ${m.account} ${m.root} ${m.role} ${m.name}\nkey ${i.key}\n`;

export const askInviteBody = (i: AskInvite) =>
  `arugula team ask invite v1\nteam ${i.team}\nrole ${i.role}\nmade ${i.made}\nexpires ${i.expires}\nkey ${i.key}\nby ${i.by}\n`;

/** What the link's one-time key signs for whoever used it: their account
 * and its root (`AskInvite::ask_body`). */
export const askBody = (i: Pick<AskInvite, "team" | "key">, account: string, root: string) =>
  `arugula team ask v1\nteam ${i.team}\naccount ${account}\nroot ${root}\nkey ${i.key}\n`;

// An Ed25519 private key as PKCS#8 is this prefix and the 32-byte seed, so
// the link carries only the seed.
const PKCS8_ED25519 = "302e020100300506032b657004220420";

/** A one-time invite key: its seed (for the link's fragment only) and
 * public half (the invite's `key`). */
export async function newInviteKey(): Promise<{ seed: string; key: string }> {
  const k = await newKeyPair("Ed25519", true, ["sign", "verify"]);
  const pkcs8 = hex(new Uint8Array(await crypto.subtle.exportKey("pkcs8", k.privateKey)));
  return { seed: pkcs8.slice(PKCS8_ED25519.length), key: hex(new Uint8Array(await crypto.subtle.exportKey("raw", k.publicKey))) };
}

/** The one-time key from a link's seed: its public half, and a signer. */
export async function inviteKey(seed: string): Promise<{ key: string; sign: (text: string) => Promise<string> }> {
  if (!/^[0-9a-f]{64}$/.test(seed)) throw new Error("that invite link is cut short");
  const priv = await crypto.subtle.importKey("pkcs8", unhex(PKCS8_ED25519 + seed), { name: "Ed25519" }, true, ["sign"]);
  const jwk = await crypto.subtle.exportKey("jwk", priv);
  const key = hex(Uint8Array.from(atob((jwk.x ?? "").replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)));
  return { key, sign: async (text) => hex(new Uint8Array(await crypto.subtle.sign("Ed25519", priv, new TextEncoder().encode(text)))) };
}

export async function signInvite(i: Omit<Invite, "by" | "sig">, keys: DeviceKeys): Promise<Invite> {
  const out: Invite = { ...i, by: keys.id, sig: "" };
  out.sig = await signText(keys, inviteBody(out));
  return out;
}

export async function signAskInvite(i: Omit<AskInvite, "by" | "sig">, keys: DeviceKeys): Promise<AskInvite> {
  const out: AskInvite = { ...i, by: keys.id, sig: "" };
  out.sig = await signText(keys, askInviteBody(out));
  return out;
}

/** A request to join that an owner's page may show (#762): the invite was
 * signed by a device of one of `r`'s owners, for this team, and the link's
 * one-time key vouches for this account and root. Says which owner made
 * it, or null when it doesn't check out (control made it up, or moved
 * someone's proof to another account). */
export async function askOk(
  r: Roster,
  certs: AccountCerts,
  req: { account: string; root: string; invite?: AskInvite | null; proof?: string | null },
): Promise<string | null> {
  const i = req.invite;
  if (!i || typeof req.proof !== "string" || i.team !== r.team || !ROLES.includes(i.role) || !isCount(i.made) || !isCount(i.expires)) return null;
  if (typeof i.key !== "string" || typeof i.by !== "string" || typeof i.sig !== "string") return null;
  if (!(await verify(i.key, askBody(i, req.account, req.root), req.proof))) return null;
  for (const o of r.members.filter((m) => m.role === "owner")) {
    const [c, rv] = certs[o.account] ?? [[], []];
    const d = (await evaluate({ account: o.account, root: o.root }, c, rv)).get(i.by);
    // One for an owner or an editor, only from a browser all the way back.
    const ok = !!d && (await raiseOk(i.role === "viewer" ? null : i.role, d, () => browserChained({ account: o.account, root: o.root }, c, i.by)));
    if (d && approves(d.kind) && ok && (await verify(d.sign, askInviteBody(i), i.sig))) return o.account;
  }
  return null;
}

/** The version after `prev` that adds `me` with a presigned invite, signed
 * by the one-time key (`Roster::sign_redeem`). */
export async function redeemInvite(prev: Roster, invite: Invite, seed: string, me: Omit<Member, "role">): Promise<Roster> {
  const k = await inviteKey(seed);
  if (k.key !== invite.key) throw new Error("that invite link doesn't match its invite");
  const m: Member = { ...me, role: invite.role };
  const version = prev.version + 1;
  const proof = await k.sign(redeemBody(invite, version, m));
  const out: Omit<Roster, "by" | "sig"> = {
    v: 2,
    team: prev.team,
    name: prev.name,
    version,
    // Never before the version it follows, whatever this clock says.
    at: Math.max(Date.now(), prev.at),
    members: [...prev.members, m],
    spent: [...(prev.spent ?? []), { key: invite.key, expires: invite.expires }],
    redeem: { invite, proof },
  };
  return signRedeem(out, seed);
}

/** Sign a version that redeems a presigned invite with its one-time key. */
export async function signRedeem(r: Omit<Roster, "by" | "sig">, seed: string): Promise<Roster> {
  const k = await inviteKey(seed);
  const out: Roster = { ...r, by: k.key, sig: "" };
  out.sig = await k.sign(rosterBody(out));
  return out;
}

/** What the approving device signs to put a joining daemon in a team
 * (#100): `TeamPin::join_body`. */
export const teamJoinBody = (daemon: string, p: TeamPin) =>
  `illogical team join v1\ndaemon ${daemon}\nteam ${p.team}\nfounder ${p.founder}\nfounder_root ${p.founder_root}\n`;

/** What a device of a machine's own account signs to move it into a team,
 * between teams, or back to the account (#100): `Move::body`. */
export const moveBody = (daemon: string, p: TeamPin | null, at: number) =>
  `illogical machine move v1\ndaemon ${daemon}\nteam ${p?.team ?? "-"}\nfounder ${p?.founder ?? "-"}\nfounder_root ${p?.founder_root ?? "-"}\nat ${at}\n`;

/** A name in a roster: no spaces or control characters. */
export const word = (s: string) => s.replace(/[\s\u0000-\u001f\u007f-\u009f]+/g, "-").slice(0, 120) || "someone";

export async function signRoster(r: Omit<Roster, "by" | "sig">, keys: DeviceKeys): Promise<Roster> {
  const out: Roster = { ...r, by: keys.id, sig: "" };
  out.sig = await signText(keys, rosterBody(out));
  return out;
}

async function verify(signHex: string, msg: string, sigHex: string): Promise<boolean> {
  try {
    const un = (h: string) => Uint8Array.from(h.match(/../g) ?? [], (x) => parseInt(x, 16));
    const key = await crypto.subtle.importKey("raw", un(signHex), { name: "Ed25519" }, false, ["verify"]);
    return await crypto.subtle.verify("Ed25519", key, un(sigHex), new TextEncoder().encode(msg));
  } catch {
    return false;
  }
}

// `Roster::well_formed`, so a browser takes exactly the rosters daemons do.
const isWord = (s: unknown) =>
  typeof s === "string" && s.length > 0 && new TextEncoder().encode(s).length <= 120 && !/[\p{White_Space}\p{Cc}]/u.test(s);
const isCount = (n: unknown) => typeof n === "number" && Number.isSafeInteger(n) && n >= 0;
const ROLES: unknown[] = ["owner", "editor", "viewer"];

function wellFormed(r: Roster): boolean {
  if (r.spent !== undefined && !Array.isArray(r.spent)) return false;
  const spent = r.spent ?? [];
  if ( !Array.isArray(r.members) || !isCount(r.version) || !isCount(r.at)) return false;
  if (typeof r.by !== "string" || typeof r.sig !== "string") return false;
  const shape =
    r.v === 1
      ? !spent.length && r.redeem == null
      : r.v === 2 && spent.every((x) => isWord(x?.key) && isCount(x?.expires));
  if (r.redeem != null) {
    const i = r.redeem.invite;
    if (!i || typeof r.redeem.proof !== "string" || !isWord(i.team) || !ROLES.includes(i.role) || !isCount(i.expires)) return false;
    if (typeof i.key !== "string" || typeof i.by !== "string" || typeof i.sig !== "string") return false;
  }
  return (
    shape &&
    isWord(r.team) &&
    typeof r.name === "string" &&
    r.name.length > 0 &&
    new TextEncoder().encode(r.name).length <= 80 &&
    !/\p{Cc}/u.test(r.name) &&
    r.members.every((m) => isWord(m?.account) && isWord(m?.root) && isWord(m?.name) && ROLES.includes(m?.role)) &&
    r.members.some((m) => m.role === "owner")
  );
}

const sameMember = (a: Member, b: Member) => a.account === b.account && a.root === b.root && a.role === b.role && a.name === b.name;

const RANK: Record<TeamRole, number> = { owner: 2, editor: 1, viewer: 0 };

/** The highest role `r` raises anyone to since `prev` (`Roster::raised`):
 * `owner` for an owner, or for `signer`'s own entry moved to another root;
 * `editor` when it makes editors only; else null. */
export function raised(r: Roster, prev: Roster | null, signer: string): TeamRole | null {
  let top: TeamRole | null = null;
  for (const m of r.members) {
    const was = prev?.members.find((p) => p.account === m.account);
    let raise: TeamRole | null = null;
    // The signer's own entry: an owner's raise if it now names another root.
    if (m.account === signer) raise = was && was.root !== m.root ? "owner" : null;
    else if (m.role !== "viewer" && !(was && was.root === m.root && RANK[was.role] >= RANK[m.role])) raise = m.role;
    if (raise && (!top || RANK[raise] > RANK[top])) top = raise;
  }
  return top;
}

/** Whether `r` raises anyone (`Roster::raises`). */
export const raises = (r: Roster, prev: Roster | null, signer: string) => raised(r, prev, signer) !== null;

/** Whether `signer` may sign a paper raising someone to `role`
 * (`team::raise_ok`): an owner only from a browser all the way back, an
 * editor from that or the desktop app's device, never a CLI. */
export async function raiseOk(role: TeamRole | null, signer: Cert, chained: () => Promise<boolean>): Promise<boolean> {
  if (!role || role === "viewer") return true;
  if (role === "editor" && fromApp(signer)) return true;
  return chained();
}

/** Whether `r` may follow `prev` (or start the team, as `pin` says). */
export async function follows(r: Roster, prev: Roster | null, pin: TeamPin, certs: AccountCerts): Promise<boolean> {
  const spent = r.spent ?? [];
  if (!wellFormed(r) || r.team !== pin.team) return false;
  if (prev && r.version <= prev.version) return false;
  // And may raise someone to `raised` (`raise_ok`): an owner only from a
  // browser all the way back, an editor from that or the desktop app.
  const signedBy = async (account: string, root: string, device: string, body: string, sig: string, raised: TeamRole | null) => {
    const [c, rv] = certs[account] ?? [[], []];
    const d = (await evaluate({ account, root }, c, rv)).get(device);
    return !!d && d.kind !== "daemon" && (await verify(d.sign, body, sig)) && (await raiseOk(raised, d, () => browserChained({ account, root }, c, device)));
  };

  const body = rosterBody(r);
  const signers: Pick<Member, "account" | "root">[] = prev
    ? prev.members.filter((m) => m.role === "owner")
    : [{ account: pin.founder, root: pin.founder_root }];
  for (const m of signers) if (await signedBy(m.account, m.root, r.by, body, r.sig, raised(r, prev, m.account))) return !r.redeem;
  // A presigned invite (`Roster::follows`): the version before, plus the
  // invitee at the end, the invite spent, signed by the one-time key.
  if (!prev || !r.redeem) return false;
  const inv = r.redeem.invite;
  const n = prev.members.length;
  const m = r.members[n];
  const was = prev.spent ?? [];
  if (
    !m ||
    r.version !== prev.version + 1 ||
    r.name !== prev.name ||
    r.members.length !== n + 1 ||
    !prev.members.every((p, i) => sameMember(p, r.members[i])) ||
    prev.members.some((p) => p.account === m.account) ||
    spent.length !== was.length + 1 ||
    !was.every((x, i) => x.key === spent[i].key && x.expires === spent[i].expires) ||
    spent[was.length].key !== inv.key ||
    spent[was.length].expires !== inv.expires ||
    was.some((x) => x.key === inv.key) ||
    inv.team !== r.team ||
    inv.role === "owner" ||
    m.role !== inv.role ||
    r.at > inv.expires ||
    r.at < prev.at ||
    r.by !== inv.key
  )
    return false;
  let byOwner = false;
  for (const o of prev.members.filter((x) => x.role === "owner")) byOwner ||= await signedBy(o.account, o.root, inv.by, inviteBody(inv), inv.sig, inv.role === "viewer" ? null : inv.role);
  return byOwner && (await verify(inv.key, redeemBody(inv, r.version, m), r.redeem.proof)) && (await verify(inv.key, body, r.sig));
}

/** Whether two rosters are the same version, signed alike. */
export const sameRoster = (a: Roster, b: Roster) => rosterBody(a) === rosterBody(b) && a.sig === b.sig;

/** Each account's revocations made after `at` left out: the devices as
 * they stood when something dated `at` was signed. */
export function asOf(certs: AccountCerts, at: number): AccountCerts {
  const out: AccountCerts = {};
  for (const [a, [c, r]] of Object.entries(certs)) out[a] = [c, r.filter((x) => x.at <= at)];
  return out;
}

/** Where a team's roster stands after `history` (oldest first), from
 * `held`: the version this browser last checked, or none for a first look
 * (`checkpoint::advance`). Older versions are passed over; one at
 * `held`'s version must be `held`; each newer one must be the very next
 * (from 1 on a first look) and follow the one before. The first that
 * doesn't stops the walk: where it stood, and that version.
 *
 * With `lateOk`, a version before the last one may also have been signed
 * by a device removed since, if that device counted as of the version's
 * `at` (which isn't before the version it follows): a real team's history
 * isn't refused for an owner's lost laptop. Those versions come back
 * third, to show. The last version is always checked as of now. Rust's
 * `advance` has no such allowance. */
export async function advance(
  held: Roster | null,
  history: Roster[],
  pin: TeamPin,
  certs: AccountCerts,
  lateOk = false,
): Promise<[Roster | null, number | null, number[]]> {
  let cur = held;
  const late: number[] = [];
  for (const [i, r] of history.entries()) {
    if (cur && r.version < cur.version) continue;
    if (cur && r.version === cur.version) {
      if (sameRoster(r, cur)) continue;
      return [cur, r.version, late];
    }
    if (r.version !== (cur ? cur.version + 1 : 1)) return [cur, r.version, late];
    if (!(await follows(r, cur, pin, certs))) {
      const last = i === history.length - 1;
      if (!lateOk || last || (cur && r.at < cur.at) || !(await follows(r, cur, pin, asOf(certs, r.at)))) return [cur, r.version, late];
      late.push(r.version);
    }
    cur = r;
  }
  return [cur, null, late];
}

/** The members whose root `to` names differently from `from`: another
 * key for that account, which a person may want to check. */
export function rootChanges(from: Roster, to: Roster): { account: string; name: string; from: string; to: string }[] {
  const out: { account: string; name: string; from: string; to: string }[] = [];
  for (const m of to.members) {
    const was = from.members.find((x) => x.account === m.account);
    if (was && was.root !== m.root) out.push({ account: m.account, name: m.name, from: was.root, to: m.root });
  }
  return out;
}

/** Whether a team is locked, signed by an owner's device (`team::TeamLock`). */
export interface TeamLock {
  team: string;
  locked: boolean;
  version: number;
  at: number;
  by: string;
  sig: string;
}

export const lockBody = (l: TeamLock) =>
  `arugula team lock v1\nteam ${l.team}\nlocked ${l.locked ? "yes" : "no"}\nversion ${l.version}\nat ${l.at}\nby ${l.by}\n`;

export async function signLock(l: Omit<TeamLock, "by" | "sig">, keys: DeviceKeys): Promise<TeamLock> {
  const out: TeamLock = { ...l, by: keys.id, sig: "" };
  out.sig = await signText(keys, lockBody(out));
  return out;
}
