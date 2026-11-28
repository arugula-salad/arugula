// Who a comment on a Doc reaches (#1258): the agent that wrote it, by an
// `@machine/session` mention in the comment's text, which is how a message
// at the top of a thread wakes an agent. A Doc records its session's name
// when the daemon that made it says it; an older Doc names only the machine,
// whose mention lands in that machine's inbox. A Doc an agent session on the
// platform made (`ses_…`), or a person, has no mention: its thread's members
// see the comment as they see any message there.

import type { ProjectDoc } from "../proto";
import { machineHandle, sessionSlug } from "../chat/agents";
import { shownSession } from "../chief-of-staff";

export interface Reach {
  /** The mention a comment ends with, or "". */
  mention: string;
  /** Who that is, for the page ("geek/docs-agent", or "Chief of staff"), or "". */
  label: string;
}

export function reachOf(doc: Pick<ProjectDoc, "agent">): Reach {
  const a = doc.agent;
  if (!a || a.machine.startsWith("ses_")) return { mention: "", label: "" };
  const machine = machineHandle(a.machine_name);
  if (!machine) return { mention: "", label: "" };
  const handle = a.session ? `${machine}/${sessionSlug(a.session)}` : machine;
  // A chief of staff's reads as itself; the mention still reaches its tab.
  const shown = a.session ? shownSession(a.session) : "";
  return { mention: `@${handle}`, label: shown && shown !== a.session ? shown : handle };
}
