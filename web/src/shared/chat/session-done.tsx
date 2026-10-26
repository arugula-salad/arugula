// What a session's card says once its PR merges (M265), and its read-only
// diff, kept apart from ./session-card so the card's own changes merge
// easily.
//
// - **Merged.** A PR from the session's branch merged: its thread is done.
//   An ended session's card says "merged ✓"; one whose turn is still under
//   way says it stops when that turn ends.
// - **Diff ↗** in place of an editor. A hosted box's editor (code-server)
//   runs as the box's own user, which can't enter a session user's home
//   (`/home/sesN`, 0700, where its checkout is), and the slots stay apart
//   on purpose. So the card offers what's readable without the box: the
//   pushed branch's diff on GitHub, read only, until its PR is open (then
//   PR ↗ is the diff).

import type { SessionCard } from "../proto";
import type { Tone } from "./session-card";

const ended = (c: SessionCard) => c.state === "done" || c.state === "failed" || c.state === "stopped";

/** The card's label once its PR merged and the session ended. */
export function mergedOf(c: SessionCard): { label: string; tone: Tone } | null {
  return c.merged_at !== null && ended(c) ? { label: "merged ✓", tone: "ok" } : null;
}

/** A line on a card whose PR merged while a turn is under way. */
export function MergedLine({ c }: { c: SessionCard }) {
  if (c.merged_at === null || ended(c)) return null;
  return (
    <p class="session-card-line dim" data-session-merging>
      Its pull request merged: the session ends when this turn does.
    </p>
  );
}

/** The pushed branch's diff on GitHub, while it has no PR. */
export function DiffLink({ c }: { c: SessionCard }) {
  if (!c.pushed || c.pr || !c.repo || !/^[\w.-]+\/[\w.-]+$/.test(c.repo)) return null;
  const branch = c.branch.split("/").map(encodeURIComponent).join("/");
  return (
    <a
      class="button"
      href={`https://github.com/${c.repo}/compare/${branch}`}
      target="_blank"
      rel="noopener"
      title="What its pushed branch changes, on GitHub, read only. (Its checkout on the box is its own user's: an editor there can't open it.)"
      data-session-diff
    >
      Diff ↗
    </a>
  );
}
