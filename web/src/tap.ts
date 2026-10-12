// Where a tap on a notification goes (sw.ts): the pane (its card shows over
// it), through control (M21) the pane on the daemon it came from; something
// that wants you (M24's reason) at its card on the swarm's rail (M26); a
// mention (M61) with its thread open.

/** A thread worth opening: a pane's or a session's. */
export function tapThread(thread?: string): string | undefined {
  return thread && /^(pane|session)-\d+$/.test(thread) ? thread : undefined;
}

/** A push's Share (#237): the pane, with its session's share picker open. */
export function shareUrl(n: { pane: number; daemon?: string }): string {
  return (n.daemon ? `/#pane=${n.daemon}.${n.pane}` : `/#pane=${n.pane}`) + "&share";
}

/** #237: a push for a pane that wants you, with room left for another
 * button (browsers show two), gets Share first: sharing it with someone is
 * the natural next tap. Approvals, answers and gates keep their two. */
export function withShare<A extends { action: string; title: string }>(actions: A[], n: { pane?: number; reason?: unknown }): (A | { action: "share"; title: "Share" })[] {
  return n.pane && n.reason && actions.length < 2 ? [{ action: "share", title: "Share" }, ...actions] : actions;
}

export function tapUrl(n: { pane?: number; daemon?: string; thread?: string; reason?: unknown }): string {
  if (!n.pane) return "/";
  const where = n.reason ? "swarm" : "pane";
  const thread = tapThread(n.thread);
  return (n.daemon ? `/#${where}=${n.daemon}.${n.pane}` : `/#${where}=${n.pane}`) + (thread ? `&thread=${thread}` : "");
}
