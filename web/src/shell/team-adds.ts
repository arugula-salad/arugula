// Which teams control lets a person put a machine in, so the page offers
// only those and control never refuses a team after it's picked. Control
// has two rules, by how the machine gets there; each function here is one
// of them, word for word.

import type { TeamRole } from "./e2e/team.ts";

/** What the rules read of a team: my role in its roster (none when I'm not
 * in it) and whether it's locked. */
export interface TeamPlace {
  role: TeamRole | null;
  locked: boolean;
}

/** A move (*Move to…*), or a join a device signs: control's `in_team`. Any
 * member puts their own machine in (#332), but only its owners while it's
 * locked; someone not in the roster never does. */
export function movesInto(t: TeamPlace): boolean {
  return t.role === "owner" || (t.role !== null && !t.locked);
}

/** A join added in one click, with no device key (aru-141), or a join
 * token: control's `may_add_to`. Its owners and editors, and only its
 * owners while it's locked; a viewer never does. */
export function addsInOneClick(t: TeamPlace): boolean {
  return t.role === "owner" || (t.role === "editor" && !t.locked);
}
