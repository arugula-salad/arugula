// Following a person an invite named (#1473). An invite says whom, not which
// window: the guest picks the window when it opens the invite, from presence.

import type { ClientId, Presence, SessionId, TabId } from "../shared/proto";

/** How long an opened follow invite waits for the person to show. */
export const WATCH_MS = 60_000;

/** The window of `who` to follow: one on the invite's session if there is
 * one, else the first. Never this client's own. */
export function pickFollowed(
  presence: Presence[],
  who: string,
  session: SessionId | undefined,
  sessionOfTab: (tab: TabId) => SessionId | undefined,
  me: ClientId | null,
): ClientId | undefined {
  const theirs = presence.filter((p) => p.who === who && p.client !== me);
  const there = theirs.find((p) => session !== undefined && p.tab !== undefined && sessionOfTab(p.tab) === session);
  return (there ?? theirs[0])?.client;
}
