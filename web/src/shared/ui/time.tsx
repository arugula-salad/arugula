// A time on the page (UI audit #182): how long ago (or "in 7h"), the full
// time in its title, the instant in `dateTime`. The words are ../time.ts.

import { ago, iso, when } from "../time";

/**
 * `<Time at={ms} />`: "3m ago", "2d ago", then "Oct 23"; hover for
 * "Oct 23, 2026, 1:24 PM".
 * - `on`: "on Oct 23" past a week, for a sentence.
 * - `long`: that, and the units spelled out ("3 days ago"), as an issue's.
 * - `title`: what the hover says before the time ("Updated").
 * - Anything else (`class`, `data-*`) lands on the `<time>`.
 * Nothing when there's no time (null, 0): a "never".
 */
export function Time({ at, on, long, title, now, ...attrs }: { at: number | null | undefined; on?: boolean; long?: boolean; title?: string; now?: number; class?: string; [data: `data-${string}`]: unknown }) {
  if (!at) return null;
  return (
    <time dateTime={iso(at)} title={title ? `${title} ${when(at)}` : when(at)} {...attrs}>
      {ago(at, now, long ? "long" : on ? "on" : "short")}
    </time>
  );
}
