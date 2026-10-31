// What the page keeps of what it already checked, as machines do
// (`arugula_e2e::checkpoint`): removals only add up, a certificate first
// seen after its approver's removal doesn't count, and certificates that
// would let a removed device back in are refused. e2e-interop.ts checks
// the two agree.

import { evaluate, type Cert, type Revocation } from "./cert.ts";

type Trust = { account: string; root: string };

/** In which round of looking this browser first saw each certificate and
 * removal, by its signature (`checkpoint::Seen`): rounds are counted, not
 * read off a clock. Round 0 is what was held before stamps were kept. */
export interface Seen {
  round: number;
  at: Record<string, number>;
}

/** The round a fresh look stamps what's new with (`Seen::next`). */
export const nextRound = (s: Seen) => s.round + 1;

/** After a look in `round`: count it if it stamped anything. */
export function settle(s: Seen, round: number) {
  if (Object.values(s.at).some((r) => r === round)) s.round = Math.max(s.round, round);
}

/** Whether `v` is a well-formed `Seen`: whole, non-negative rounds. */
export function isSeen(v: unknown): v is Seen {
  const ok = (n: unknown) => typeof n === "number" && Number.isSafeInteger(n) && n >= 0;
  if (!v || typeof v !== "object") return false;
  const s = v as Seen;
  return ok(s.round) && !!s.at && typeof s.at === "object" && !Array.isArray(s.at) && Object.values(s.at).every(ok);
}

/** `fresh`, and every one of `old` it leaves out. */
export function kept<T>(fresh: T[], old: T[], same: (a: T, b: T) => boolean): T[] {
  const out = [...fresh];
  for (const o of old) if (!out.some((x) => same(x, o))) out.push(o);
  return out;
}

const sameRevocation = (a: Revocation, b: Revocation) =>
  a.v === b.v && a.account === b.account && a.device === b.device && a.at === b.at && a.by === b.by && a.sig === b.sig;

/** Stamp each of `certs` and `revocations` not stamped yet with `round`. */
export function stamp(seen: Seen, certs: Cert[], revocations: Revocation[], round: number) {
  for (const sig of [...certs.map((c) => c.sig), ...revocations.map((r) => r.sig)]) if (!Object.hasOwn(seen.at, sig)) seen.at[sig] = round;
}

/** The devices held removed (`checkpoint::removed`). */
export async function removed(trust: Trust, certs: Cert[], revocations: Revocation[]): Promise<Set<string>> {
  const trusted = await evaluate(trust, certs, revocations);
  const out = new Set(revocations.filter((r) => r.account === trust.account && !trusted.has(r.device)).map((r) => r.device));
  for (;;) {
    const before = out.size;
    for (const c of certs) if (c.account === trust.account && !trusted.has(c.device) && out.has(c.approver)) out.add(c.device);
    if (out.size === before) return out;
  }
}

/** `certs` without those first seen after their approver's removal was
 * first seen (`checkpoint::drop_late`). */
export async function dropLate(trust: Trust, certs: Cert[], revocations: Revocation[], seen: Seen): Promise<Cert[]> {
  // A removal never stamped counts as seen first; a certificate never
  // stamped, as seen last: a missing stamp never lets one in.
  const at = (sig: string) => (Object.hasOwn(seen.at, sig) ? seen.at[sig] : Number.MAX_SAFE_INTEGER);
  const removedAt = (device: string) => {
    const ts = revocations.filter((r) => r.account === trust.account && r.device === device).map((r) => (Object.hasOwn(seen.at, r.sig) ? seen.at[r.sig] : 0));
    return ts.length ? Math.min(...ts) : null;
  };
  const late = (c: Cert) => {
    if (c.account !== trust.account) return false;
    const t = removedAt(c.approver);
    return t !== null && at(c.sig) > t;
  };
  // Only a certificate first seen after a removal naming its approver can
  // go: none, and the chain needn't be walked at all.
  if (!certs.some(late)) return certs;
  const gone = await removed(trust, certs, revocations);
  return certs.filter((c) => !(gone.has(c.approver) && late(c)));
}

/** An account's certificates and removals after a fresh look, over what
 * was held (`checkpoint::refresh_account`). */
export async function refreshAccount(
  trust: Trust,
  fresh: { certs: Cert[]; revocations: Revocation[] },
  old: { certs: Cert[]; revocations: Revocation[] },
  seen: Seen,
  round: number,
): Promise<{ certs: Cert[]; revocations: Revocation[]; refused: boolean }> {
  stamp(seen, old.certs, old.revocations, 0);
  const revocations = kept(fresh.revocations, old.revocations, sameRevocation);
  stamp(seen, fresh.certs, revocations, round);
  // Nothing new: the same certificates, and no removal not held. What's
  // held stands, as it was checked.
  const sigs = new Set(old.certs.map((c) => c.sig));
  if (revocations.length === old.revocations.length && fresh.certs.length === old.certs.length && fresh.certs.every((c) => sigs.has(c.sig)))
    return { certs: old.certs, revocations, refused: false };
  const certs = await dropLate(trust, fresh.certs, revocations, seen);
  // The devices `old` held removed are each named by a held removal, or
  // approved (by a certificate held or sent) by one of those, or by one
  // approved so (`removed`). If none of these suspects counts now, none of
  // those does, and the old view needn't be walked; if one does, it is.
  const suspects = new Set(old.revocations.filter((r) => r.account === trust.account).map((r) => r.device));
  if (!suspects.size) return { certs, revocations, refused: false };
  for (;;) {
    const before = suspects.size;
    for (const c of [...old.certs, ...certs]) if (c.account === trust.account && suspects.has(c.approver)) suspects.add(c.device);
    if (suspects.size === before) break;
  }
  const after = await evaluate(trust, certs, revocations);
  if (![...suspects].some((id) => after.has(id))) return { certs, revocations, refused: false };
  const gone = await removed(trust, old.certs, old.revocations);
  if ([...gone].some((id) => after.has(id))) return { certs: await dropLate(trust, old.certs, revocations, seen), revocations, refused: true };
  return { certs, revocations, refused: false };
}
