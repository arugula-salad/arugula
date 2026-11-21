// Which teams control lets a person put a machine in, so the page offers
// only those and control never refuses a team after it's picked. Control
// has two rules, by how the machine gets there; each function here is one
// of them, word for word.

import type { TeamRole } from "./team-roster.ts";

/** What the rules read of a team: my role in its roster (none when I'm not
 * in it) and whether it's locked. */
export interface TeamPlace {
  role: TeamRole | null;
  locked: boolean;
}

/** A move (*Move to…*), or a join a device signs: control's `in_team`.
 * Its owners and editors put their own machine in (#332), and only its
 * owners while it's locked; a viewer, or someone not in the roster, never
 * does (#1184). Taking a machine out isn't this rule: its owner always
 * can. */
export function movesInto(t: TeamPlace): boolean {
  return t.role === "owner" || (t.role === "editor" && !t.locked);
}

/** A join added in one click, with no device key (aru-141), or a join
 * token: control's `may_add_to`. Its owners and editors, and only its
 * owners while it's locked; a viewer never does. */
export function addsInOneClick(t: TeamPlace): boolean {
  return t.role === "owner" || (t.role === "editor" && !t.locked);
}

/** Why a team the machine asked for isn't one I can pick (#1183), for the
 * option's note and the error under it: `t` is my place in it, if I'm
 * in it at all. */
export function cantAdd(name: string, t: TeamPlace | null): { note: string; error: string } {
  const ask = "Ask one of them to approve it, or pick Just me.";
  if (t?.locked && t.role !== "owner") return { note: "locked", error: `${name} is locked: only its owners add machines to it. ${ask}` };
  if (t?.role === "viewer")
    return { note: "viewers can't add machines", error: `Viewers can't add machines to ${name}: only its owners and editors do. ${ask}` };
  // An owner or editor whose roster this device couldn't check.
  if (t?.role) return { note: "not checked here", error: `${name}'s roster didn't check out on this browser, so it can't add machines to it. Approve it on another device, or pick Just me.` };
  return { note: "you're not in it", error: `Only ${name}'s owners and editors add machines to it. ${ask}` };
}
