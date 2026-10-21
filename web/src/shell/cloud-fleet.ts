// The Cloud workspaces' tabs as fleet panes (S40, M116): what the swarm and
// the attention drawer draw for them, beside the machines' panes.
import { cloudTabAttention, cloudWorkspaces } from "../shared/cloud";
import type { FleetPane } from "./fleet";

/** Every cloud pane the swarm and the attention drawer draw: the Cloud
 * workspaces' tabs. */
export function cloudFleet(): FleetPane[] {
  return cloudWorkspaces.list.flatMap((w) => (cloudWorkspaces.tabs.get(w.id) ?? []).map((t): FleetPane => {
    const attention = cloudTabAttention(t);
    const id = Number.parseInt(t.id.slice(4, 17), 16);
    return {
      key: `cloud:${t.id}`, host: "Cloud", id, cloudTab: { workspace: w.id, tab: t.id },
      info: { id, epoch: 0, cwd: t.cwd, command: t.agent_name, running: attention === "working", policy: { kind: "none" }, current: null, last: null,
        attention, integration: false, type: "agent", host: null, kind: "agent", title: `${t.agent_name} in ${w.name}`,
        reason: attention === "needs_input" ? { kind: "input", since_ms: t.created, headline: `${t.agent_name} in ${w.name} waits for you`, actions: [] }
          : t.status === "failed" ? { kind: "failed", since_ms: t.created, headline: t.error ?? `${t.agent_name} in ${w.name} failed`, actions: [] } : null },
      session: { id: Number.parseInt(w.id.slice(3, 16), 16), name: w.name }, stale: !!cloudWorkspaces.error, person: { id: "me", name: "me", kind: "me" }, driver: null, watchers: [], unread: 0, mention: false,
    };
  }));
}
