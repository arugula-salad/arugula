// The Steps view's words and arithmetic (Arugula #763): phases (the board's
// groups) with their rows by place (a done row stays where its links put
// it, #824), a row's links as arrows, and a reorder as the move the
// platform takes. Plain functions, for the view and
// its tests (scripts/boards.test.mjs).

import type { BoardGroup, BoardRow, BoardView, RelationKind } from "../proto";

/** The view a viewer last picked, kept in this browser. */
const VIEW_KEY = "arugula.board.view";

export type BoardMode = "lanes" | "steps";

/** The width at which Steps is the default, in CSS pixels (a phone). */
export const STEPS_DEFAULT_WIDTH = 390;

/** Column 1 shows this many rows, then folds behind "+M more". */
export const STEP_ONE_SHOWN = 8;

/** The view this viewer picked, else Steps on a phone and Lanes elsewhere. Storage that refuses reads as no pick. */
export function loadMode(width: number = window.innerWidth): BoardMode {
  try {
    const v = localStorage.getItem(VIEW_KEY);
    if (v === "lanes" || v === "steps") return v;
  } catch {
    // No storage here: the default.
  }
  return width <= STEPS_DEFAULT_WIDTH ? "steps" : "lanes";
}

export function saveMode(mode: BoardMode): void {
  try {
    localStorage.setItem(VIEW_KEY, mode);
  } catch {
    // Not kept; the pick holds for this page.
  }
}

/** Whether done rows are drawn, kept in this browser ("0": hidden). */
const DONE_KEY = "arugula.board.steps.done";

/** Whether done rows are shown: yes until the viewer hides them. Storage that refuses reads as shown. */
export function loadDone(): boolean {
  try {
    return localStorage.getItem(DONE_KEY) !== "0";
  } catch {
    return true;
  }
}

export function saveDone(show: boolean): void {
  try {
    localStorage.setItem(DONE_KEY, show ? "1" : "0");
  } catch {
    // Not kept; the pick holds for this page.
  }
}

const LINKS_KEY = "arugula.board.steps.links";

/** Whether every arrow is drawn: yes until the viewer turns them off. Storage that refuses reads as on. */
export function loadLinks(): boolean {
  try {
    return localStorage.getItem(LINKS_KEY) !== "0";
  } catch {
    return true;
  }
}

export function saveLinks(show: boolean): void {
  try {
    localStorage.setItem(LINKS_KEY, show ? "1" : "0");
  } catch {
    // Not kept; the pick holds for this page.
  }
}

/** The title column's width, kept in this browser. */
const TITLE_KEY = "arugula.board.steps.title";

/** The title column's width when the viewer hasn't set one, and the least and most it takes (the most is a share of the chart's width). */
export const TITLE_DEFAULT = { desktop: 260, phone: 150 };
export const TITLE_MIN = { desktop: 160, phone: 120 };
export const TITLE_MAX_SHARE = 0.6;

/** The width the viewer set, else null. Storage that refuses, or holds something else, reads as none. */
export function loadTitle(): number | null {
  try {
    const v = Number(localStorage.getItem(TITLE_KEY));
    return Number.isFinite(v) && v > 0 ? v : null;
  } catch {
    return null;
  }
}

/** Keep the width (null: forget it, so the default holds again). */
export function saveTitle(width: number | null): void {
  try {
    if (width === null) localStorage.removeItem(TITLE_KEY);
    else localStorage.setItem(TITLE_KEY, String(Math.round(width)));
  } catch {
    // Not kept; the width holds for this page.
  }
}

/** The least and most the title column may be in a chart `chart` pixels wide (0: not measured yet, so no most). */
export function titleRange(chart: number, phone: boolean): [number, number] {
  const min = phone ? TITLE_MIN.phone : TITLE_MIN.desktop;
  return [min, chart > 0 ? Math.max(min, Math.floor(chart * TITLE_MAX_SHARE)) : Number.POSITIVE_INFINITY];
}

/** `width` held to the range. */
export function clampTitle(width: number, chart: number, phone: boolean): number {
  const [min, max] = titleRange(chart, phone);
  return Math.min(max, Math.max(min, Math.round(width)));
}

/** What a reorder refused as stale says on the moved row. */
export const MOVED_FIRST = "moved first by someone else";

