// An agent's display name (plan §3.11, a seam for Product mode): the only
// place one comes from. The panel's tabs and head, and chat's name on an
// agent's message (./chat/agent-name.tsx), ask here, so `names: "role"`
// (surfacesFor) can one day show Lead and Worker everywhere at once. Today
// everyone gets the engineer's set: an agent by its work's title or its
// session, a message's agent by `machine/tab`. Run with
// `node --experimental-strip-types`.

import { surfacesFor, type Surfaces } from "./surfaces.ts";

/** What a name is made from; every part is optional. */
export interface Agent {
  /** Its rung: the project's chief of staff, a board's lead, a worker on a card, or any agent. */
  role?: "chief" | "lead" | "worker" | "agent";
  /** What its work is called: a session's title (`#1431 staging checkout`). */
  title?: string | null;
  /** Its machine's name (`dust`). */
  machine?: string | null;
  /** Its session (its tab) on that machine, as people read it. */
  session?: string | null;
}

const ROLE: Record<NonNullable<Agent["role"]>, string> = { chief: "Chief of staff", lead: "Lead", worker: "Worker", agent: "Agent" };

/** The rung in words, lower case, for a head's meta line ("chief of staff", "lead"). */
export const rungOf = (a: Agent): string => ROLE[a.role ?? "agent"].toLowerCase();

/** How `a` is shown. `handle`: as chat tags it, `machine/tab` (the machine
 * alone without a tab). `names` is the reader's (surfacesFor); by role, a
 * Lead or a Worker. The chief of staff is always the Chief of staff. */
export function agentName(a: Agent, o: { handle?: boolean; names?: Surfaces["names"] } = {}): string {
  if (a.role === "chief") return ROLE.chief;
  if ((o.names ?? surfacesFor().names) === "role") return ROLE[a.role ?? "agent"];
  const session = a.session?.trim() ?? "";
  if (o.handle) {
    const machine = a.machine?.trim() || "a machine that left";
    return session ? `${machine}/${session}` : machine;
  }
  return a.title?.trim() || session || (a.machine?.trim() ? `an agent on ${a.machine.trim()}` : "An agent");
}
