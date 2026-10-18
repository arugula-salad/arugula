// Whether what the workspace shows is live (M113, #1028). While the page's
// link to the machine is down, its state is the last it heard: the chrome
// says so, greys what it can't vouch for (working, running, needs you), and
// turns off what would change something, with the reason.

import type { Client } from "../../client";
import { directory } from "../../hosts";

/** Why nothing can be changed right now, or null when the link is up. */
export function offlineWhy(client: Client): string | null {
  if (client.connected) return null;
  return `Reconnecting to ${directory.current || "the machine"}: nothing changes until it's back`;
}

/** Why this person can't open tabs in `session`, if they can't. */
export function newTabRefusal(client: Client, session: number): string | null {
  return offlineWhy(client) ?? (client.role(session) === "viewer" ? "Viewers can't open tabs in this session" : null);
}
