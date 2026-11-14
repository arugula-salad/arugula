// The Machines list's rows (Arugula #990): what each machine says of itself,
// and the order they come in. Pure, so a test reads it without a page.

import type { FleetHost, FleetPane } from "../shell/fleet";
import { needsYou } from "../shared/needs-you.ts";

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
  /** Its sessions, as the fleet last heard; null before it has. */
  sessions: number | null;
  /** How many of them have a pane that needs you. */
  needsYou: number;
  /** The projects its sessions serve; empty when the fleet doesn't say. */
  projects: string[];
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

/** The rows for `hosts`, with `panes` the fleet's panes and `hostedRole`
 * the token's role for a cloud workspace by machine id. */
export function rowsOf(hosts: FleetHost[], panes: FleetPane[], hostedRole: (id: string) => Role | undefined = () => undefined): Row[] {
  return hosts.map((h) => {
    const sessions = h.summary?.sessions ?? null;
    const asking = new Set<number>();
    const projects = new Set<string>();
    for (const p of panes) {
      if (p.host !== h.name) continue;
      if (p.session && needsYou(p.info)) asking.add(p.session.id);
      if (p.info.session_project) projects.add(p.info.session_project.name);
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
      projects: [...projects].sort((a, b) => a.localeCompare(b)),
    };
  });
}

/** Needs you first, then online, then by name. */
export function sortRows(rows: Row[]): Row[] {
  return [...rows].sort((a, b) => Number(b.needsYou > 0) - Number(a.needsYou > 0) || Number(b.online) - Number(a.online) || a.name.localeCompare(b.name));
}

/** "3 sessions · 1 needs you", "No sessions", or nothing before it's heard. */
export function sessionsLine(r: Pick<Row, "sessions" | "needsYou">): string {
  if (r.sessions === null) return "";
  if (r.sessions === 0) return "No sessions";
  const n = `${r.sessions} ${r.sessions === 1 ? "session" : "sessions"}`;
  return r.needsYou ? `${n} · ${r.needsYou} ${r.needsYou === 1 ? "needs" : "need"} you` : n;
}
