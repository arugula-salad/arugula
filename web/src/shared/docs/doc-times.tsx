// When a Doc was made and last changed (Arugula #825): "Created 4 Nov ·
// edited 3h ago" under an open Doc's title, the full time on hover. The share
// viewer draws it too, so this imports no more than the time does.

import { day, when } from "../time";
import { Time } from "../ui/time";

/** A Doc as the times see it: `made` is absent or null where it isn't known. */
export interface DocTimesOf {
  made?: number | null;
  updated?: number | null;
  /** Who made it: `migrated` is a Doc from before Docs were kept. */
  by?: string;
}

/** Two times this close are one: a Doc is made and first written in a minute. */
const SAME_MS = 60_000;

/** When it was made, or null if it can't be told. A Doc from before Docs
 * were kept (`migrated`) has the oldest file's time, or the migration's:
 * not when it was made, so it isn't said. */
export function madeAt(d: DocTimesOf): number | null {
  return d.by === "migrated" || !d.made ? null : d.made;
}

/** "Created 4 Nov · edited 3h ago"; just "Created 4 Nov" while the two are
 * a minute apart or less, and just "Edited 3h ago" where created is unknown. */
export function DocTimes({ doc, now }: { doc: DocTimesOf; now?: number }) {
  const made = madeAt(doc);
  const updated = doc.updated || null;
  if (!made && !updated) return null;
  const same = made !== null && (updated === null || updated - made <= SAME_MS);
  return (
    <span class="doc-times" data-doc-times>
      {made !== null && (
        <span data-doc-created>
          Created{" "}
          <time dateTime={new Date(made).toISOString()} title={when(made)}>
            {day(made, now)}
          </time>
        </span>
      )}
      {!same && updated !== null && (
        <span data-doc-edited>
          {made !== null ? " · edited " : "Edited "}
          <Time at={updated} now={now} />
        </span>
      )}
    </span>
  );
}
