// The hub's teams as Team and billing changes them in place (Members,
// Machines): `GET /api/teams`, each with its roster, pin, lock and who
// waits on an Ask me first link. Every change here is control's word
// (aru-141), unsigned: no device key is on this page. Until the hub serves
// these routes on the app's host it answers 404 there, and the sections
// keep their ↗ row to control's page instead (`elsewhere`).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { AskInvite, Roster, TeamPin, TeamRole } from "../../shared/team-roster";

/** A team as `GET /api/teams` has it: what this page reads of it. */
export interface HubTeam {
  team: string;
  pin: TeamPin;
  locked: boolean;
  roster: Roster;
  /** The reader's role in it. */
  role: TeamRole | null;
  /** Who used an Ask me first link and waits for an owner (owners only). */
  requests?: { account: string; root: string; name: string; created: number; invite: AskInvite }[];
  /** Members' names as they set them, by account: the roster's is one word. */
  names?: Record<string, string>;
}

/** A call to the hub: its JSON, or its `{error}` thrown, with the status. */
export async function hubCall<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw Object.assign(new Error(v.error ?? `Request failed (${r.status})`), { status: r.status });
  return v as T;
}

/** What the read said: the teams, that the hub doesn't answer here yet, or
 * why it failed. */
type Read = { teams: HubTeam[] } | { elsewhere: true } | { err: string };

let known: Promise<Read> | null = null;
const listeners = new Set<() => void>();

function read(): Promise<Read> {
  known ??= platformFetch("/api/teams", { credentials: "same-origin" }).then(
    async (r): Promise<Read> => {
      const v = (await r.json().catch(() => ({}))) as { teams?: HubTeam[]; error?: string };
      if (r.status === 404) return { elsewhere: true };
      if (!r.ok) return { err: v.error ?? `Request failed (${r.status})` };
      return { teams: v.teams ?? [] };
    },
    (e: Error): Read => ({ err: e.message }),
  );
  return known;
}

/** Read the teams again, for every section showing them. */
export function reloadTeams() {
  known = null;
  listeners.forEach((fn) => fn());
}

/** The hub's teams, read once a page load and again after a change: null
 * until read. A failed read is tried again on the next mount. */
export function useTeams(): Read | null {
  const [got, setGot] = useState<Read | null>(null);
  const [n, setN] = useState(0);
  useEffect(() => {
    const fn = () => setN((x) => x + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  useEffect(() => {
    let live = true;
    read().then((v) => {
      if ("err" in v) known = null;
      if (live) setGot(v);
    });
    return () => void (live = false);
  }, [n]);
  return got;
}
