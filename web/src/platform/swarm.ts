// The swarm's language on the platform's lists: which palette color a
// card's state bar takes, for the one fact each list's bar says
// (./swarm.css has the whole table). The swarm's own colors are fixed
// (shell/swarm/model.ts's REASON_COL); these are the palette's, so the
// chosen appearance holds.

import type { Tone } from "../shared/ui/state-chip";

/** A card's bar, by the palette color it takes: a state chip's tones
 * (UI audit #183). */
export type { Tone };

/** A run's, a deploy's and an issue's, as their chips take them. */
export { deployTone, issueTone, runTone } from "../shared/tones";

/** The class that gives a card its tone. */
export const toneClass = (t: Tone | null) => (t ? `sw-${t}` : "");

/** A project: the reader's role in it. Their own project, theirs alone,
 * is muted, so its bar isn't a team owner's (UI audit #162); it has no
 * team pill, so it isn't taken for one they only view. (Teal was the
 * other choice, but it's the accent's near twin in the default palette.) */
export function roleTone(role: string | null | undefined): Tone {
  if (!role) return "muted";
  if (role === "owner") return "accent";
  return role === "editor" ? "info" : "muted";
}

/** A hosted workspace (M179): being made or woken waits, ready is up,
 * failed failed, and paused or being deleted is neither. */
export function workspaceTone(state: string, asked: string | null): Tone {
  if (asked === "wake" || state === "creating" || state === "enrolling" || state === "waking") return "warn";
  if (state === "ready") return "ok";
  if (state === "failed") return "danger";
  return "muted";
}

/** An inbox entry (M171): why it wants the reader, as the swarm colors a
 * card by why its pane wants them. A review asked of them is the swarm's
 * ask, changes asked of their PR wait on them, a failed check or run is a
 * failure, a mention or an assignment is news, and done is done. */
const REASON: Record<string, Tone> = {
  review_requested: "accent",
  changes_requested: "warn",
  checks_failed: "danger",
  run_failed: "danger",
  mentioned: "info",
  thread_mention: "info",
  assigned: "teal",
  done: "ok",
};

export const reasonTone = (reason: string): Tone => REASON[reason] ?? "muted";

/** A chat room: a mention, unread messages, or nothing to say (the same
 * colors as its count's, `.chat-count`). */
export function roomTone(unread: number, mentions: number): Tone | null {
  if (unread > 0 && mentions > 0) return "warn";
  return unread > 0 ? "accent" : null;
}
