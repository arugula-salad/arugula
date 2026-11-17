// The Machines list's rows (Arugula #990): what each machine says of itself,
// and the order they come in. Pure, so a test reads it without a page.

import type { FleetHost, FleetPane } from "../shell/fleet";
import { needsYou } from "../shared/needs-you.ts";
import type { ProjectRef } from "../shared/proto.gen";

/** Whether a session in `project`, on a machine that's `yours` or not,
 * shows here (`inShown`, Arugula #1164); every one by default. */
export type Shows = (project: ProjectRef | null | undefined, yours: boolean) => boolean;
const every: Shows = () => true;

export type Role = "owner" | "editor" | "viewer";

export interface Row {
  /** The name the directory knows it by. */
  name: string;
  id?: string;
  online: boolean;
  /** A cloud workspace, not someone's computer. */
  cloud: boolean;
  /** Whose it is when it isn't yours. */
  owner?: string;
  role: Role;
  /** The project's sessions on it, as the fleet last heard; null before it has. */
  sessions: number | null;
  /** How many of them have a pane that needs you. */
  needsYou: number;
}

const RANK: Record<Role, number> = { viewer: 0, editor: 1, owner: 2 };

/** What you may do there: the role the token carries for a cloud workspace;
 * yours when the machine is; else the most the machine gives you in any of
 * its sessions, and watching when it says nothing. */
export function roleOf(o: { mine: boolean; hosted?: Role; roles?: Role[] }): Role {
  if (o.hosted) return o.hosted;
  if (o.mine) return "owner";
  return (o.roles ?? []).reduce<Role>((a, r) => (RANK[r] > RANK[a] ? r : a), "viewer");
}

export const ROLE_WORDS: Record<Role, string> = {
  owner: "You own it",
  editor: "You can steer",
  viewer: "You can watch",
};

/** The rows for `hosts`, with `panes` the fleet's panes, `hostedRole`
 * the token's role for a cloud workspace by machine id, and counting only
 * the sessions `shows` lets through. */
export function rowsOf(hosts: FleetHost[], panes: FleetPane[], hostedRole: (id: string) => Role | undefined = () => undefined, shows: Shows = every): Row[] {
  return hosts.map((h) => {
    const yours = !h.owner;
    const sessions = h.summary?.sessions.filter((s) => shows(s.project, yours)) ?? null;
    const asking = new Set<number>();
    for (const p of panes) {
      if (p.host !== h.name || !shows(p.info.session_project, yours)) continue;
      if (p.session && needsYou(p.info)) asking.add(p.session.id);
    }
    return {
      name: h.name,
      id: h.id,
      online: h.status ? h.status !== "offline" : h.state === "connected",
      cloud: !!h.hosted,
      owner: h.owner,
      role: roleOf({ mine: !h.ownerId && !h.team, hosted: h.id ? hostedRole(h.id) : undefined, roles: (h.summary?.roles ?? []).map(([, r]) => r as Role) }),
      sessions: sessions ? sessions.length : null,
      needsYou: asking.size,
    };
  });
}

/** Needs you first, then online, then by name. */
export function sortRows(rows: Row[]): Row[] {
  return [...rows].sort((a, b) => Number(b.needsYou > 0) - Number(a.needsYou > 0) || Number(b.online) - Number(a.online) || a.name.localeCompare(b.name));
}

/** "3 sessions · 1 needs you", "No sessions" ("No Site sessions yet." in
 * `project`), or nothing before it's heard. */
export function sessionsLine(r: Pick<Row, "sessions" | "needsYou">, project?: string): string {
  if (r.sessions === null) return "";
  if (r.sessions === 0) return project ? `No ${project} sessions yet.` : "No sessions";
  const n = `${r.sessions} ${r.sessions === 1 ? "session" : "sessions"}`;
  return r.needsYou ? `${n} · ${r.needsYou} ${r.needsYou === 1 ? "needs" : "need"} you` : n;
}

/** One machine's panes in Build's list (Arugula #1083). */
export interface PaneGroup {
  name: string;
  id?: string;
  online: boolean;
  /** A cloud workspace, not someone's computer. */
  cloud: boolean;
  /** Whose it is when it isn't yours. */
  owner?: string;
  panes: FleetPane[];
}

/** Every machine's panes, by machine, or only `machine`'s (by its id or,
 * without one, its name): the machines with something that needs you
 * first, then the online ones, then by name; in each, what needs you
 * first, then by session and pane. An editor that only joined (no tab) is
 * no pane to open, and a pane whose session `shows` keeps out isn't there. */
export function paneGroups(hosts: FleetHost[], panes: FleetPane[], machine?: string, shows: Shows = every): PaneGroup[] {
  const groups = hosts
    .filter((h) => !machine || h.id === machine || (!h.id && h.name === machine))
    .map((h) => ({
      name: h.name,
      id: h.id,
      online: h.status ? h.status !== "offline" : h.state === "connected",
      cloud: !!h.hosted,
      owner: h.owner,
      panes: panes
        .filter((p) => p.host === h.name && !(p.info.type === "editor" && p.session === null) && shows(p.info.session_project, !h.owner))
        .sort((a, b) => Number(needsYou(b.info)) - Number(needsYou(a.info)) || (a.session?.id ?? 0) - (b.session?.id ?? 0) || a.id - b.id),
    }));
  const asks = (g: PaneGroup) => Number(g.panes.some((p) => needsYou(p.info)));
  return groups.sort((a, b) => asks(b) - asks(a) || Number(b.online) - Number(a.online) || a.name.localeCompare(b.name));
}
