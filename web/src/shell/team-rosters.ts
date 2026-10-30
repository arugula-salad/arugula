// The last roster this browser checked for each team, and what control
// sends checked against it, as a team machine checks its own
// (`checkpoint::advance` in crates/e2e). An owner's page signs the next
// version on top of the one it shows, so that one must follow the last it
// checked: control can't hand it a roster no owner wrote.

import type { Cert, Revocation } from "./e2e/cert.ts";
import { refreshAccount, type Seen } from "./e2e/checkpoint.ts";
import { advance, follows, rootChanges, sameRoster, type AccountCerts, type Roster, type TeamPin } from "./e2e/team.ts";

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

/** `offered` (control's latest for the team) checked against `held`. On
 * first sight (no `held`) the whole signed history is checked, from
 * version 1 and the founder `pin` names. `history(since)` fetches the
 * versions after `since`, with their signers' certificates; with `held`,
 * it's asked only when `offered` isn't the very next version. */
export async function checkRoster(held: Roster | null, offered: Roster, pin: TeamPin, certs: AccountCerts, history: History): Promise<Checked> {
  const no = (roster: Roster, problem: RosterProblem, changes: RootChange[] = [], late: number[] = [], keep: Roster | null = held): Checked => ({
    roster,
    problem,
    changes,
    late,
    keep,
  });
  if (held && offered.version < held.version) return no(held, "older");
  if (held && offered.version === held.version) return sameRoster(offered, held) ? { roster: held, problem: null, changes: [], late: [], keep: held } : no(held, "another");
  let cur: Roster | null = held;
  let stopped: number | null = null;
  let late: number[] = [];
  if (held && offered.version === held.version + 1 && (await follows(offered, held, pin, certs))) cur = offered;
  else {
    const h = await history(held?.version ?? 0).catch(() => null);
    if (!h) return no(held ?? offered, "unchecked");
    [cur, stopped, late] = await advance(held, h.rosters, pin, { ...certs, ...h.certs }, true);
  }
  if (!cur) return no(offered, "unchecked");
  const changes = held ? rootChanges(held, cur).map((c) => ({ ...c, version: cur.version })) : [];
  if (stopped !== null || !sameRoster(cur, offered)) return no(cur, "unchecked", changes, late, cur);
  return { roster: cur, problem: null, changes, late, keep: cur };
}

/** The last roster this page checked for each team: read from `storage`
 * once, then kept in memory for the page's life, so storage cleared or
 * changed under it doesn't take the page back to a first look. If it
 * can't be read or written, `kept` goes false for good: the page still
 * checks rosters, but signs no team change on what it would forget. */
export class RosterStore {
  private map: Record<string, Roster> | null = null;
  private storage: () => Pick<Storage, "getItem" | "setItem">;
  private key: string;
  kept = true;

  constructor(storage: () => Pick<Storage, "getItem" | "setItem">, key: string) {
    this.storage = storage;
    this.key = key;
  }

  held(): Record<string, Roster> {
    if (this.map) return this.map;
    try {
      const raw = this.storage().getItem(this.key);
      const v: unknown = raw === null ? {} : JSON.parse(raw);
      if (!v || typeof v !== "object" || Array.isArray(v)) throw new Error("not a map");
      this.map = v as Record<string, Roster>;
    } catch {
      this.kept = false;
      this.map = {};
    }
    return this.map;
  }

  save() {
    try {
      this.storage().setItem(this.key, JSON.stringify(this.held()));
    } catch {
      this.kept = false;
    }
  }
}

/** Each account's certificates and removals as this page last held them,
 * by account and the root they're checked from, and when it first saw
 * each (`checkpoint::Seen`): removals only add up across loads, and a
 * certificate first seen after its approver's removal doesn't count
 * (`refreshAccount`). Read once and kept in memory for the page's life,
 * as `RosterStore`; if storage can't be read or written, `kept` goes
 * false for good and the page signs no team change. */
export class TrustStore {
  private data: { accounts: Record<string, { certs: Cert[]; revocations: Revocation[] }>; seen: Seen } | null = null;
  private storage: () => Pick<Storage, "getItem" | "setItem">;
  private key: string;
  kept = true;

  constructor(storage: () => Pick<Storage, "getItem" | "setItem">, key: string) {
    this.storage = storage;
    this.key = key;
  }

  private load() {
    if (this.data) return this.data;
    try {
      const raw = this.storage().getItem(this.key);
      const v = raw === null ? { accounts: {}, seen: {} } : JSON.parse(raw);
      if (!v || typeof v !== "object" || typeof v.accounts !== "object" || typeof v.seen !== "object" || !v.accounts || !v.seen) throw new Error("not a store");
      this.data = v;
    } catch {
      this.kept = false;
      this.data = { accounts: {}, seen: {} };
    }
    return this.data!;
  }

  /** `trust`'s account as control sends it now (`fresh`), over what's held. */
  async take(trust: { account: string; root: string }, fresh: { certs: Cert[]; revocations: Revocation[] }, now = Date.now()) {
    const d = this.load();
    const k = `${trust.account}.${trust.root}`;
    const got = await refreshAccount(trust, fresh, d.accounts[k] ?? { certs: [], revocations: [] }, d.seen, now);
    d.accounts[k] = { certs: got.certs, revocations: got.revocations };
    return got;
  }

  /** Write it back, forgetting stamps of what's no longer held. */
  save() {
    const d = this.load();
    const held = new Set<string>();
    for (const a of Object.values(d.accounts)) for (const x of [...a.certs, ...a.revocations]) held.add(x.sig);
    for (const s of Object.keys(d.seen)) if (!held.has(s)) delete d.seen[s];
    try {
      this.storage().setItem(this.key, JSON.stringify(d));
    } catch {
      this.kept = false;
    }
  }
}
