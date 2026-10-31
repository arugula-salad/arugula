// The last roster this browser checked for each team, and what control
// sends checked against it, as a team machine checks its own
// (`checkpoint::advance` in crates/e2e). An owner's page signs the next
// version on top of the one it shows, so that one must follow the last it
// checked: control can't hand it a roster no owner wrote.

import type { Cert, Revocation } from "./e2e/cert.ts";
import { isSeen, nextRound, refreshAccount, settle, type Seen } from "./e2e/checkpoint.ts";
import { advance, follows, lockSignedIn, rootChanges, sameRoster, type AccountCerts, type Roster, type TeamLock, type TeamPin } from "./e2e/team.ts";

/** Why the roster control sent isn't taken: an older version than the one
 * held, another version at one held, or one that doesn't follow it (or
 * whose versions in between couldn't be fetched). `unsaved`: it's taken,
 * but this browser can't keep what it checked, so changes wait. */
export type RosterProblem = "older" | "another" | "unchecked" | "unsaved";

/** A member's root that changed between the version held and the one
 * taken: another key for that account. */
export interface RootChange {
  account: string;
  name: string;
  from: string;
  to: string;
  version: number;
}

export interface Checked {
  /** The latest version that checks out: control's, the one held, or (if
   * none does on a first look) control's, with a problem. */
  roster: Roster;
  problem: RosterProblem | null;
  changes: RootChange[];
  /** Versions taken that a device removed since had signed, while it
   * still counted (`advance`'s `lateOk`): shown, not refused. */
  late: number[];
  /** The latest version checked, to keep: none when nothing checked out. */
  keep: Roster | null;
}

type History = (since: number) => Promise<{ rosters: Roster[]; certs: AccountCerts }>;

/** How the page checks a step: `vet` gives the certificates it holds for
 * a roster's owners, and `lateOk` says whether a version signed by a
 * device removed since may stand (`advance`). The defaults take control's
 * certificates as they come and allow none. */
export interface CheckWith {
  vet?: (prev: Roster | null, certs: AccountCerts) => Promise<AccountCerts>;
  lateOk?: (account: string, root: string, device: string) => boolean;
}

/** `offered` (control's latest for the team) checked against `held`. On
 * first sight (no `held`) the whole signed history is checked, from
 * version 1 and the founder `pin` names. `history(since)` fetches the
 * versions after `since`, with their signers' certificates; with `held`,
 * it's asked only when `offered` isn't the very next version. Every step
 * is checked with the certificates `vet` gives, history's included. */
export async function checkRoster(held: Roster | null, offered: Roster, pin: TeamPin, certs: AccountCerts, history: History, opts: CheckWith = {}): Promise<Checked> {
  const no = (roster: Roster, problem: RosterProblem, changes: RootChange[] = [], late: number[] = [], keep: Roster | null = held): Checked => ({
    roster,
    problem,
    changes,
    late,
    keep,
  });
  if (held && offered.version < held.version) return no(held, "older");
  if (held && offered.version === held.version) return sameRoster(offered, held) ? { roster: held, problem: null, changes: [], late: [], keep: held } : no(held, "another");
  const vet = opts.vet ?? (async (_: Roster | null, c: AccountCerts) => c);
  let cur: Roster | null = held;
  let stopped: number | null = null;
  let late: number[] = [];
  if (held && offered.version === held.version + 1 && (await follows(offered, held, pin, await vet(held, certs)))) cur = offered;
  else {
    const h = await history(held?.version ?? 0).catch(() => null);
    if (!h) return no(held ?? offered, "unchecked");
    [cur, stopped, late] = await advance(held, h.rosters, pin, { ...certs, ...h.certs }, { vet, lateOk: opts.lateOk });
  }
  if (!cur) return no(offered, "unchecked");
  const changes = held ? rootChanges(held, cur).map((c) => ({ ...c, version: cur.version })) : [];
  if (stopped !== null || !sameRoster(cur, offered)) return no(cur, "unchecked", changes, late, cur);
  return { roster: cur, problem: null, changes, late, keep: cur };
}

type Store = () => Pick<Storage, "getItem" | "setItem">;

const isRoster = (v: unknown): v is Roster =>
  !!v && typeof v === "object" && typeof (v as Roster).team === "string" && Number.isSafeInteger((v as Roster).version) && Array.isArray((v as Roster).members);

/** Read `key` from `storage` as JSON that `ok` accepts: `null` when there's
 * none, `undefined` when it can't be read or isn't well formed. */
function readStored<T>(storage: Store, key: string, ok: (v: unknown) => v is T): T | null | undefined {
  try {
    const raw = storage().getItem(key);
    if (raw === null) return null;
    const v: unknown = JSON.parse(raw);
    return ok(v) ? v : undefined;
  } catch {
    return undefined;
  }
}

