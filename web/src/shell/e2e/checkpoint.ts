// What the page keeps of what it already checked, as machines do
// (`arugula_e2e::checkpoint`): removals only add up, a certificate first
// seen after its approver's removal doesn't count, and certificates that
// would let a removed device back in are refused. e2e-interop.ts checks
// the two agree.

import { evaluate, type Cert, type Revocation } from "./cert.ts";

type Trust = { account: string; root: string };

/** When this browser first saw each certificate and removal (ms), by its
 * signature (`checkpoint::Seen`). */
export type Seen = Record<string, number>;

/** `fresh`, and every one of `old` it leaves out. */
export function kept<T>(fresh: T[], old: T[], same: (a: T, b: T) => boolean): T[] {
  const out = [...fresh];
  for (const o of old) if (!out.some((x) => same(x, o))) out.push(o);
  return out;
}

const sameRevocation = (a: Revocation, b: Revocation) =>
  a.v === b.v && a.account === b.account && a.device === b.device && a.at === b.at && a.by === b.by && a.sig === b.sig;

/** Stamp each of `certs` and `revocations` not stamped yet with `now`. */
export function stamp(seen: Seen, certs: Cert[], revocations: Revocation[], now: number) {
  for (const sig of [...certs.map((c) => c.sig), ...revocations.map((r) => r.sig)]) if (!(sig in seen)) seen[sig] = now;
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
  const at = (sig: string) => seen[sig] ?? Number.MAX_SAFE_INTEGER;
  const removedAt = (device: string) => {
    const ts = revocations.filter((r) => r.account === trust.account && r.device === device).map((r) => at(r.sig));
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
  now: number,
): Promise<{ certs: Cert[]; revocations: Revocation[]; refused: boolean }> {
  const revocations = kept(fresh.revocations, old.revocations, sameRevocation);
  stamp(seen, old.certs, revocations, now);
  stamp(seen, fresh.certs, [], now);
  const certs = await dropLate(trust, fresh.certs, revocations, seen);
  const gone = await removed(trust, old.certs, old.revocations);
  if (!gone.size) return { certs, revocations, refused: false };
  const after = await evaluate(trust, certs, revocations);
  if ([...gone].some((id) => after.has(id))) return { certs: await dropLate(trust, old.certs, revocations, seen), revocations, refused: true };
  return { certs, revocations, refused: false };
}
