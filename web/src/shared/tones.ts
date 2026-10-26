// The tone of a run's, a deploy's and a native issue's state, one wherever
// it shows (UI audit #183): their cards' bars on the platform page's lists,
// their state chips (./ui/state-chip.tsx) on its pages and inbox, and on
// control's deploy prompt, which can't reach platform/. platform/swarm.ts
// hands them on to the lists.

import type { Tone } from "./ui/state-chip";

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

/** A native issue (M194): open is live work, closed is done (whatever the
 * reason), neither good news nor a failure: never a failed run's color. */
export function issueTone(state: string): Tone {
  return state === "closed" ? "muted" : "ok";
}
