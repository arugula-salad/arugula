// The session menu's Project… (Arugula #1177, the Doc "Sessions follow the
// project (#1161)", tab 3): the reader's projects, each saying who sees its
// sessions, and what moving the session to one does to who sees it. Every
// session is in a project (#1163), and who sees it follows that (#1162):
// a team's project's, its team watches; your own's, just you.
import type { PlatformProject, PlatformProjects, ProjectRef } from "../../shared/proto";

/** A project's team, the one that watches its sessions: none for your own.
 * An older control names no owner, and then no team does. */
export function teamOf(p: PlatformProject): { id: string; name: string } | null {
  return p.owner?.kind === "team" ? { id: p.owner.id, name: p.owner.name } : null;
}

/** Who sees a project's sessions, beside it in the list. */
export function whoSees(p: PlatformProject): string {
  const team = teamOf(p);
  return team ? `${team.name} watches` : "Just you";
}

/** The projects a session can go in: the reader's, archived ones only
 * when the session is in it. */
export function projectRows(all: PlatformProjects, current: ProjectRef | null | undefined): PlatformProject[] {
  return all.projects.filter((p) => !p.archived || p.id === current?.id);
}

/** The team that watches the session now: its project's, as the list
 * names it, else as the session keeps it (named by a project of the
 * team's, else by the session's project). */
export function teamNow(all: PlatformProjects, current: ProjectRef | null | undefined): { id: string; name: string } | null {
  const listed = all.projects.find((p) => p.id === current?.id);
  if (listed) return teamOf(listed);
  if (!current?.team) return null;
  const owner = all.projects.map((p) => p.owner).find((o) => o?.kind === "team" && o.id === current.team);
  return { id: current.team, name: owner?.kind === "team" ? owner.name : current.name };
}

const names = (xs: string[]) => (xs.length < 2 ? xs.join("") : `${xs.slice(0, -1).join(", ")} and ${xs[xs.length - 1]}`);

/** The line under the list: who sees the session now (`to` is its own
 * project, or none picked), or what moving it to `to` does to that. A
 * move that lets a team watch warns. A share with a person stays wherever
 * the session goes (decision 4), so it names them. */
export function whoLine(
  now: { id: string; name: string } | null,
  current: ProjectRef | null | undefined,
  to: PlatformProject | null,
  shared: string[] = [],
): { text: string; warn: boolean } {
  const stays = shared.length ? ` It stays shared with ${names(shared)}.` : "";
  if (!to || to.id === current?.id) return { text: `Now: ${now ? `you, and the ${now.name} team watching` : "only you"}.${stays}`, warn: false };
  const team = teamOf(to);
  if (team && team.id !== now?.id) {
    const leaves = now ? ` The ${now.name} team stops seeing it.` : "";
    return {
      text: `Moving it to ${to.name} lets the ${team.name} team watch it: its tabs, its panes and what they print, including what they printed before.${leaves}${stays}`,
      warn: true,
    };
  }
  if (now && !team) return { text: `Moving it to ${to.name} takes it away from the ${now.name} team: only you will see it.${stays}`, warn: false };
  return { text: `Who sees it doesn't change: ${team ? `the ${team.name} team watches` : "only you"}.${stays}`, warn: false };
}

/** What the toast says once it's moved. */
export function movedSaid(session: string, to: PlatformProject): string {
  const team = teamOf(to);
  return `“${session}” is in ${to.name} now. ${team ? `The ${team.name} team can watch it.` : "Only you can see it."}`;
}

/** A project as a session keeps it, with its team, which says who watches
 * the session (Arugula #1162). An older control names no owner, and then no
 * team does. */
export function projectRef(p: PlatformProject): ProjectRef {
  const team = teamOf(p)?.id;
  return team ? { id: p.id, name: p.name, team } : { id: p.id, name: p.name };
}
