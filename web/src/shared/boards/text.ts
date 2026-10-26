// What a board says (live status board, step 2): a row's glyph, a zone's
// progress, a pointer's label and the id a person typed as a pointer.
// Plain functions, for the view and its tests (scripts/boards.test.mjs).

import type { BoardLive, BoardLiveState, BoardPointer, BoardRow, BoardStatus, BoardZone } from "../proto";

/** A row's status as its glyph: ○ open (or none), ◐ working, ? needs
 * input, ● done, ✕ failed. */
export const GLYPH: Record<BoardStatus, string> = {
  none: "○",
  open: "○",
  working: "◐",
  needs_input: "?",
  done: "●",
  failed: "✕",
};

/** A status as words. */
export const STATUS_LABEL: Record<BoardStatus, string> = {
  none: "No status",
  open: "Open",
  working: "Working",
  needs_input: "Needs input",
  done: "Done",
  failed: "Failed",
};

export const STATUSES: BoardStatus[] = ["none", "open", "working", "needs_input", "done", "failed"];

/** What a pointer's live state says, as a tag. */
export const LIVE_LABEL: Record<BoardLiveState, string> = {
  open: "open",
  checks_running: "checks running",
  checks_failed: "checks failed",
  merged: "merged",
  closed: "closed",
};

/** The classes a live tag takes: the forge blocks' state colours, and
 * `waits` and `bad` for checks. */
export function liveClass(s: BoardLiveState): string {
  return s === "checks_running" ? "waits" : s === "checks_failed" ? "bad" : `forge-state ${s}`;
}

/** A zone's rows that are done, of all its rows. */
export function progress(z: Pick<BoardZone, "rows">): { done: number; total: number; pct: number } {
  const total = z.rows.length;
  const done = z.rows.filter((r) => r.status === "done").length;
  return { done, total, pct: total ? Math.round((done / total) * 100) : 0 };
}

/** The last characters of an id, for a row that says nothing better. */
const short = (id: string) => id.replace(/^[a-z]+_/, "").slice(0, 8);

/** The words of a row's link: the item's repo and number when the
 * platform read it, else the pointer's kind and a bit of its id. */
export function pointerLabel(p: BoardPointer, live?: BoardLive | null): string {
  switch (p.kind) {
    case "none":
      return "";
    case "forge_item":
    case "issue":
      if (live) return live.repo ? `${live.repo}#${live.number}` : `#${live.number}`;
      return p.kind === "issue" ? `issue ${short(p.id)}` : `PR or issue ${short(p.id)}`;
    case "run":
      return `run ${short(p.id)}`;
    case "workspace":
      return `workspace ${short(p.id)}`;
    case "block":
      return `agent ${p.pane}`;
  }
}

/** The pointer as the id a person edits: none for a block's, which isn't typed. */
export function pointerText(p: BoardPointer): string {
  return p.kind === "none" || p.kind === "block" ? "" : p.id;
}

/** An id a person typed as a pointer (`fi_…`, `iss_…`, `run_…`, `hw_…`),
 * or null when it isn't one; empty is no pointer. */
export function parsePointer(text: string): BoardPointer | null {
  const t = text.trim();
  if (!t) return { kind: "none" };
  const m = /^(fi|iss|run|hw)_[0-9A-Za-z]{1,64}$/.exec(t);
  if (!m) return null;
  switch (m[1]) {
    case "fi":
      return { kind: "forge_item", id: t };
    case "iss":
      return { kind: "issue", id: t };
    case "run":
      return { kind: "run", id: t };
    default:
      return { kind: "workspace", id: t };
  }
}

/** Where a row's link goes: an item on its forge, a native issue or a run on
 * the platform page (`platform`, the paths of ids a host knows how to open). */
export function pointerHref(r: Pick<BoardRow, "pointer" | "live">, platform: (path: string) => string | undefined): string | undefined {
  const p = r.pointer;
  if (p.kind === "forge_item") {
    const l = r.live;
    return l?.repo ? `https://github.com/${l.repo}/${l.kind === "issue" ? "issues" : "pull"}/${l.number}` : undefined;
  }
  if (p.kind === "issue") return platform(`/issues/${p.id}`);
  if (p.kind === "run") return platform(`/runs/${p.id}`);
  return undefined;
}

/** The `before` that moves entry `i` of `ids` one place `up` or down: the
 * sibling it goes before (none: last), or null when it can't move. */
export function stepBefore(ids: string[], i: number, up: boolean): { before: string | null } | null {
  if (up) return i > 0 ? { before: ids[i - 1] } : null;
  if (i >= ids.length - 1) return null;
  return { before: ids[i + 2] ?? null };
}
