// A board drawn as its workers (#466): the swarm narrowed to one board's
// agents, clustered by the lane of the row each works on. Plain functions
// from the panes and the board view the page loaded, for the view and its
// test (scripts/board-focus.test.mjs).

import type { BoardLane, BoardRow, BoardView, OnIssue } from "../../shared/proto";
import { pointerLabel, isCard } from "../../shared/boards/text.ts";

/** What a pane tells of itself: where it is, and the boards its agent is on. */
export interface FocusPane {
  /** `host:pane`. */
  key: string;
  host: string;
  id: number;
  on_issue?: OnIssue | null;
}

/** One lane of the board, as a cluster. */
export interface FocusLane {
  lane: BoardLane;
  /** The lane's name as the board page shows it: the cluster's name. */
  name: string;
  rows: number;
  /** The panes in it, by key. */
  keys: string[];
  /** `Doing · 3 rows · 2 workers`, or `Review · 1 row · no one on it`. */
  label: string;
}

/** Where a worker is on the board. */
export interface Placed {
  name: string;
  row: BoardRow;
}

export interface BoardFocus {
  /** The lanes that have rows, in the board's order. */
  lanes: FocusLane[];
  /** A pane's key to its place; a pane that works on none of the board's rows is not in it. */
  placed: Map<string, Placed>;
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** The board's workers by lane. A pane works on a row when its agent's `on_issue.boards` names the board and the row, or when a work attachment of the row points at its block (`machine` is a daemon's id, which `hostOf` turns into a host's name). When both say where, the attachment does. A lane with no rows doesn't show; one with rows and no workers does. */
export function boardFocus(view: Pick<BoardView, "board" | "zones">, panes: FocusPane[], hostOf: (machine: string) => string | undefined): BoardFocus {
  const rows = new Map<string, Placed>();
  for (const z of view.zones) for (const r of z.rows) rows.set(r.id, { name: z.title, row: r });
  const placed = new Map<string, Placed>();
  for (const p of panes) {
    const row = p.on_issue?.boards?.find((b) => b.board === view.board.id)?.row;
    const at = row && rows.get(row);
    if (at) placed.set(p.key, at);
  }
  const byBlock = new Map(panes.map((p) => [`${p.host}:${p.id}`, p.key]));
  for (const z of view.zones) {
    for (const r of z.rows) {
      for (const a of r.attachments) {
        if (a.role !== "work" || a.released || a.pointer.kind !== "block") continue;
        const host = hostOf(a.pointer.machine);
        const key = host && byBlock.get(`${host}:${a.pointer.pane.replace(/^%/, "")}`);
        if (key) placed.set(key, { name: z.title, row: r });
      }
    }
  }
  const lanes = view.zones
    .filter((z) => z.rows.length)
    .map((z): FocusLane => {
      const keys = [...placed].filter(([, at]) => at.name === z.title).map(([k]) => k);
      const workers = keys.length ? count(keys.length, "worker", "workers") : "no one on it";
      return { lane: z.rows[0].lane, name: z.title, rows: z.rows.length, keys, label: `${z.title} · ${count(z.rows.length, "row", "rows")} · ${workers}` };
    });
  return { lanes, placed };
}

/** The most of a row's title a tile's chip shows. */
const TITLE_MAX = 24;

/** A tile's chip in a board's focus: the row's issue and a short title, `#12 · Fix the installer`. A note or a block has no issue, so its text stands alone. */
export function rowChip(r: Pick<BoardRow, "pointer" | "live" | "note">): string {
  const head = r.pointer.kind === "issue" || r.pointer.kind === "forge_item" ? pointerLabel(r.pointer, r.live) : "";
  const title = isCard(r.live) ? r.live.title : r.note.split("\n").map((l) => l.trim()).find(Boolean) ?? "";
  const short = title.length > TITLE_MAX ? `${title.slice(0, TITLE_MAX - 1).trimEnd()}…` : title;
  return [head, short].filter(Boolean).join(" · ") || "a row";
}
