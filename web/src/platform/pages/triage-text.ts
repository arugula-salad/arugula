// The "Not on a board" section's words and the "Add to board…" menu's model (#292), as
// plain functions: a project's open issues that sit on no board, and where
// each can go. Run with `node --experimental-strip-types`.

import type { BoardPointer, BoardWorkItem, ProjectBoard } from "../../shared/proto";

export const TRIAGE_NONE = "Nothing to triage: every open issue is on a board.";

/** `OWNER/NAME#N` of a mirrored issue, `ARUGU-12` of a tracker's, `#N` of a native one. */
export function sourceLabel(i: Pick<BoardWorkItem, "repo" | "identifier" | "number">): string {
  if (i.number == null) return "";
  if (i.identifier) return `${i.identifier}-${i.number}`;
  return i.repo ? `${i.repo}#${i.number}` : `#${i.number}`;
}

/** Where an item's label goes: a native issue to its page on the platform
 * (`issuePath`), a GitHub one to github.com. Undefined when it can't say. */
export function itemHref(i: Pick<BoardWorkItem, "pointer" | "source" | "repo" | "number" | "url">, issuePath: (id: string) => string): string | undefined {
  if (i.source === "native" && i.pointer.kind === "issue") return issuePath(i.pointer.id);
  if (i.source === "github" && i.repo && i.number != null && /^[\w.-]+\/[\w.-]+$/.test(i.repo)) return `https://github.com/${i.repo}/issues/${i.number}`;
  if (i.source === "plane" && i.url && /^https:\/\//.test(i.url)) return i.url;
  return undefined;
}

/** A place an item can go: a board. */
export type Target = { board: string };

/** The menu's model: each board of the project that is still open (a closed
 * board's work is done). Boards have no zones to choose from: the item is a
 * row, and its lane follows it. */
export function targets(list: readonly Pick<ProjectBoard, "id" | "title" | "closed_at">[]): { board: string; title: string }[] {
  return list.filter((b) => b.closed_at == null).map((b) => ({ board: b.id, title: b.title }));
}

/** The items still on the list once `pointer` has been put on a board. */
export function without<T extends { pointer: BoardPointer }>(items: readonly T[], pointer: BoardPointer): T[] {
  return items.filter((i) => JSON.stringify(i.pointer) !== JSON.stringify(pointer));
}

/** The keys from `a` to `b` in `keys`, either way round; just `b` when `a` isn't in them (no anchor yet). */
export function rangeKeys(keys: readonly string[], a: string | null, b: string): string[] {
  const x = a === null ? -1 : keys.indexOf(a);
  const y = keys.indexOf(b);
  if (y < 0) return [];
  if (x < 0) return [b];
  return keys.slice(Math.min(x, y), Math.max(x, y) + 1);
}

/** The select-all box over `of` rows with `picked` of them selected: unchecked, mixed or checked. */
export function boxState(picked: number, of: number): "none" | "some" | "all" {
  return picked === 0 || of === 0 ? "none" : picked >= of ? "all" : "some";
}

/** The toast after `n` issues went onto `board`: "#12 is on Launch, in Next". `one` is the lone issue's label. */
export function addedWords(n: number, one: string, board: string): string {
  return n === 1 ? `${one} is on ${board}, in Next` : `${n} issues are on ${board}, in Next`;
}
