// An agent's display name (plan §3.11, a seam for Product mode): the only
// place one comes from. The panel's tabs and head, and chat's name on an
// agent's message (./chat/agent-name.tsx), ask here, so `names: "role"`
// (surfacesFor) can one day show Lead and Worker everywhere at once. Today
// everyone gets the engineer's set: an agent by its work's title or its
// session. In chat (Arugula #1582) an agent reads by what it is, a board's
// lead by its board, never `machine/tab`: that's only the handle `@` routes
// by. Run with `node --experimental-strip-types`.

import { shownSession } from "./chief-of-staff.ts";
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
 * Lead or a Worker. The chief of staff, by its rung or its session's name
 * (./chief-of-staff.ts), is always the Chief of staff. */
export function agentName(a: Agent, o: { handle?: boolean; names?: Surfaces["names"] } = {}): string {
  if (a.role === "chief" || [a.session, a.title].some((n) => n && shownSession(n.trim()) === ROLE.chief)) return ROLE.chief;
  if ((o.names ?? surfacesFor().names) === "role") return ROLE[a.role ?? "agent"];
  const session = a.session?.trim() ?? "";
  if (o.handle) {
    const machine = a.machine?.trim() || "a machine that left";
    return session ? `${machine}/${session}` : machine;
  }
  return a.title?.trim() || session || (a.machine?.trim() ? `an agent on ${a.machine.trim()}` : "An agent");
}

/** A board whose lead an agent in chat may be (a project's board as read). */
export interface LedBoard {
  title: string;
  closed_at?: number;
  lead?: { machine: string; session: string } | null;
}

/** An agent in chat as people read it (Arugula #1582): a board's lead by its
 * board ("Design lift lead"), the chief of staff as "Chief of staff", any
 * other by its session's name. Never `machine/…`: the machine goes in the
 * name's hover card. Chat's author lines, `@`'s rows, receipts and a
 * thread's To chips all ask here. `boards`: those whose lead it may be. */
export function chatAgentName(a: { machine: string; machine_name?: string | null; session?: string | null }, boards: readonly LedBoard[] = []): string {
  const session = a.session?.trim() ?? "";
  const led = session ? boards.find((b) => !b.closed_at && b.lead?.machine === a.machine && slug(b.lead.session) === slug(session)) : undefined;
  if (led?.title.trim()) return `${led.title.trim()} lead`;
  // `cos` is the handle that follows its owner's chief of staff (Arugula #1515).
  if (session === "cos") return ROLE.chief;
  return agentName({ session, machine: a.machine_name }, { names: "agent" });
}

/** A session's name as `@` routes it (chat/agents.ts `sessionSlug`). */
const slug = (name: string) => name.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-+|-+$/g, "");