/** The last roster this page checked for each team: read from `storage`
 * once, then kept in memory for the page's life, so storage cleared or
 * changed under it doesn't take the page back to a first look; on each
 * save, what another tab of this browser checked since is merged in (the
 * newer version of each team). If it can't be read or written, or is
 * missing when `expected` says this browser kept one, `kept` goes false
 * for good: the page still checks rosters, but signs no team change on
 * what it would forget. */
export class RosterStore {
  private map: Record<string, Roster> | null = null;
  private storage: Store;
  private key: string;
  private expected: () => boolean;
  kept = true;

  constructor(storage: Store, key: string, expected: () => boolean = () => false) {
    this.storage = storage;
    this.key = key;
    this.expected = expected;
  }

  private static ok(v: unknown): v is Record<string, Roster> {
    return !!v && typeof v === "object" && !Array.isArray(v) && Object.values(v).every(isRoster);
  }

  held(): Record<string, Roster> {
    if (this.map) return this.map;
    const got = readStored(this.storage, this.key, RosterStore.ok);
    if (got === undefined || (got === null && this.expected())) this.kept = false;
    this.map = got ?? {};
    return this.map;
  }

  save() {
    const mine = this.held();
    const theirs = readStored(this.storage, this.key, RosterStore.ok);
    if (theirs === undefined) this.kept = false;
    for (const [t, r] of Object.entries(theirs ?? {})) if (!mine[t] || r.version > mine[t].version) mine[t] = r;
    try {
      this.storage().setItem(this.key, JSON.stringify(mine));
    } catch {
      this.kept = false;
    }
  }
}

interface Held {
  accounts: Record<string, { certs: Cert[]; revocations: Revocation[] }>;
  seen: Seen;
  /** Each team's lock as an owner signed it, the latest this page took. */
  locks: Record<string, TeamLock>;
}

/** Runs `fn` holding the browser-wide lock `name` (the Web Locks API), so
 * no other tab of this browser runs one at the same time. `null` where
 * there's none: the page then keeps nothing across loads. */
export type Locks = (<T>(name: string, fn: () => Promise<T>) => Promise<T>) | null;

export const webLocks: Locks = (() => {
  const l = (globalThis as { navigator?: { locks?: { request: (name: string, fn: () => Promise<unknown>) => Promise<unknown> } } }).navigator?.locks;
  return l ? <T,>(name: string, fn: () => Promise<T>) => l.request(name, fn) as Promise<T> : null;
})();

/** One look at what's held, inside [`TrustStore::look`]. */
export interface Look {
  /** `trust`'s account as control sends it now (`fresh`), over what's held. */
  take(trust: { account: string; root: string }, fresh: { certs: Cert[]; revocations: Revocation[] }): Promise<{ certs: Cert[]; revocations: Revocation[]; refused: boolean }>;
  /** Whether `device`'s removal, for `account` from `root`, was seen before this look. */
  removedBefore(account: string, root: string, device: string): boolean;
  /** The lock held for `team`. */
  lock(team: string): TeamLock | undefined;
  /** Hold `l` for `team` if it's newer than the one held. */
  holdLock(team: string, l: TeamLock): void;
}

const emptyHeld = (): Held => ({ accounts: {}, seen: { round: 0, at: {} }, locks: {} });

/** Each account's certificates and removals as this page last held them,
 * by account and the root they're checked from, in which round it first
 * saw each (`checkpoint::Seen`), and each team's signed lock: removals
 * only add up across loads, and a certificate first seen after its
 * approver's removal doesn't count (`refreshAccount`).
 *
 * Each look (`look`) is one transaction across this browser's tabs: it
 * takes the browser-wide lock, reads what's stored afresh (another tab
 * may have looked since), counts its round after the stored one, and
 * writes back before letting go. So no two tabs share a round, and
 * nothing is merged: what's stored is always one tab's whole view. If the
 * store can't be read or written, is malformed, is missing when
 * `expected` says this browser kept one, or there's no browser-wide lock,
 * `kept` goes false for good: the look runs on a copy of what this page
 * last held and keeps nothing, and the page signs no team change and
 * lists no one else's machines. */
export class TrustStore {
  private data: Held | null = null;
  private storage: Store;
  private key: string;
  private expected: () => boolean;
  private locks: Locks;
  kept = true;

  constructor(storage: Store, key: string, expected: () => boolean = () => false, locks: Locks = webLocks) {
    this.storage = storage;
    this.key = key;
    this.expected = expected;
    this.locks = locks;
  }

