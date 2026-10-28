// The Triage page's words and the "Add to board…" menu's model (#292), as
// plain functions: a project's open issues that sit on no board, and where
// each can go. Run with `node --experimental-strip-types`.

import type { BoardPointer, BoardView, BoardWorkItem } from "../../shared/proto";

export const TRIAGE_LEDE = "Open issues that aren't on a board yet.";
export const TRIAGE_NONE = "Nothing to triage: every open issue is on a board.";
export const TRIAGE_OFF = "Boards are a Labs feature, and they're off for you. Triage comes with them: until then, every issue is under All issues.";

/** The place the bar's Issues place becomes: Triage once boards are on. A
 * reader whose boards haven't answered yet (`null`) or are off keeps Issues. */
export function issuesPlace(boards: readonly unknown[] | null | "off"): "issues" | "triage" {
  return boards === null || boards === "off" ? "issues" : "triage";
}

/** `OWNER/NAME#N` of a mirrored issue, `#N` of a native one. */
export function sourceLabel(i: Pick<BoardWorkItem, "repo" | "number">): string {
  if (i.number == null) return "";
  return i.repo ? `${i.repo}#${i.number}` : `#${i.number}`;
}

/** Where an item's label goes: a native issue to its page on the platform
 * (`issuePath`), a GitHub one to github.com. Undefined when it can't say. */
export function itemHref(i: Pick<BoardWorkItem, "pointer" | "source" | "repo" | "number">, issuePath: (id: string) => string): string | undefined {
  if (i.source === "native" && i.pointer.kind === "issue") return issuePath(i.pointer.id);
  if (i.source === "github" && i.repo && i.number != null && /^[\w.-]+\/[\w.-]+$/.test(i.repo)) return `https://github.com/${i.repo}/issues/${i.number}`;
  return undefined;
}

/** A place an item can go: one zone of one board. */
export type Target = { board: string; zone: string };

/** The menu's model: each board with its zones but Proofs (a board's last
 * zone, where what's proven goes, not where work is put). A board left with
 * no zone is out. */
export function targets(views: readonly Pick<BoardView, "board" | "zones">[]): { board: string; title: string; zones: { id: string; title: string }[] }[] {
  return views
    .map((v) => ({
      board: v.board.id,
      title: v.board.title,
      zones: v.zones.filter((z) => z.kind !== "proofs").map((z) => ({ id: z.id, title: z.title })),
    }))
    .filter((b) => b.zones.length > 0);
}

/** The items still on the list once `pointer` has been put on a board. */
export function without<T extends { pointer: BoardPointer }>(items: readonly T[], pointer: BoardPointer): T[] {
  return items.filter((i) => JSON.stringify(i.pointer) !== JSON.stringify(pointer));
}
