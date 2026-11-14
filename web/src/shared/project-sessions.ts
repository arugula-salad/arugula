// A project's sessions by card (Arugula inversion, L3 and L6): the hub's
// `GET /api/projects/{project}/sessions/by-card`, for any member of the
// project. It names each session, where it runs and its state, grouped by
// the card it works on, with the sessions on no card last (`card` null).
// It has ids, names and states only: a session's last line, its live panes
// and what needs you come from the machines themselves. The Overview and
// the Sessions page both read it through here. The cloud's route isn't
// everywhere yet: a hub without it answers 404, which reads as null.

import { platformFetch } from "./maintenance.ts";

/** The card a group of sessions works on: a native issue's number and
 * title, or its id alone for a card that isn't an issue. */
export interface SessionCard {
  id: string;
  number?: number;
  title?: string;
}

/** Where a session runs: one of the team's machines, or Arugula cloud. */
export type SessionOn = { kind: "machine"; id: string; name: string } | { kind: "hosted" };

/** One pane of a session on a machine. `work_item` is the pane's own card
 * when it has one (an `iss_…`); `state` may not be known. */
export interface SessionPane {
  pane: number;
  kind: string;
  work_item: string | null;
  state: string | null;
}

/** A session: a set of tabs and panes on a machine, an agent block, or a
 * session in a cloud workspace. `updated_at` is in ms (0: not known). */
export type ProjectSession =
  | { kind: "machine"; id: string; name: string; on: SessionOn; panes: SessionPane[]; updated_at: number }
  | { kind: "agent"; id: number; name: string; on: SessionOn; state: string; updated_at: number }
  | { kind: "hosted"; id: string; name: string; on: SessionOn; state: string; updated_at: number };

export interface SessionGroup {
  /** null: Not on a card, which comes last. */
  card: SessionCard | null;
  sessions: ProjectSession[];
}

export interface SessionsByCard {
  project: string;
  groups: SessionGroup[];
}

/** The hub reads this at most 600 times an hour per account: a page that
 * keeps it fresh reads it no more often than this. */
export const BY_CARD_EVERY_MS = 60_000;

/** The project's sessions by card, or null where the hub has no such
 * route yet (404). Anything else that isn't an answer throws. */
export async function sessionsByCard(project: string, send: (url: string, init: RequestInit) => Promise<Response> = platformFetch): Promise<SessionsByCard | null> {
  const res = await send(`/api/projects/${encodeURIComponent(project)}/sessions/by-card`, { credentials: "same-origin" });
  if (res.status === 404) return null;
  const j = (await res.json().catch(() => null)) as (SessionsByCard & { error?: string }) | null;
  if (!res.ok || !j || !Array.isArray(j.groups)) throw new Error(j?.error ?? `HTTP ${res.status}`);
  return j;
}

/** The panes on machines a group's sessions name, as `machine id → pane
 * ids`: an agent block is its own pane; a session lists its panes. */
export function panesOf(sessions: ProjectSession[]): { machine: string; pane: number }[] {
  const out: { machine: string; pane: number }[] = [];
  for (const s of sessions) {
    if (s.on.kind !== "machine") continue;
    if (s.kind === "agent") out.push({ machine: s.on.id, pane: s.id });
    else if (s.kind === "machine") for (const p of s.panes) out.push({ machine: s.on.id, pane: p.pane });
  }
  return out;
}
