// Where a tap on a notification goes (sw.ts): the pane (its card shows over
// it), through control (M21) the pane on the daemon it came from; something
// that wants you (M24's reason) at its card on the swarm's rail (M26); a
// chat push from control (M123) at its message. Once control has a
// platform page (M156), a chat push or a Cloud workspace's opens there, by
// id: `platformTap`.

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

export function tapUrl(n: { pane?: number; daemon?: string; reason?: unknown }): string {
  if (!n.pane) return "/";
  const where = n.reason ? "swarm" : "pane";
  return n.daemon ? `/#${where}=${n.daemon}.${n.pane}` : `/#${where}=${n.pane}`;
}

/** M156: where a chat push or a Cloud one opens on the platform page (an
 * id-only path, which the page puts in its project), or undefined for a
 * push that isn't one of those. */
export function platformTap(n: { chat?: unknown; cloud_run?: unknown; cloud_workspace?: unknown; cloud_tab?: unknown }): string | undefined {
  const c = n.chat as { room?: unknown; seq?: unknown } | undefined;
  if (c && typeof c.room === "string" && /^[a-z]{2}_[0-9a-f]{8,32}$/.test(c.room)) {
    const msg = typeof c.seq === "number" && Number.isInteger(c.seq) ? `?msg=${c.seq}` : "";
    return `/chat/${c.room}${msg}`;
  }
  if (typeof n.cloud_run === "string" && /^run_[0-9a-f]+$/.test(n.cloud_run)) return `/past/${n.cloud_run}`;
  // A Cloud tab's (one that waits for you, or finished).
  if (typeof n.cloud_workspace === "string" && /^cw_[0-9a-f]+$/.test(n.cloud_workspace) && typeof n.cloud_tab === "string" && /^cwc_[0-9a-f]+$/.test(n.cloud_tab)) {
    return `/workspaces/${n.cloud_workspace}/${n.cloud_tab}`;
  }
  return undefined;
}

/** M156: the platform page's origin as `/control.json` says it, or null. */
export function platformOf(info: unknown): string | null {
  const p = (info as { platform?: unknown } | null)?.platform;
  return typeof p === "string" && /^https?:\/\/[^/]+$/.test(p) ? p : null;
}
