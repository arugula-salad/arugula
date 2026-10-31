// What a board says (boards, step 2; lanes, aru-125): a row's glyph, a
// lane's count, a group's sections, a row's marks, an attachment's chip, a pointer's label and the id a person
// typed as a pointer. Plain functions, for the view and its tests
// (scripts/boards.test.mjs).

import type { BoardAsk, BoardAttachment, BoardBlockState, BoardBlockWord, BoardGroup, BoardLane, BoardMark, BoardProgress, BoardLive, BoardLiveState, BoardPointer, BoardProof, BoardRow, BoardView, BoardWorkItem, BoardZone } from "../proto";

/** A row's lane as its glyph. A row asking for input shows `?` instead. */
export const LANE_GLYPH: Record<BoardLane, string> = {
  next: "○",
  doing: "◐",
  review: "◑",
  done: "●",
};

/** The glyph a row shows: its lane's, or `?` when it asks for input. */
export function rowGlyph(r: Pick<BoardRow, "lane" | "status">): string {
  return r.status === "needs_input" ? "?" : LANE_GLYPH[r.lane];
}

/** The rows that wait on the reader, in the board's order: those whose ask names them, and those the board raised itself (a PR's red checks, an agent waiting). */
export function needsYou(v: Pick<BoardView, "zones">): BoardRow[] {
  return v.zones.flatMap((z) => z.rows).filter((r) => r.ask?.mine);
}

/** The board header's count: `Needs you (2)`. */
export function needsYouLine(n: number): string {
  return `Needs you (${n})`;
}

/** What a card says of who an ask waits on, when the reader can't answer it. */
export function askWaitingLine(a: Pick<BoardAsk, "raised" | "who_name" | "asked_by">): string {
  return a.raised ? "Raised by the board; it clears when its cause does." : `Waiting for ${a.who_name}, asked by ${a.asked_by}.`;
}

/** Whether a typed answer to a free-text ask can be sent. */
export function answerReady(text: string): boolean {
  return text.trim().length > 0;
}

/** Whether a row has an issue or a PR as its item: only those rows are
 * counted, and only they are proven or not (a note is neither). */
export function counted(r: Pick<BoardRow, "pointer">): boolean {
  return r.pointer.kind === "issue" || r.pointer.kind === "forge_item";
}

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

/** The mark on a Done row: how it is proven, or that it isn't yet. Null for a
 * row that isn't Done, and for a note, which has nothing to prove. */
export function proofMark(r: Pick<BoardRow, "lane" | "pointer" | "proof">): { proven: boolean; words: string } | null {
  if (r.lane !== "done" || !counted(r)) return null;
  const p: BoardProof | undefined = r.proof;
  if (!p) return { proven: false, words: "not proven yet" };
  return { proven: true, words: p.by === "ci" ? "proven by CI" : "proven" };
}

/** A board's sections, top to bottom: each group with the four lanes of its
 * rows, then the rows with no group (only if there are any). A board with
 * no groups is one section without a group, its lanes as they came. */
export type Section = { group: BoardGroup | null; lanes: BoardZone[] };

export function sections(v: Pick<BoardView, "zones" | "groups">): Section[] {
  const groups = v.groups ?? [];
  if (!groups.length) return [{ group: null, lanes: v.zones }];
  const of = (keep: (r: BoardRow) => boolean): BoardZone[] => v.zones.map((z) => ({ ...z, rows: z.rows.filter(keep) }));
  const out: Section[] = groups.map((g) => ({ group: g, lanes: of((r) => r.group === g.id) }));
  const loose = of((r) => !r.group || !groups.some((g) => g.id === r.group));
  if (loose.some((z) => z.rows.length)) out.push({ group: null, lanes: loose });
  return out;
}

/** A group's progress in words: `2 of 5 done`, or `no work yet`. */
export function groupLine(p: Pick<BoardProgress, "done" | "total">): string {
  return p.total ? `${p.done} of ${p.total} done` : "no work yet";
}

/** What a row's marks say, for its card: `merged, not closed`; `after #1859`
 * for a PR stacked on another (the row waits for it); and the attachments of
 * agents that were replaced (`earlier`), which the card folds. */
export function marksOf(r: Pick<BoardRow, "marks">): { merged: boolean; after: { words: string; repo?: string } | null; earlier: Set<string> } {
  const marks: BoardMark[] = r.marks ?? [];
  const after = marks.find((m) => m.kind === "after");
  return {
    merged: marks.some((m) => m.kind === "merged_not_closed"),
    after: after && after.kind === "after" ? { words: `after #${after.number}`, repo: after.repo } : null,
    earlier: new Set(marks.flatMap((m) => (m.kind === "earlier" ? [m.attachment] : []))),
  };
}

