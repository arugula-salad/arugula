// The last roster this browser checked for each team, and what control
// sends checked against it, as a team machine checks its own
// (`checkpoint::advance` in crates/e2e). An owner's page signs the next
// version on top of the one it shows, so that one must follow the last it
// checked: control can't hand it a roster no owner wrote.

import { advance, follows, rootChanges, sameRoster, type AccountCerts, type Roster, type TeamPin } from "./e2e/team.ts";

/** Why the roster control sent isn't taken: an older version than the one
 * held, another version at one held, or one that doesn't follow it (or
 * whose versions in between couldn't be fetched). */
export type RosterProblem = "older" | "another" | "unchecked";

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
  /** The latest version that checks out: control's, or the one held. */
  roster: Roster;
  problem: RosterProblem | null;
  changes: RootChange[];
}

/** `offered` (control's latest for the team) checked against `held` (none
 * on first sight: taken, as the founder is). `history(since)` fetches the
 * versions after `since`, with their signers' certificates; it's asked
 * only when `offered` isn't the very next version. */
export async function checkRoster(
  held: Roster | null,
  offered: Roster,
  pin: TeamPin,
  certs: AccountCerts,
  history: (since: number) => Promise<{ rosters: Roster[]; certs: AccountCerts }>,
): Promise<Checked> {
  if (!held) return { roster: offered, problem: null, changes: [] };
  if (offered.version < held.version) return { roster: held, problem: "older", changes: [] };
  if (offered.version === held.version) return sameRoster(offered, held) ? { roster: held, problem: null, changes: [] } : { roster: held, problem: "another", changes: [] };
  let cur: Roster | null = held;
  let stopped: number | null = null;
  if (offered.version === held.version + 1 && (await follows(offered, held, pin, certs))) cur = offered;
  else {
    const h = await history(held.version).catch(() => null);
    if (!h) return { roster: held, problem: "unchecked", changes: [] };
    [cur, stopped] = await advance(held, h.rosters, pin, { ...certs, ...h.certs });
  }
  const at = cur ?? held;
  const changes = rootChanges(held, at).map((c) => ({ ...c, version: at.version }));
  if (stopped !== null || !sameRoster(at, offered)) return { roster: at, problem: "unchecked", changes };
  return { roster: at, problem: null, changes };
}
