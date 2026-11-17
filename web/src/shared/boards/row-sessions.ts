// A card's sessions on its board row (Arugula #1082, which folded the
// Sessions page away): the hub's sessions by card (`sessions/by-card`)
// that work on the row's card, less those the row shows already (an agent
// block attached to it, a hosted session started on its issue) and those
// that have finished. Pure, so scripts/row-sessions.test.mjs reads it.

import type { BoardRow } from "../proto";
import { finishedStates, statesOf, stateStanding, waitsOnYou, type ProjectSession, type SessionsByCard } from "../project-sessions.ts";

/** A session the row draws: what to call it, where it runs, its state in
 * a word, whether it waits on you, and the pane it opens on (a machine's)
 * or its session page (a hosted one's id). */
export interface RowSession {
  key: string;
  name: string;
  where: string | null;
  state: string;
  needs: boolean;
  open: { kind: "pane"; machine: string; pane: string } | { kind: "hosted"; session: string } | null;
}

/** Whether a by-card group's card is this row's: its issue, or the row itself. */
const isRow = (r: Pick<BoardRow, "id" | "pointer">, card: string): boolean => card === r.id || (r.pointer.kind === "issue" && r.pointer.id === card);

/** Its state as one word: what waits on you, else what isn't over, else any. */
function stateWord(s: ProjectSession): string {
  const states = statesOf(s);
  return states.find(waitsOnYou) ?? states.find((x) => x && !finishedStates([x])) ?? states.find(Boolean) ?? "";
}

/** The pane it opens on: an agent's own; a machine session's that waits on you, else its first. */
function paneOf(s: ProjectSession): RowSession["open"] {
  if (s.kind === "hosted") return { kind: "hosted", session: s.id };
  if (s.on.kind !== "machine") return null;
  if (s.kind === "agent") return { kind: "pane", machine: s.on.id, pane: String(s.id) };
  const p = s.panes.find((x) => waitsOnYou(x.state)) ?? s.panes[0];
  return p ? { kind: "pane", machine: s.on.id, pane: String(p.pane) } : null;
}

const RANK = { needs: 0, failed: 1, working: 2, quiet: 3 } as const;

/** The row's card's sessions that it doesn't show yet, what needs you first. */
export function rowSessions(r: Pick<BoardRow, "id" | "pointer" | "attachments"> & Partial<Pick<BoardRow, "sessions">>, byCard: SessionsByCard | null): RowSession[] {
  const hosted = new Set((r.sessions ?? []).map((x) => x.session));
  const blocks = new Set(r.attachments.flatMap((a) => (a.pointer.kind === "block" ? [`${a.pointer.machine}:${a.pointer.pane.replace(/^%/, "")}`] : [])));
  const out: (RowSession & { rank: number })[] = [];
  for (const g of byCard?.groups ?? []) {
    if (!g.card || !isRow(r, g.card.id)) continue;
    for (const s of g.sessions) {
      if (s.kind === "hosted" && hosted.has(s.id)) continue;
      if (s.kind === "agent" && s.on.kind === "machine" && blocks.has(`${s.on.id}:${s.id}`)) continue;
      const states = statesOf(s);
      const needs = states.some(waitsOnYou);
      if (!needs && finishedStates(states)) continue;
      out.push({
        key: `${s.kind}:${s.on.kind === "machine" ? s.on.id : ""}:${s.id}`,
        name: s.name || (s.kind === "agent" ? "Agent" : "Session"),
        where: s.on.kind === "machine" ? s.on.name : null,
        state: stateWord(s).replace(/_/g, " "),
        needs,
        open: paneOf(s),
        rank: RANK[stateStanding(states)],
      });
    }
  }
  return out.sort((a, b) => a.rank - b.rank).map(({ rank: _r, ...s }) => s);
}

/** The hosted sessions whose live event (`chat.onObject(ses)`, the
 * platform's push as a session's state moves, Arugula #1117) changes what
 * `rows` draw: `board`, those a row holds itself (`sessions`, which the
 * board's own read says), and `cards`, those the by-card list has on a
 * row's card. Finished ones too: a session asked again goes again. */
export function pushedSessions(rows: (Pick<BoardRow, "id" | "pointer"> & Partial<Pick<BoardRow, "sessions">>)[], byCard: SessionsByCard | null): { board: string[]; cards: string[] } {
  const board = rows.flatMap((r) => (r.sessions ?? []).map((x) => x.session));
  const cards = (byCard?.groups ?? []).flatMap((g) => {
    const card = g.card;
    if (!card || !rows.some((r) => isRow(r, card.id))) return [];
    return g.sessions.flatMap((s) => (s.kind === "hosted" ? [s.id] : []));
  });
  return { board: [...new Set(board)].sort(), cards: [...new Set(cards)].sort() };
}
