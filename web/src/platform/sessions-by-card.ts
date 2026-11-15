// A project's sessions by the card each works on (Arugula inversion L3):
// the cloud's `GET /api/projects/{p}/sessions/by-card`, every session in
// the project wherever it runs (a machine's, an agent's pane, a hosted
// one), put together with the hosted wave (`/sessions`), which says more of
// a hosted session (its branch and push, what waits on you) and carries
// the filters. Pure, so scripts/sessions-by-card.test.mjs reads it.

import type { SessionRow, SessionWave } from "../shared/proto.gen";
import { finishedStates, statesOf, waitsOnYou, type ProjectSession, type SessionCard, type SessionsByCard } from "../shared/project-sessions.ts";

/** A session on the page: the by-card answer's, the wave's row for a
 * hosted one, or both. */
export interface Listed {
  key: string;
  src: ProjectSession | null;
  wave: SessionRow | null;
}

export interface Group {
  card: SessionCard | null;
  sessions: Listed[];
}

/** Its state as one word: a hosted session's (the wave's first), an
 * agent's, or a machine session's busiest pane's. */
export function stateOfListed(s: Listed): string {
  if (s.wave) return s.wave.state;
  const states = statesOf(s.src!);
  return states.find(waitsOnYou) ?? states.find((x) => x && !finishedStates([x])) ?? states.find(Boolean) ?? "";
}

/** What waits on you, in a few words, or null. */
export function waitsOn(s: Listed): string | null {
  if (s.wave) return s.wave.waiting;
  const src = s.src!;
  if (src.kind === "machine") {
    const n = src.panes.filter((p) => waitsOnYou(p.state)).length;
    return n === 0 ? null : n === 1 ? "a pane's question" : `${n} panes' questions`;
  }
  return waitsOnYou(src.state) ? "its question" : null;
}

/** Whether it has finished: it ended, and nothing waits on you. A hosted
 * session's state is the wave's when it has one. */
export function finished(s: Listed): boolean {
  if (waitsOn(s)) return false;
  return finishedStates(s.wave ? [s.wave.state] : statesOf(s.src!));
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
  const groupOf = (card: SessionCard | null) => {
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
export function cardTitle(card: SessionCard | null, rowWords?: string): string {
  if (!card) return "Not on a card";
  if (card.number !== undefined) return `#${card.number}${card.title ? ` ${card.title}` : ""}`;
  return card.title || rowWords || card.id;
}

/** The panes a session shows when its row opens: a machine session's,
 * or an agent's own pane. A hosted one has none here. */
export function panesShown(src: ProjectSession | null): { machine: string; pane: number; kind: string; state: string | null }[] {
  if (!src || src.on.kind !== "machine") return [];
  const machine = src.on.id;
  if (src.kind === "machine") return src.panes.map((p) => ({ machine, pane: p.pane, kind: p.kind, state: p.state }));
  if (src.kind === "agent") return [{ machine, pane: src.id, kind: "agent", state: src.state }];
  return [];
}

/** A session's branch, as the by-card answer says it: its own, else its
 * first pane's that says one; null when it isn't said (a cloud from
 * before the field, or a session on none). */
export function branchOf(src: ProjectSession | null): string | null {
  if (!src) return null;
  if (src.branch) return src.branch;
  if (src.kind === "machine") return src.panes.map((p) => p.branch).find((b) => !!b) ?? null;
  return null;
}

const LANE: Record<string, string> = { next: "Next", doing: "Doing", review: "Review", done: "Done" };

/** Where a card sits, as the by-card answer says it: its board and its
 * lane's title (empty when it has none). Null when it's on no board, or
 * when the answer doesn't say (a cloud from before the fields). */
export function cardPlace(card: SessionCard | null): { board: string; boardTitle: string; lane: string } | null {
  if (!card?.board) return null;
  return { board: card.board.id, boardTitle: card.board.title, lane: card.lane ? LANE[card.lane] : "" };
}

/** Whether the page must read the project's boards to place its groups:
 * the wave alone (no by-card answer), or an answer whose cards don't say
 * their board (a cloud from before the field). */
export function readsBoards(byCard: SessionsByCard | null): boolean {
  return !byCard || byCard.groups.some((g) => !!g.card && g.card.board === undefined);
}

// Putting a session on a card (Arugula #1019): the page moves it at once,
// then asks the cloud, which asks its machine. The by-card answer says so
// only once the machine has reported, so the move is kept over each read
// until a read agrees with it, or for MOVE_KEPT_MS; a refusal takes it back.

/** A session put on a card here, ahead of the by-card answer. `to` null is
 * Not on a card (back to what its branch gives); `from` is where it was. */
export interface Move {
  session: ProjectSession;
  from: SessionCard | null;
  to: SessionCard | null;
  /** When it was put there, in ms. */
  at: number;
}

/** How long a move is kept with no read agreeing: then the cloud's answer is the page's again. */
export const MOVE_KEPT_MS = 120_000;

/** Which session a move is: its kind, its machine and its id. */
export const moveKey = (s: ProjectSession): string => `${s.kind}:${s.on.kind === "machine" ? s.on.id : ""}:${s.id}`;

/** Only a machine's session is put on a card from here: a hosted one's card
 * is its issue's, an agent's its block's own. */
export const movable = (s: ProjectSession | null): s is Extract<ProjectSession, { kind: "machine" }> => !!s && s.kind === "machine" && s.on.kind === "machine";

/** The card group a read has the session in: undefined when it isn't in the answer. */
function cardIn(byCard: SessionsByCard, key: string): SessionCard | null | undefined {
  for (const g of byCard.groups) if (g.sessions.some((s) => moveKey(s) === key)) return g.card;
  return undefined;
}

/** The answer with each move made: the session taken out of its group and
 * put in its card's, which is made (last, before Not on a card) when no
 * session was on it. Groups left empty go. A cloud without the by-card
 * answer (null) moves nothing. */
export function withMoves(byCard: SessionsByCard | null, moves: Move[]): SessionsByCard | null {
  if (!byCard || moves.length === 0) return byCard;
  let groups = byCard.groups.map((g) => ({ ...g, sessions: [...g.sessions] }));
  for (const m of moves) {
    const key = moveKey(m.session);
    let session = m.session;
    for (const g of groups) {
      const i = g.sessions.findIndex((s) => moveKey(s) === key);
      if (i >= 0) [session] = g.sessions.splice(i, 1);
    }
    const id = m.to?.id ?? null;
    let to = groups.find((g) => (g.card?.id ?? null) === id);
    if (!to) {
      to = { card: m.to, sessions: [] };
      const none = groups.findIndex((g) => !g.card);
      if (m.to && none >= 0) groups.splice(none, 0, to);
      else groups.push(to);
    }
    to.sessions.push(session);
  }
  groups = groups.filter((g) => g.sessions.length > 0);
  return { ...byCard, groups };
}

/** The moves still to keep over a new read: not those it agrees with (the
 * session on the card it was put on; for Not on a card, off the one it was
 * on, since its branch may give it another), nor those older than
 * MOVE_KEPT_MS. */
export function keptMoves(byCard: SessionsByCard | null, moves: Move[], now: number): Move[] {
  return moves.filter((m) => {
    if (now - m.at > MOVE_KEPT_MS) return false;
    if (!byCard) return true;
    const at = cardIn(byCard, moveKey(m.session));
    if (at === undefined) return true;
    return m.to ? at?.id !== m.to.id : at?.id === m.from?.id;
  });
}

/** The moves without one the cloud refused: the session goes back. */
export const undoMove = (moves: Move[], m: Move): Move[] => moves.filter((x) => x !== m);
