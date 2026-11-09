// A project's open issues on no board: every read of them and every add to a
// board (#656, #661). The one place that knows how the platform's work route
// pages and filters, and how rows are put on a board many at a time, so the
// section and All boards ask here.

import type { BoardAddedMany, BoardAddMany, BoardPointer, BoardWorkItem, BoardWorkItems } from "../../shared/proto";
import { platformFetch, refusalOf } from "../../shared/maintenance";

/** An untriaged item. */
export type Untriaged = BoardWorkItem;

/** One label of the facet and how many of the matches carry it. */
export type LabelCount = { label: string; count: number };

/** One page of the list: `next` is the cursor of the following page (null at the end), `total` counts every match, `labels` the facet. */
export type UntriagedPage = { items: Untriaged[]; next: string | null; total: number; labels: LabelCount[] };

export type UntriagedQuery = { q?: string; label?: string; order?: "newest" | "oldest"; cursor?: string | null; limit?: number };

/** The key a row, a selection and a drop know an item by; the pointer, as JSON. */
export function keyOf(i: { pointer: BoardPointer }): string {
  return JSON.stringify(i.pointer);
}

/** The pointer a key stands for. */
export function pointerOf(key: string): BoardPointer {
  return JSON.parse(key) as BoardPointer;
}

/** The project's untriaged, a page at a time: `q`, `label` and `order` are the server's. */
export async function readUntriaged(project: string, query: UntriagedQuery = {}): Promise<UntriagedPage> {
  const args = new URLSearchParams({ untriaged: "true", limit: String(query.limit ?? 100) });
  if (query.q?.trim()) args.set("q", query.q.trim());
  if (query.label) args.set("label", query.label);
  if (query.order === "oldest") args.set("order", "old");
  if (query.cursor) args.set("cursor", query.cursor);
  const res = await platformFetch(`/api/projects/${encodeURIComponent(project)}/work?${args}`, { credentials: "same-origin" });
  const j = (await res.json().catch(() => ({}))) as BoardWorkItems & { error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return { items: j.items, next: j.next ?? null, total: j.total ?? j.items.length, labels: (j.labels ?? []).map((l) => ({ label: l.name, count: l.count })) };
}

/** How many open issues sit on no board, for each of `projects`; one that can't be read counts none. */
export async function countUntriaged(projects: readonly string[]): Promise<Map<string, number>> {
  const all = await Promise.all(
    projects.map((id) =>
      readUntriaged(id, { limit: 1 }).then(
        (p): [string, number] => [id, p.total],
        (): [string, number] => [id, 0],
      ),
    ),
  );
  return new Map(all);
}

/** What to put on a board: these pointers, or everything the query matches but `except`. */
export type Chosen = { pointers: BoardPointer[] } | { query: { project: string; q?: string; label?: string }; except: BoardPointer[] };

/** Puts `chosen` on `board`, each a row (its lane follows it), in one call. Answers how many rows were made. */
export async function addUntriaged(board: string, chosen: Chosen): Promise<number> {
  const body: BoardAddMany = "pointers" in chosen ? { pointers: chosen.pointers } : { untriaged: chosen.query, except: chosen.except };
  const res = await platformFetch(`/api/boards/${encodeURIComponent(board)}/entries/many`, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const j = (await res.json().catch(() => ({}))) as Partial<BoardAddedMany> & { error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return j.added ?? 0;
}