/** A phase: a group (none: the rows with no group) with its rows by place and the count of its done rows. */
export interface Phase {
  group: BoardGroup | null;
  /** The count of done rows. */
  done: number;
  /** Rows by place (the chart's column), done or open. Inside a place the done rows come first, then the open rows in the order the board holds them. */
  places: { place: number; rows: BoardRow[] }[];
}

/** Every row of the view, flat. */
export const rowsOf = (v: Pick<BoardView, "zones">): BoardRow[] => v.zones.flatMap((z) => z.rows);

/** The group a row sits in, or null: a row of a group the board no longer has sits in none, as `sections` has it. */
function groupOf(v: Pick<BoardView, "groups">, r: Pick<BoardRow, "group">): string | null {
  return r.group && (v.groups ?? []).some((g) => g.id === r.group) ? r.group : null;
}

/** A row's step: the platform's, else 1 (a note has no item and so no step; it is still open work). */
export const stepOf = (r: Pick<BoardRow, "step">): number => r.step ?? 1;

/** The column a row is drawn in (#824): its place, which counts done rows and closed links so finishing work moves nothing; else its step. */
export const placeOf = (r: Pick<BoardRow, "place" | "step">): number => r.place ?? stepOf(r);

/** The row ids of group `group` (null: no group) in the order the board stores them. Empty when the platform doesn't say (`order`). */
export function groupOrder(v: Pick<BoardView, "groups" | "zones" | "order">, group: string | null): string[] {
  const rows = new Map(rowsOf(v).map((r) => [r.id, r]));
  return (v.order ?? []).filter((id) => {
    const r = rows.get(id);
    return r && groupOf(v, r) === group;
  });
}

/** The phases of a board: its groups in order, then the rows with none. Inside a place the done rows come first, then the open rows keep the order the board holds, which a person's drag changes (the platform's `sequence` puts what unblocks most first, which a drag couldn't move). */
export function phases(v: Pick<BoardView, "groups" | "zones" | "order" | "sequence">): Phase[] {
  const all = rowsOf(v);
  const index = new Map((v.order?.length ? v.order : (v.sequence ?? [])).map((id, i) => [id, i]));
  const at = (r: BoardRow) => index.get(r.id) ?? Number.MAX_SAFE_INTEGER;
  const groups = v.groups ?? [];
  const slots: (BoardGroup | null)[] = [...groups, null];
  const out: Phase[] = [];
  for (const g of slots) {
    const mine = all.filter((r) => groupOf(v, r) === (g?.id ?? null));
    if (g === null && mine.length === 0) continue;
    const byPlace = new Map<number, BoardRow[]>();
    const done = (r: BoardRow) => (r.lane === "done" ? 0 : 1);
    for (const r of [...mine].sort((a, b) => done(a) - done(b) || at(a) - at(b))) byPlace.set(placeOf(r), [...(byPlace.get(placeOf(r)) ?? []), r]);
    out.push({
      group: g,
      done: mine.filter((r) => r.lane === "done").length,
      places: [...byPlace.entries()].sort((a, b) => a[0] - b[0]).map(([place, rows]) => ({ place, rows })),
    });
  }
  return out;
}

/** The phases without their done rows (#831), a place left with no row dropped; each phase's `done` still counts the rows hidden. */
export function withoutDone(ps: Phase[]): Phase[] {
  return ps.map((p) => ({ ...p, places: p.places.map((s) => ({ ...s, rows: s.rows.filter((r) => r.lane !== "done") })).filter((s) => s.rows.length > 0) }));
}

/** The column of each place drawn (#831): the places in order, numbered 1…k, so a place no row is drawn in leaves no empty column. When every place from 1 is used it is the identity. */
export function columns(places: number[]): Map<number, number> {
  return new Map([...new Set(places)].sort((a, b) => a - b).map((p, i) => [p, i + 1]));
}

/** The highest place of any row. */
export const lastPlace = (ps: Phase[]): number => Math.max(1, ...ps.flatMap((p) => p.places.map((s) => s.place)));

