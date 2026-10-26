// Where a tap on a notification goes (sw.ts): the pane (its card shows over
// it), through control (M21) the pane on the daemon it came from; something
// that wants you (M24's reason) at its card on the swarm's rail (M26); a
// chat push from control (M123) at its message. Once control has a
// platform page (M156), a chat push opens there, by id: `platformTap`; an inbox push (M171) at the inbox, or (M202) at the
// native issue or the thread it's about; a deploy waiting for approval
// (M218) at its card.

/** A chat push from control (M123): `chat: {room, seq}`, opened on the chat
 * page at that message. Anything that isn't a room's id opens nothing. */
export function chatTap(n: { chat?: unknown }): string | undefined {
  const c = n.chat as { room?: unknown; seq?: unknown; parent?: unknown } | undefined;
  if (!c || typeof c.room !== "string" || !/^[a-z]{2}_[0-9a-f]{8,32}$/.test(c.room)) return undefined;
  const msg = typeof c.seq === "number" && Number.isInteger(c.seq) ? `&msg=${c.seq}` : "";
  // A reply in a thread (#106) opens the thread.
  const thread = typeof c.parent === "number" && Number.isInteger(c.parent) ? `&thread=${c.parent}` : "";
  return `/#chat=${c.room}${msg}${thread}`;
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

export function tapUrl(n: { pane?: number; daemon?: string; reason?: unknown; follow?: unknown }): string {
  if (!n.pane) return "/";
  const where = n.reason ? "swarm" : "pane";
  const at = n.daemon ? `/#${where}=${n.daemon}.${n.pane}` : `/#${where}=${n.pane}`;
  // A follow invite (#1473): whom to follow once the pane is up.
  return where === "pane" && typeof n.follow === "string" && n.follow ? `${at}&follow=${encodeURIComponent(n.follow)}` : at;
}

/** M156: where a chat push opens on the platform page (an id-only path,
 * which the page puts in its project), or undefined for a push that isn't
 * one of the platform's. */
export function platformTap(n: { chat?: unknown; inbox?: unknown; deploy?: unknown; hosted?: unknown }): string | undefined {
  // M218: a deploy waiting for an owner's approval.
  if (typeof n.deploy === "string" && /^dep_[0-9a-f]{32}$/.test(n.deploy)) return `/deploys/${n.deploy}`;
  // A hosted workspace whose agent turn ran past the provider's limit.
  const h = n.hosted as { project?: unknown; workspace?: unknown } | undefined;
  if (h && typeof h.project === "string" && /^prj_[0-9a-f]{32}$/.test(h.project) && typeof h.workspace === "string" && /^hw_[0-9a-f]{32}$/.test(h.workspace)) {
    return `/p/${h.project}/workspaces/${h.workspace}`;
  }
  const c = n.chat as { room?: unknown; seq?: unknown; parent?: unknown } | undefined;
  if (c && typeof c.room === "string" && /^[a-z]{2}_[0-9a-f]{8,32}$/.test(c.room)) {
    const q = [
      typeof c.seq === "number" && Number.isInteger(c.seq) ? `msg=${c.seq}` : "",
      // A reply in a thread (#106) opens the thread.
      typeof c.parent === "number" && Number.isInteger(c.parent) ? `thread=${c.parent}` : "",
    ].filter(Boolean);
    return `/chat/${c.room}${q.length ? `?${q.join("&")}` : ""}`;
  }
  // M171: something on a linked repo wants you: the inbox. M202: a native
  // issue opens itself (at the message, for a mention in its thread); a
  // mention in a mirrored item's thread opens the thread.
  if (n.inbox && typeof n.inbox === "object") {
    const i = n.inbox as { item?: unknown; room?: unknown; seq?: unknown };
    const msg = typeof i.seq === "number" && Number.isInteger(i.seq) ? `?msg=${i.seq}` : "";
    if (typeof i.item === "string" && /^iss_[0-9a-f]{8,64}$/.test(i.item)) return `/issues/${i.item}${msg}`;
    // M215: a run that failed opens the run.
    if (typeof i.item === "string" && /^run_[0-9a-f]{32}$/.test(i.item)) return `/runs/${i.item}`;
    if (typeof i.room === "string" && /^[a-z]{2}_[0-9a-f]{8,32}$/.test(i.room)) return `/chat/${i.room}${msg}`;
    return "/inbox";
  }
  return undefined;
}

/** M156: the platform page's origin as `/control.json` says it, or null. */
export function platformOf(info: unknown): string | null {
  const p = (info as { platform?: unknown } | null)?.platform;
  return typeof p === "string" && /^https?:\/\/[^/]+$/.test(p) ? p : null;
}