/** The fold's summary: `earlier agents (2)`. */
export function earlierLine(n: number): string {
  return `earlier agent${n === 1 ? "" : "s"} (${n})`;
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

/** A lane's rows, and how many of them are proven (only the Done lane has
 * any). */
export function laneCount(z: Pick<BoardZone, "rows">): { total: number; proven: number } {
  return { total: z.rows.length, proven: z.rows.filter((r) => r.proof).length };
}

/** An attachment's chip: what it points at, and its state when the reader
 * may see it (an item they can't read says nothing but what it is). */
export function chipOf(a: Pick<BoardAttachment, "pointer" | "live" | "block" | "role">): { label: string; state: string | null } {
  const label = pointerLabel(a.pointer, a.live);
  if (a.block) return { label, state: BLOCK_LABEL[a.block.state] };
  if (a.live) return { label, state: liveWord(a.live) };
  return { label, state: null };
}

/** The longest goal the platform takes (`GOAL_MAX`, control-wire). */
export const GOAL_MAX = 2000;

/** A text's first line that says something: a goal or a done note on a list. */
export function firstLine(text: string): string {
  return text.split("\n").map((l) => l.trim()).find(Boolean) ?? "";
}

/** A board's work that is done, in words, and how much of that is proven;
 * `pct` is for its bar. */
export function epicLine(p: Pick<BoardProgress, "done" | "total" | "proven">): { words: string; pct: number } {
  if (!p.total) return { words: "No work yet", pct: 0 };
  const proven = p.done ? `, ${p.proven} proven` : "";
  return { words: `${p.done} of ${p.total} done${proven}`, pct: Math.round((p.done / p.total) * 100) };
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
      if (live?.identifier) return `${live.identifier}-${live.number}`;
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

/** An id a person typed as a row's item (`fi_…`, `iss_…`) or an `https://`
 * link (a note's), or null when it isn't one; empty is no pointer. A run or
 * an agent block isn't a row's item: it is attached. */
export function parsePointer(text: string): BoardPointer | null {
  const t = text.trim();
  if (!t) return { kind: "none" };
  if (/^https:\/\/[^\s\u0000-\u001f\u007f]+$/.test(t)) return { kind: "link", url: t };
  const m = /^(fi|iss)_[0-9A-Za-z]{1,64}$/.exec(t);
  if (!m) return null;
  return m[1] === "fi" ? { kind: "forge_item", id: t } : { kind: "issue", id: t };
}

/** How many results the "points at" picker lists. */
export const PICK_LIMIT = 8;

/** How long the picker waits after a keystroke before it searches. */
export const PICK_DELAY_MS = 250;

/** A result's source label: `OWNER/NAME#N` for a mirrored item, `ARUGU-12`
 * for a tracker's, `#N` for a native issue, the run's own title for a run. */
export function workLabel(w: Pick<BoardWorkItem, "source" | "repo" | "identifier" | "number" | "title">): string {
  if (w.source === "run" || w.number == null) return w.title;
  if (w.identifier) return `${w.identifier}-${w.number}`;
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

/** What a card's state tag says: a tracker's own name for the state
 * (`In Progress`), else `LIVE_LABEL`'s. */
export function liveWord(l: Pick<BoardLive, "state" | "state_name">): string {
  return l.state_name || LIVE_LABEL[l.state];
}

/** Where a row's link goes: an item on its forge, a native issue or a run on
 * the platform page (`platform`, the paths of ids a host knows how to open). */
export function pointerHref(r: Pick<BoardRow, "pointer" | "live">, platform: (path: string) => string | undefined): string | undefined {
  const p = r.pointer;
  if (p.kind === "forge_item") {
    const l = r.live;
    if (l?.provider === "plane") return l.url && /^https:\/\//.test(l.url) ? l.url : undefined;
    return l?.repo ? `https://github.com/${l.repo}/${l.kind === "issue" ? "issues" : "pull"}/${l.number}` : undefined;
  }
  if (p.kind === "issue") return platform(`/issues/${p.id}`);
  if (p.kind === "run") return platform(`/runs/${p.id}`);
  if (p.kind === "link") return /^https:\/\//.test(p.url) ? p.url : undefined;
  return undefined;
}
