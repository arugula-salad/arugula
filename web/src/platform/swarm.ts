// The swarm's language on the platform's lists: which palette color a
// card's state bar takes, for the one fact each list's bar says
// (./swarm.css has the whole table). The swarm's own colors are fixed
// (shell/swarm/model.ts's REASON_COL); these are the palette's, so the
// chosen appearance holds.

/** A card's bar, by the palette color it takes. */
export type Tone = "accent" | "ok" | "warn" | "danger" | "info" | "teal" | "muted";

/** The class that gives a card its tone. */
export const toneClass = (t: Tone | null) => (t ? `sw-${t}` : "");

/** A project: the reader's role in it. Their own project is theirs alone,
 * so they own it. */
export function roleTone(role: string | null | undefined): Tone {
  if (!role || role === "owner") return "accent";
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

/** A run (S37's states): waiting for an approval or its workspace waits,
 * running goes, then how it ended. Denied, canceled and expired runs never
 * ran to an end of their own. */
export function runTone(state: string): Tone {
  if (state === "pending" || state === "approved") return "warn";
  if (state === "running") return "accent";
  if (state === "succeeded") return "ok";
  if (state === "failed") return "danger";
  return "muted";
}

/** A deploy (M217's states, a run's with `planned` before them and
 * `stale` beside them): planned and waiting for its approval waits, then
 * as a run. A stale one never applied, as a denied one didn't. */
export function deployTone(state: string): Tone {
  return state === "planned" ? "warn" : runTone(state);
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

/** A native issue (M194): open is live work, closed is done (whatever the
 * reason), neither good news nor a failure: never a failed run's color. */
export function issueTone(state: string): Tone {
  return state === "closed" ? "muted" : "ok";
}
