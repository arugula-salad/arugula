// What a board says (live status board, step 2): a row's glyph, a zone's
// progress, a pointer's label and the id a person typed as a pointer.
// Plain functions, for the view and its tests (scripts/boards.test.mjs).

import type { BoardBlockState, BoardBlockWord, BoardProgress, BoardLive, BoardLiveState, BoardPointer, BoardRow, BoardStatus, BoardWorkItem, BoardZone } from "../proto";

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
  running: "running",
  succeeded: "succeeded",
  run_failed: "failed",
  other: "not run",
};

/** The classes a live tag takes: the forge blocks' state colours, and
 * `waits` and `bad` for checks and runs. */
export function liveClass(s: BoardLiveState): string {
  if (s === "checks_running" || s === "running") return "waits";
  if (s === "checks_failed" || s === "run_failed") return "bad";
  if (s === "succeeded") return "forge-state merged";
  if (s === "other") return "";
  return `forge-state ${s}`;
}

/** Whether a row proves what it says (it matters in a `proofs` zone): a
 * link, a run that succeeded, or a PR that is merged. Derived from what the
 * platform read, never stored over the row's status. */
export function proven(r: Pick<BoardRow, "pointer" | "live">): boolean {
  switch (r.pointer.kind) {
    case "link":
      return true;
    case "run":
      return r.live?.state === "succeeded";
    case "forge_item":
      return r.live?.state === "merged";
    default:
      return false;
  }
}

/** What a machine reported of an agent block, as a tag. */
export const BLOCK_LABEL: Record<BoardBlockWord, string> = {
  working: "working",
  needs_input: "needs input",
  idle: "idle",
  closed: "closed",
};

/** The class a block's state tag takes: `waits` when it wants someone. */
export function blockClass(w: BoardBlockWord): string {
  return w === "needs_input" ? "waits" : "";
}

/** The rest of a block's report, each fact short: `branch@head`, pushed or
 * unpushed, the uncommitted files and the diffstat (`3 files, +40 −2`). */
export function blockFacts(b: BoardBlockState): string[] {
  const out: string[] = [];
  if (b.branch && b.head) out.push(`${b.branch}@${b.head}`);
  else if (b.branch || b.head) out.push((b.branch ?? b.head)!);
  if (b.pushed != null) out.push(b.pushed ? "pushed" : "unpushed");
  if (b.uncommitted) out.push(`${b.uncommitted} uncommitted`);
  const d = b.diffstat;
  if (d) out.push(`${d.files} file${d.files === 1 ? "" : "s"}, +${d.added} −${d.removed}`);
  return out;
}

/** A zone's rows that are done, of all its rows. */
export function progress(z: Pick<BoardZone, "rows">): { done: number; total: number; pct: number } {
  const total = z.rows.length;
  const done = z.rows.filter((r) => r.status === "done").length;
  return { done, total, pct: total ? Math.round((done / total) * 100) : 0 };
}

/** The longest goal the platform takes (`GOAL_MAX`, control-wire). */
export const GOAL_MAX = 2000;

/** A text's first line that says something: a goal or a done note on a list. */
export function firstLine(text: string): string {
  return text.split("\n").map((l) => l.trim()).find(Boolean) ?? "";
}

/** A board's issues that are closed, in words; `pct` is for its bar. */
export function epicLine(p: Pick<BoardProgress, "done" | "total">): { words: string; pct: number } {
  if (!p.total) return { words: "No issues yet", pct: 0 };
  return { words: `${p.done} of ${p.total} issue${p.total === 1 ? "" : "s"} done`, pct: Math.round((p.done / p.total) * 100) };
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
    case "link":
      return p.url.replace(/^https:\/\//, "");
  }
}

/** Whether a row reads as a card: it points at an issue or a PR the
 * platform read (a run's `live` stays a link and a tag). */
export function isCard(live?: BoardLive | null): live is BoardLive {
  return !!live && live.kind !== "run";
}

/** Who a card is assigned to, comma-separated: three names, then `+N`. */
export function assigneesLine(who?: string[] | null): string {
  const list = (who ?? []).filter(Boolean);
  if (list.length <= 3) return list.join(", ");
  return `${list.slice(0, 2).join(", ")} +${list.length - 2}`;
}

/** The pointer as the id a person edits: none for a block's, which isn't typed. */
export function pointerText(p: BoardPointer): string {
  return p.kind === "none" || p.kind === "block" ? "" : p.kind === "link" ? p.url : p.id;
}

/** An id a person typed as a pointer (`fi_…`, `iss_…`, `run_…`, `hw_…`) or
 * an `https://` link, or null when it isn't one; empty is no pointer. */
export function parsePointer(text: string): BoardPointer | null {
  const t = text.trim();
  if (!t) return { kind: "none" };
  if (/^https:\/\/[^\s\u0000-\u001f\u007f]+$/.test(t)) return { kind: "link", url: t };
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

/** How many results the "points at" picker lists. */
export const PICK_LIMIT = 8;

/** How long the picker waits after a keystroke before it searches. */
export const PICK_DELAY_MS = 250;

/** A result's source label: `OWNER/NAME#N` for a mirrored item, `#N` for a
 * native issue, the run's own title for a run. */
export function workLabel(w: Pick<BoardWorkItem, "source" | "repo" | "number" | "title">): string {
  if (w.source === "run" || w.number == null) return w.title;
  return w.repo ? `${w.repo}#${w.number}` : `#${w.number}`;
}

/** Call `fn` once, `ms` after the last call. `cancel` drops a pending one. */
export function debounced<A extends unknown[]>(fn: (...a: A) => void, ms: number): ((...a: A) => void) & { cancel: () => void } {
  let t: ReturnType<typeof setTimeout> | undefined;
  const run = (...a: A) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...a), ms);
  };
  run.cancel = () => clearTimeout(t);
  return run;
}

/** The active result after an arrow key: down and up wrap round, and -1 is
 * none (the field itself). */
export function stepActive(active: number, count: number, down: boolean): number {
  if (count <= 0) return -1;
  if (down) return active + 1 >= count ? 0 : active + 1;
  return active <= 0 ? count - 1 : active - 1;
}

/** What a row's form sends as the pointer: a picked result's, else the typed
 * text's when it differs from `start` (the row's own), else none, so a note
 * edit leaves a link or an agent block's pointer alone. Null when the typed
 * text isn't a pointer. */
export function pointerToSend(text: string, start: string, picked: BoardPointer | null): { pointer?: BoardPointer } | null {
  if (picked) return { pointer: picked };
  const pointer = parsePointer(text);
  if (!pointer) return null;
  return text.trim() !== start ? { pointer } : {};
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
  if (p.kind === "link") return /^https:\/\//.test(p.url) ? p.url : undefined;
  return undefined;
}

/** The `before` that moves entry `i` of `ids` one place `up` or down: the
 * sibling it goes before (none: last), or null when it can't move. */
export function stepBefore(ids: string[], i: number, up: boolean): { before: string | null } | null {
  if (up) return i > 0 ? { before: ids[i - 1] } : null;
  if (i >= ids.length - 1) return null;
  return { before: ids[i + 2] ?? null };
}
