// A project's sessions by the card each works on (Arugula inversion L3):
// the cloud's `GET /api/projects/{p}/sessions/by-card`, every session in
// the project wherever it runs (a machine's, an agent's pane, a hosted
// one), put together with the hosted wave (`/sessions`), which says more of
// a hosted session (its branch and push, what waits on you) and carries
// the filters. Pure, so scripts/sessions-by-card.test.mjs reads it.

import type { SessionRow, SessionWave } from "../shared/proto.gen";

/** A card a group works on: an issue's id, number and title, or `{ id }`
 * alone for a card that isn't an issue. */
export interface ByCardCard {
  id: string;
  number?: number;
  title?: string;
}

/** A pane of a machine's session. `work_item` and `state` may be null. */
export interface ByCardPane {
  pane: number;
  kind: string;
  work_item: string | null;
  state: string | null;
}

export type ByCardOn = { kind: "machine"; id: string; name: string } | { kind: "hosted" };

export type ByCardSession =
  | { kind: "machine"; id: string; name: string; on: ByCardOn; panes: ByCardPane[]; updated_at: number }
  | { kind: "agent"; id: number; name: string; on: ByCardOn; state: string | null; updated_at: number }
  | { kind: "hosted"; id: string; name: string; on: ByCardOn; state: string | null; updated_at: number };

/** `card` null is Not on a card, always last. */
export interface ByCardGroup {
  card: ByCardCard | null;
  sessions: ByCardSession[];
}

export interface SessionsByCard {
  project: string;
  groups: ByCardGroup[];
}

/** A session on the page: the by-card answer's, the wave's row for a
 * hosted one, or both. */
export interface Listed {
  key: string;
  src: ByCardSession | null;
  wave: SessionRow | null;
}

export interface Group {
  card: ByCardCard | null;
  sessions: Listed[];
}

/** States that mean a person is waited on. */
const NEEDS = new Set(["needs_input", "waiting", "asks", "permission"]);
/** States a session has finished in: it does nothing more on its own. */
const FINISHED = new Set(["done", "failed", "stopped", "ended", "exited", "closed", "finished", "merged"]);

/** Its state as one word: a hosted session's (the wave's first), an
 * agent's, or a machine session's busiest pane's. */
export function stateOfListed(s: Listed): string {
  if (s.wave) return s.wave.state;
  const src = s.src!;
  if (src.kind !== "machine") return src.state ?? "";
  const states = src.panes.map((p) => p.state ?? "");
  return states.find((x) => NEEDS.has(x)) ?? states.find((x) => x && !FINISHED.has(x)) ?? states.find(Boolean) ?? "";
}

/** What waits on you, in a few words, or null. */
export function waitsOn(s: Listed): string | null {
  if (s.wave) return s.wave.waiting;
  const src = s.src!;
  if (src.kind === "machine") {
    const n = src.panes.filter((p) => p.state && NEEDS.has(p.state)).length;
    return n === 0 ? null : n === 1 ? "a pane's question" : `${n} panes' questions`;
  }
  return src.state && NEEDS.has(src.state) ? "its question" : null;
}

/** Whether it has finished: it ended, and nothing waits on you. A machine
 * session with no pane state known is going. */
export function finished(s: Listed): boolean {
  if (waitsOn(s)) return false;
  const src = s.src;
  if (src?.kind === "machine" && !s.wave) return src.panes.length > 0 && src.panes.every((p) => !!p.state && FINISHED.has(p.state));
  return FINISHED.has(stateOfListed(s));
}

/** When it last changed, for the order in a group: newest first. */
const at = (s: Listed) => Math.max(s.src?.updated_at ?? 0, s.wave?.pushed_at ?? 0);

/**
 * The page's groups. `byCard` null is a cloud from before the by-card
 * route: the wave alone, grouped by each session's issue. `narrowed` (a
 * label, a tracker or who asked) narrows to the wave's sessions, since only
 * the wave knows those of a session; `going` leaves finished ones out.
 * Groups that need you come first, then the rest as the cloud sorted them
 * (by card number, then cards that aren't issues); Not on a card is last.
 */
export function groupsOf(byCard: SessionsByCard | null, wave: SessionWave | null, f: { narrowed: boolean; going: boolean }): Group[] {
  const rows = new Map((wave?.sessions ?? []).map((r) => [r.session, r]));
  const used = new Set<string>();
  const groups = new Map<string, Group>();
  const groupOf = (card: ByCardCard | null) => {
    const k = card?.id ?? "";
    let g = groups.get(k);
    if (!g) groups.set(k, (g = { card, sessions: [] }));
    else if (card && g.card && card.number !== undefined && g.card.number === undefined) g.card = card;
    return g;
  };
  for (const g of byCard?.groups ?? []) {
    const group = groupOf(g.card);
    for (const src of g.sessions) {
      const row = src.kind === "hosted" ? (rows.get(src.id) ?? null) : null;
      if (row) used.add(row.session);
      if (f.narrowed && !row) continue;
      group.sessions.push({ key: `${src.kind}:${src.kind === "agent" && src.on.kind === "machine" ? `${src.on.id}.` : ""}${src.id}`, src, wave: row });
    }
  }
  // Hosted sessions the by-card answer hasn't (or it isn't there yet): by
  // their issue, as the wave says.
  for (const r of wave?.sessions ?? []) {
    if (used.has(r.session)) continue;
    groupOf(r.issue ? { id: r.issue.id, number: r.issue.number, title: r.issue.title } : null).sessions.push({ key: `hosted:${r.session}`, src: null, wave: r });
  }
  const out = [...groups.values()]
    .map((g) => ({ ...g, sessions: g.sessions.filter((s) => !f.going || !finished(s)).sort((a, b) => at(b) - at(a)) }))
    .filter((g) => g.sessions.length > 0);
  // The cloud's order for its own groups; the wave's own, by number, after.
  const cloud = new Map((byCard?.groups ?? []).map((g, i) => [g.card?.id ?? "", i]));
  const place = (g: Group) => cloud.get(g.card?.id ?? "") ?? Infinity;
  const needs = (g: Group) => g.sessions.some((s) => !!waitsOn(s));
  return out.sort(
    (a, b) =>
      Number(!a.card) - Number(!b.card) ||
      Number(needs(b)) - Number(needs(a)) ||
      place(a) - place(b) ||
      (a.card?.number ?? Infinity) - (b.card?.number ?? Infinity) ||
      (a.card?.id ?? "").localeCompare(b.card?.id ?? ""),
  );
}

/** A group's heading: `#985 Title`, a card that isn't an issue by its
 * board row's words or its id, or Not on a card. */
export function cardTitle(card: ByCardCard | null, rowWords?: string): string {
  if (!card) return "Not on a card";
  if (card.number !== undefined) return `#${card.number}${card.title ? ` ${card.title}` : ""}`;
  return card.title || rowWords || card.id;
}

/** The panes a session shows when its row opens: a machine session's,
 * or an agent's own pane. A hosted one has none here. */
export function panesOf(src: ByCardSession | null): { machine: string; pane: number; kind: string; state: string | null }[] {
  if (!src || src.on.kind !== "machine") return [];
  const machine = src.on.id;
  if (src.kind === "machine") return src.panes.map((p) => ({ machine, pane: p.pane, kind: p.kind, state: p.state }));
  if (src.kind === "agent") return [{ machine, pane: src.id, kind: "agent", state: src.state }];
  return [];
}