/** A column's rows as shown: column 1 shows `STEP_ONE_SHOWN` of them until it is unfolded; the rest are `more`. */
export function shown<T>(place: number, rows: T[], unfolded: boolean): { rows: T[]; more: number } {
  if (place !== 1 || unfolded || rows.length <= STEP_ONE_SHOWN) return { rows, more: 0 };
  return { rows: rows.slice(0, STEP_ONE_SHOWN), more: rows.length - STEP_ONE_SHOWN };
}

/** "+M more". */
export const moreLine = (n: number): string => `+${n} more`;

/** A move for the platform: the row goes before `before` (null: last in its group), in the group whose rows are `seen`. */
export interface Reorder {
  before: string | null;
  seen: string[];
}

/** Where a row of a column goes when it is dropped before `target` in it (`step`: the ids of the column's open rows), or last in it when `target` is null. `group` is its group's rows as stored. Null when that leaves the group as it is. Only the position inside the group changes: the step is worked out again from the links. */
export function reorder(group: string[], step: string[], id: string, target: string | null): Reorder | null {
  const rest = step.filter((s) => s !== id);
  if (target !== null && !rest.includes(target)) return null;
  let before: string | null = target;
  if (before === null) {
    const last = rest[rest.length - 1];
    if (last === undefined) return null;
    // The next row of the group after the step's last, wherever its step is.
    const others = group.filter((g) => g !== id);
    before = others[others.indexOf(last) + 1] ?? null;
  }
  const others = group.filter((g) => g !== id);
  const to = before === null ? others.length : others.indexOf(before);
  if (to < 0) return null;
  const next = [...others.slice(0, to), id, ...others.slice(to)];
  if (next.every((g, i) => g === group[i])) return null;
  return { before, seen: group };
}

/** A row one place up or down among its column's open rows (the drawer's ↑ and ↓), or null at the end. */
export function nudge(group: string[], step: string[], id: string, up: boolean): Reorder | null {
  const i = step.indexOf(id);
  if (i < 0) return null;
  if (up) return i === 0 ? null : reorder(group, step, id, step[i - 1]);
  return i === step.length - 1 ? null : reorder(group, step, id, step[i + 2] ?? null);
}

/** One arrow: a link of kind `kind` between two rows drawn, `from` to `to` (the way an arrow head points, where it has one). */
export interface Link {
  key: string;
  kind: RelationKind;
  from: string;
  to: string;
}

/** The id of the issue or mirrored item a row is on. */
const itemOf = (r: Pick<BoardRow, "pointer">): string | undefined => (r.pointer.kind === "issue" || r.pointer.kind === "forge_item" ? r.pointer.id : undefined);

/** The links of row `r` to rows among `drawn` (the ids on screen). An edge that runs out of `r` goes from it; `before` and `blocks` run from the row that goes first, and `discovered_from` from where the work was found. `relates` and `duplicate_of` have no way, drawn as they were written. */
export function linksOf(r: BoardRow, rows: BoardRow[], drawn: Set<string>): Link[] {
  const byItem = new Map(rows.flatMap((x) => (itemOf(x) ? [[itemOf(x)!, x] as const] : [])));
  const out: Link[] = [];
  for (const x of r.relations ?? []) {
    const other = x.item ? byItem.get(x.item) : undefined;
    if (!other || other.id === r.id || !drawn.has(other.id)) continue;
    // `out` reads "this row <kind> the other": before, blocks and duplicate_of run from this row; "this was discovered from the other" runs from the other.
    const forward = x.kind === "discovered_from" ? x.direction === "in" : x.direction === "out";
    const [from, to] = forward ? [r.id, other.id] : [other.id, r.id];
    out.push({ key: x.id, kind: x.kind, from, to });
  }
  return out;
}

/** Where row `id` sits: its group's rows as stored (for `seen`) and the ids of the open rows of its column in the order shown. Null for a row that can't be moved: a done row, or one the view has no `order` for. */
export function stepPlace(v: Pick<BoardView, "groups" | "zones" | "order" | "sequence">, id: string): { group: string[]; step: string[] } | null {
  for (const p of phases(v)) {
    for (const s of p.places) {
      if (!s.rows.some((r) => r.id === id)) continue;
      const open = s.rows.filter((r) => r.lane !== "done");
      if (!open.some((r) => r.id === id)) return null;
      const group = groupOrder(v, p.group?.id ?? null);
      return group.length ? { group, step: open.map((r) => r.id) } : null;
    }
  }
  return null;
}