  private static ok(v: unknown): v is Held {
    if (!v || typeof v !== "object" || Array.isArray(v)) return false;
    const h = v as Held;
    const accounts = !!h.accounts && typeof h.accounts === "object" && Object.values(h.accounts).every((a) => !!a && Array.isArray(a.certs) && Array.isArray(a.revocations));
    const locks = h.locks === undefined || (!!h.locks && typeof h.locks === "object" && Object.values(h.locks).every((l) => !!l && typeof l.at === "number" && Number.isSafeInteger(l.at)));
    return accounts && isSeen(h.seen) && locks;
  }

  /** What's stored now, or `undefined` if it can't be trusted. */
  private read(): Held | undefined {
    const got = readStored(this.storage, this.key, TrustStore.ok);
    if (got === undefined || (got === null && (this.expected() || this.data))) return undefined;
    if (got) got.locks ??= {};
    return got ?? emptyHeld();
  }

  /** What this page last held (or kept), for `trust`'s account: its view
   * when a look can't be kept. */
  view(trust: { account: string; root: string }): { certs: Cert[]; revocations: Revocation[] } | undefined {
    return this.data?.accounts[`${trust.account}.${trust.root}`];
  }

  /** The lock this page holds for `team`. */
  lock(team: string): TeamLock | undefined {
    return this.data?.locks[team];
  }

  /** Run one look: `fn` takes accounts, asks about removals and holds
   * locks through what it's given. True if what it found was kept. */
  async look<T>(fn: (l: Look) => Promise<T>): Promise<{ value: T; kept: boolean }> {
    const run = async () => {
      const fresh = this.kept && this.locks ? this.read() : undefined;
      if (!fresh) this.kept = false;
      // Kept: the stored view. Not: a copy of this page's, thrown away.
      const d: Held = fresh ?? structuredClone(this.data ?? emptyHeld());
      const round = nextRound(d.seen);
      const value = await fn({
        take: async (trust, got) => {
          const k = `${trust.account}.${trust.root}`;
          const out = await refreshAccount(trust, got, d.accounts[k] ?? { certs: [], revocations: [] }, d.seen, round);
          d.accounts[k] = { certs: out.certs, revocations: out.revocations };
          return out;
        },
        removedBefore: (account, root, device) =>
          (d.accounts[`${account}.${root}`]?.revocations ?? []).some((r) => r.device === device && Object.hasOwn(d.seen.at, r.sig) && d.seen.at[r.sig] < round),
        lock: (team) => d.locks[team],
        holdLock: (team, l) => {
          if (!d.locks[team] || l.at > d.locks[team].at) d.locks[team] = l;
        },
      });
      if (!fresh) return { value, kept: false };
      settle(d.seen, round);
      const held = new Set<string>();
      for (const a of Object.values(d.accounts)) for (const x of [...a.certs, ...a.revocations]) held.add(x.sig);
      for (const sig of Object.keys(d.seen.at)) if (!held.has(sig)) delete d.seen.at[sig];
      try {
        this.storage().setItem(this.key, JSON.stringify(d));
      } catch {
        this.kept = false;
        return { value, kept: false };
      }
      this.data = d;
      return { value, kept: true };
    };
    return this.locks && this.kept ? this.locks("arugula-trust", run) : run();
  }
}

/** How far ahead of this browser's clock a team's signed lock may be dated
 * (control refuses more, by its own). */
export const LOCK_SKEW_MS = 10 * 60 * 1000;

/** The lock this page holds for `team` after control sends `got`: held if
 * an owner in `roster` (the roster checked) signed it for this team, it
 * isn't dated far ahead of `now`, and it's newer than the one held
 * (`checkpoint::take_lock`). */
export async function takeLock(store: Pick<Look, "lock" | "holdLock">, team: string, got: TeamLock | null | undefined, roster: Roster, certs: AccountCerts, now = Date.now()): Promise<TeamLock | undefined> {
  if (got && got.team === team && Number.isSafeInteger(got.at) && got.at <= now + LOCK_SKEW_MS && (await lockSignedIn(got, roster, certs))) store.holdLock(team, got);
  return store.lock(team);
}

/** How long a fetch the page's trust waits on may take. */
export const FETCH_TIMEOUT_MS = 15_000;

/** `send`, given up on (aborted) after `ms`. */
export function withTimeout(send: (path: string, init: RequestInit) => Promise<Response>, ms = FETCH_TIMEOUT_MS) {
  return (path: string, init: RequestInit = {}) => {
    const ctl = new AbortController();
    const timer = setTimeout(() => ctl.abort(new Error(`no answer in ${ms / 1000} s`)), ms);
    return send(path, { ...init, signal: ctl.signal }).finally(() => clearTimeout(timer));
  };
}
