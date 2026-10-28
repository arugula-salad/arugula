// The agents' notifications hint's fact: a guest or a teammate (not the
// machine's owner, who's always told) has the shown session open, may
// answer in it, has agents in it, and isn't told about them yet (M29,
// `Client.notifyPref`).

import { paneIds, type Client } from "../client";
import type { Fact } from "./index";

export function sharedAgents(client: Client): Fact | null {
  const st = client.state;
  const session = client.session;
  if (!st?.roles || session === null || client.role(session) === "viewer") return null;
  const pref = client.notifyPref;
  if (!pref || pref.all || pref.sessions.includes(session)) return null;
  const tabs = st.sessions.find((s) => s.id === session)?.tabs ?? [];
  const agent = st.tabs
    .filter((t) => tabs.includes(t.id))
    .flatMap(paneIds)
    .some((id) => {
      const p = client.info(id);
      return p?.type === "agent" || p?.kind === "agent";
    });
  return agent ? { args: { session } } : null;
}
