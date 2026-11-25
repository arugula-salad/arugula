// An agent's tab head (Arugula #1342): what it is to the reader (its role,
// "Chief of staff"), the machine it runs on (a Cloud workspace by its own
// name) and whether anything hears a message now: awake while a turn
// runs, idle on its machine (a message wakes it), asleep while its machine
// is offline (a message waits), or none. The chief of staff's tab draws it
// over its room; a board lead's and an issue agent's tab can draw it too,
// from `leadAwake` (../shell/swarm/overview.ts) over their session.

import type { LeadAwake } from "../shell/swarm/overview";
import { shortHost } from "../shell/swarm/model";

/** The head: `role`, then where it runs and how it stands. */
export function AgentHead({ role, at }: { role: string; at: LeadAwake }) {
  const m = at.state === "none" ? "" : shortHost(at.machine);
  return (
    <span class="agent-head" data-agent-head={role} data-agent-state={at.state}>
      <b>{role}</b>
      {at.state === "none" && " · none"}
      {at.state === "awake" && ` · ${m} · awake`}
      {at.state === "idle" && ` · idle on ${m} (wakes on its own)`}
      {at.state === "asleep" && ` · asleep: ${m} is offline (messages wait)`}
      {at.state === "unknown" && ` · ${m}`}
    </span>
  );
}
