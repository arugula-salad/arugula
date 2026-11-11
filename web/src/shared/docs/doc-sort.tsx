// How a Docs list sorts (Arugula #825): by when a Doc was last changed (the
// default) or when it was made, newest first, and the time each row shows.

import { Time } from "../ui/time";
import { useListSort, type ListSortOption } from "../ui/list-page";
import { madeAt, type DocTimesOf } from "./doc-times";

export type DocSort = "updated" | "created";

export const DOC_SORTS: ListSortOption<DocSort>[] = [
  { key: "updated", label: "Updated" },
  { key: "created", label: "Created" },
];

/** The sort this browser left on: Updated until someone chooses Created. */
export const useDocSort = () => useListSort<DocSort>("docs-sort", DOC_SORTS);

/** The time a row sorts by. A Doc whose creation isn't known sorts by its
 * edit when asked for created, so it keeps its place among the rest. */
export function sortTime(d: DocTimesOf, by: DocSort): number {
  return (by === "created" ? madeAt(d) : null) ?? d.updated ?? d.made ?? 0;
}

/** A row's time, following the sort: "edited 3h ago" or "created 2d ago". */
export function DocRowTime({ doc, by }: { doc: DocTimesOf; by: DocSort }) {
  const made = by === "created" ? madeAt(doc) : null;
  const at = made ?? doc.updated ?? doc.made;
  return (
    <>
      {made !== null ? "created " : "edited "}
      <Time at={at} title={made !== null ? "Created" : "Updated"} />
    </>
  );
}
