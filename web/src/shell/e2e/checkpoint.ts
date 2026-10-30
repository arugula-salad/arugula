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
  const gone = await removed(trust, certs, revocations);
  if (!gone.size) return certs;
  // A removal never stamped counts as seen first; a certificate never
  // stamped, as seen last: a missing stamp never lets one in.
  const at = (sig: string) => (Object.hasOwn(seen.at, sig) ? seen.at[sig] : Number.MAX_SAFE_INTEGER);
  const removedAt = (device: string) => {
    const ts = revocations.filter((r) => r.account === trust.account && r.device === device).map((r) => (Object.hasOwn(seen.at, r.sig) ? seen.at[r.sig] : 0));
    return ts.length ? Math.min(...ts) : null;
  };
  return certs.filter((c) => {
    if (c.account !== trust.account || !gone.has(c.approver)) return true;
    const t = removedAt(c.approver);
    return !(t !== null && at(c.sig) > t);
  });
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
  const certs = await dropLate(trust, fresh.certs, revocations, seen);
  const gone = await removed(trust, old.certs, old.revocations);
  if (!gone.size) return { certs, revocations, refused: false };
  const after = await evaluate(trust, certs, revocations);
  if ([...gone].some((id) => after.has(id))) return { certs: await dropLate(trust, old.certs, revocations, seen), revocations, refused: true };
  return { certs, revocations, refused: false };
}
