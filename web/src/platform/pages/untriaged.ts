// A project's open issues on no board: every read of them and every add to a
// board (#656). The one place that knows how the platform's work route pages
// and how rows are put on a board, so the section and All boards ask here.

import type { BoardPointer, BoardWorkItem, BoardWorkItems } from "../../shared/proto";
import { boards } from "../../shared/boards/store";
import { platformFetch, refusalOf } from "../../shared/maintenance";

/** An untriaged item; `labels` come with the server's paging. */
export type Untriaged = BoardWorkItem & { labels?: string[] };

/** One label of the facet and how many of the matches carry it. */
export type LabelCount = { label: string; count: number };

/** One page of the list: `next` is the cursor of the following page (null at the end), `total` counts every match, `labels` the facet. */
export type UntriagedPage = { items: Untriaged[]; next: string | null; total: number; labels: LabelCount[] };

export type UntriagedQuery = { q?: string; label?: string; order?: "newest" | "oldest"; cursor?: string | null };

/** The key a row, a selection and a drop know an item by. */
export function keyOf(i: { pointer: BoardPointer }): string {
  return JSON.stringify(i.pointer);
}

/** Whether `i` is what the filter words ask for: a `#number` by its number's start, words in the title. */
function matches(i: Untriaged, q: string): boolean {
  const s = q.trim().toLowerCase().replace(/^#/, "");
  if (!s) return true;
  return i.title.toLowerCase().includes(s) || String(i.number ?? "").startsWith(s);
}

/** The project's untriaged: the newest 100, narrowed and ordered here until the route pages. */
export async function readUntriaged(project: string, query: UntriagedQuery = {}): Promise<UntriagedPage> {
  const res = await platformFetch(`/api/projects/${encodeURIComponent(project)}/work?untriaged=true&limit=100`, { credentials: "same-origin" });
  const j = (await res.json().catch(() => ({}))) as BoardWorkItems & { error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  const words = (j.items as Untriaged[]).filter((i) => matches(i, query.q ?? ""));
  const counts = new Map<string, number>();
  for (const i of words) for (const l of i.labels ?? []) counts.set(l, (counts.get(l) ?? 0) + 1);
  const labels = [...counts].map(([label, count]) => ({ label, count })).sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
  let items = query.label ? words.filter((i) => i.labels?.includes(query.label!)) : words;
  if (query.order === "oldest") items = [...items].reverse();
  return { items, next: null, total: items.length, labels };
}

/** How many open issues sit on no board, for each of `projects`; one that can't be read counts none. */
export async function countUntriaged(projects: readonly string[]): Promise<Map<string, number>> {
  const all = await Promise.all(
    projects.map((id) =>
      readUntriaged(id).then(
        (p): [string, number] => [id, p.total],
        (): [string, number] => [id, 0],
      ),
    ),
  );
  return new Map(all);
}

/** What an add did: the pointers that went on the board, and those that didn't, with why. */
export type Added = { added: BoardPointer[]; failed: { pointer: BoardPointer; error: string }[] };

/** Puts `pointers` on `board`, each a row (its lane follows it). */
export async function addUntriaged(board: string, pointers: readonly BoardPointer[]): Promise<Added> {
  const out: Added = { added: [], failed: [] };
  for (const pointer of pointers) {
    try {
      await boards.put(board, { entry: "row", pointer });
      out.added.push(pointer);
    } catch (e) {
      out.failed.push({ pointer, error: (e as Error).message });
    }
  }
  return out;
}
