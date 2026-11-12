// Whose each project is (Arugula #895, aru-140): Personal, the reader's
// own, or a team's. The bar's switcher lists the owners, then the chosen
// one's projects, its archived ones apart. Plain functions over the
// projects the platform sent, tested in scripts/owners.test.mjs.

import type { PlatformProject, ProjectMember } from "../shared/proto.gen.ts";

/** Personal's key; a team's is its id. */
export const PERSONAL = "personal";

/** One owner of the reader's projects. */
export interface Owner {
  /** `PERSONAL`, or the team's id. */
  key: string;
  kind: "personal" | "team";
  name: string;
  /** The reader's role in it: `owner` for Personal; a team's, as one of its
   * projects says, or "" when none does. */
  role: string;
  /** A team's members, as one of its projects says. */
  members: ProjectMember[];
  /** Its projects, archived ones left out, in the order they came. */
  projects: PlatformProject[];
  /** Its archived projects. */
  archived: PlatformProject[];
}

/** Whose `p` is: its `owner`, or, from an older control, its `team`. */
export function ownerKey(p: Pick<PlatformProject, "owner" | "team">): string {
  if (p.owner) return p.owner.kind === "team" ? p.owner.id : PERSONAL;
  return p.team?.id ?? PERSONAL;
}

/** The reader's owners: Personal first, always, then each team in the
 * order its first project came. */
export function owners(projects: readonly PlatformProject[]): Owner[] {
  const mine: Owner = { key: PERSONAL, kind: "personal", name: "Personal", role: "owner", members: [], projects: [], archived: [] };
  const all = new Map<string, Owner>([[PERSONAL, mine]]);
  for (const p of projects) {
    const key = ownerKey(p);
    let o = all.get(key);
    if (!o) {
      const name = (p.owner?.kind === "team" ? p.owner.name : undefined) || p.team?.name || "Team";
      o = { key, kind: "team", name, role: "", members: [], projects: [], archived: [] };
      all.set(key, o);
    }
    // A team's default project carries the reader's role and the roster.
    if (p.team?.id === key && !o.role) {
      o.role = p.team.role;
      o.members = p.team.members;
    }
    (p.archived ? o.archived : o.projects).push(p);
  }
  return [...all.values()];
}

/** The owner `key` names, among `projects`' owners. */
export function ownerNamed(projects: readonly PlatformProject[], key: string | null | undefined): Owner | undefined {
  return key ? owners(projects).find((o) => o.key === key) : undefined;
}

/** The owners a new project can be made for, or a project moved to: Personal,
 * and the teams the reader owns. */
export function makers(all: readonly Owner[]): Owner[] {
  return all.filter((o) => o.role === "owner");
}
