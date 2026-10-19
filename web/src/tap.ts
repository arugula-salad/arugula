// Where a tap on a notification goes (sw.ts): the pane (its card shows over
// it), through control (M21) the pane on the daemon it came from; something
// that wants you (M24's reason) at its card on the swarm's rail (M26); a
// mention (M61) with its thread open.

/** A thread worth opening: a pane's, a session's or a conversation's. */
export function tapThread(thread?: string): string | undefined {
  return thread && /^(?:(pane|session)-\d+|conv-c-[0-9a-f]{32})$/.test(thread) ? thread : undefined;
}

/** A conversation's notification (#855): it has no pane, and opens on the
 * chat page, on its home machine (`host`: the daemon's id, through
 * control). */
export function convTap(n: { daemon?: string; thread?: string }): string | undefined {
  const t = tapThread(n.thread);
  if (!t?.startsWith("conv-")) return undefined;
  return `/#chat=${t}${n.daemon ? `&host=${n.daemon}` : ""}`;
}

/** A chat push from control (M123): `chat: {room, seq}`, opened on the chat
 * page at that message. Anything that isn't a room's id opens nothing. */
export function chatTap(n: { chat?: unknown }): string | undefined {
  const c = n.chat as { room?: unknown; seq?: unknown } | undefined;
  if (!c || typeof c.room !== "string" || !/^[a-z]{2}_[0-9a-f]{8,32}$/.test(c.room)) return undefined;
  const msg = typeof c.seq === "number" && Number.isInteger(c.seq) ? `&msg=${c.seq}` : "";
  return `/#chat=${c.room}${msg}`;
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
  const conv = convTap(n);
  if (conv) return conv;
  if (!n.pane) return "/";
  const where = n.reason ? "swarm" : "pane";
  const thread = tapThread(n.thread);
  return (n.daemon ? `/#${where}=${n.daemon}.${n.pane}` : `/#${where}=${n.pane}`) + (thread ? `&thread=${thread}` : "");
}
