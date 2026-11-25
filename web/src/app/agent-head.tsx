// An agent's tab head (Arugula #1342): what it is to the reader (its role,
// "Chief of staff"), the machine it runs on (a Cloud workspace by its own
// name) and whether anything hears a message now: awake while a turn
// runs, idle on its machine (a message wakes it), asleep while its machine
// is offline (a message waits), or none; what a message does is the
// state's tooltip. The chief of staff's tab draws it over its room; a
// board lead's and an issue agent's tab can draw it too, from `leadAwake`
// (../shell/swarm/overview.ts) over their session.
//
// Under it, its rung on the agent ladder (Arugula #1441): what it is for
// ("lead of Launch", "on #12 for Launch") and who started it; then its
// parent rung's latest word to it, the newest post in its room by the
// session above it (a board's lead for an issue agent, the chief of staff
// for a lead), so you see what it was told before you add yours.

import { useEffect } from "preact/hooks";
import type { ChatMessage } from "../shared/proto";
import { chat } from "../shared/chat-store";
import { useSubscribe } from "../shared/ui/hooks";
import type { Fleet } from "../shell/fleet";
import type { LeadAwake, OvSession } from "../shell/swarm/overview";
import { shortHost, whatItDoes } from "../shell/swarm/model";
import { cleanTitle } from "../shared/titles";

/** What a message does now, in the state's tooltip. */
const HINT: Partial<Record<LeadAwake["state"], string>> = { idle: "wakes on its own", asleep: "messages wait" };

/** An agent's rung (#1441): what it is for, who started it, and its parent's latest word to it. */
export interface RungLine {
  /** "lead of Launch", "on #12 for Launch". */
  line: string;
  /** Who started it, where the page can tell: "you", a session's name. */
  started?: string | null;
  /** Its parent's newest post to it, and what to call the parent. */
  word?: { who: string; m: ChatMessage } | null;
}

/** The head: `role`, then where it runs and how it stands; and its rung, under it. */
export function AgentHead({ role, at, rung }: { role: string; at: LeadAwake; rung?: RungLine }) {
  const m = at.state === "none" ? "" : shortHost(at.machine);
  const state = at.state === "none" ? "none" : at.state === "awake" ? `${m} · awake` : at.state === "idle" ? `idle on ${m}` : at.state === "asleep" ? `asleep: ${m} is offline` : m;
  const word = rung?.word;
  return (
    <>
      <span class="agent-head" data-agent-head={role} data-agent-state={at.state}>
        <b>{role}</b> · <span title={HINT[at.state]}>{state}</span>
      </span>
      {rung && (
        <span class="agent-rung" data-agent-rung>
          {[rung.line, rung.started && `started by ${rung.started}`].filter(Boolean).join(" · ")}
        </span>
      )}
      {word && (
        <span class="agent-word" data-agent-word={word.who} title={word.m.text}>
          Last from {word.who}: “{word.m.text.replace(/\s+/g, " ").trim()}”
        </span>
      )}
    </>
  );
}

/** Who started a session, as its agent's pane says (`started_by`): a
 * session's name where an agent block or a terminal started it through
 * MCP and is still here, else the MCP client; "you" (or a teammate) by
 * hand. Null where the page has no pane of it (a Cloud workspace's). */
export function starterOf(fleet: Fleet, s: OvSession): string | null {
  const ov = s.panes.find((p) => p.agent) ?? s.lead;
  const pane = ov && fleet.panes.find((p) => p.key === ov.key);
  if (!pane) return null;
  const by = pane.info.started_by;
  if (!by) return pane.person.kind === "me" ? "you" : pane.person.name || null;
  const at = by.block ?? by.from;
  const from = at === undefined ? undefined : fleet.panes.find((p) => p.host === pane.host && p.id === at);
  if (from) return cleanTitle(from.info.title ?? "") || whatItDoes(from);
  return by.by.replace(/^mcp:/, "");
}

/** The thread on an object (an issue, `iss_…`; a board, `brd_…`), as chat lists it once it's opened. */
export const objectRoom = (target: string) => chat.rooms.find((r) => r.kind === "object" && r.target === target);

/** A parent's newest post in `targets`' threads (#1441): by its machine and
 * session's name, as chat says an agent posted (`ChatMessage.agent`). The
 * threads are opened and read once while it's shown; null with chat off or
 * no parent. */
export function useParentWord(parent: { machine: string; session: string } | null, targets: string[]): ChatMessage | null {
  useSubscribe((fn) => chat.subscribe(fn));
  const on = !!chat.on && !!parent;
  const key = targets.join(",");
  useEffect(() => {
    if (!on) return;
    for (const t of targets) {
      const r = objectRoom(t);
      void (r ? Promise.resolve(r) : chat.openObject(t)).then((x) => chat.load(x.id)).catch(() => {});
    }
  }, [key, on]);
  if (!on) return null;
  let newest: ChatMessage | null = null;
  for (const t of targets) {
    const r = objectRoom(t);
    for (const m of (r && chat.messages(r.id)?.list) ?? [])
      if (!m.deleted && m.agent?.machine === parent!.machine && m.agent.session === parent!.session && (!newest || m.at > newest.at)) newest = m;
  }
  return newest;
}
