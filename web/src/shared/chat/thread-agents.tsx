// The agents in a thread (#646): who's here, what each is doing, and how far
// each has read. One agent is a single line, several are a card each, and on a
// phone they fold into one line. Control asks each machine when this asks it
// (aru-133), so it's read while the thread is open and in view, and again when
// a reply comes in. Nothing for a thread with no agents, or one whose only agent
// is a hosted session: its session card is already in the thread.

import { useEffect, useState } from "preact/hooks";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import type { ChatMessage, ChatThreadAgent, ChatThreadAgentState } from "../proto";
import { sessionSlug } from "./agents";
import "./thread-agents.css";

/** How often an open thread asks again. */
const EVERY_MS = 15_000;

const STATE: Record<ChatThreadAgentState, string> = {
  working: "Working",
  idle: "Idle",
  needs_you: "Needs you",
  offline: "Offline",
  unknown: "Unknown",
};

/** What an agent goes by: a machine told with no session yet has no name. */
export function agentName(a: ChatThreadAgent): string {
  return a.name || `an agent on ${a.machine_name}`;
}

/** Its last line: what's waiting for it, or how far it has read. */
export function readLine(a: ChatThreadAgent, newest: number | undefined): string {
  if (a.waiting > 0) return `${a.waiting} waiting`;
  if (a.state === "offline" || a.state === "unknown") return "";
  if (a.read_seq === undefined || newest === undefined) return "";
  return a.read_seq >= newest ? "Read everything" : "Not woken since";
}

/** The folded line on a phone. */
export function summary(agents: ChatThreadAgent[]): string {
  const count = (s: ChatThreadAgentState) => agents.filter((a) => a.state === s).length;
  const waiting = agents.reduce((n, a) => n + a.waiting, 0);
  const parts = [`${agents.length} agent${agents.length === 1 ? "" : "s"}`];
  if (count("working")) parts.push(`${count("working")} working`);
  if (count("needs_you")) parts.push(`${count("needs_you")} needs you`);
  if (waiting) parts.push(`${waiting} waiting`);
  return parts.join(" · ");
}

/** A thread's agents, read while it's open. */
export function useThreadAgents(room: string, parent: number, newest: number | undefined): ChatThreadAgent[] {
  const [agents, setAgents] = useState<ChatThreadAgent[]>([]);
  useEffect(() => {
    setAgents([]);
  }, [room, parent]);
  useEffect(() => {
    let live = true;
    const read = () => {
      if (document.visibilityState !== "visible") return;
      chat.threads
        .agents(room, parent)
        .then((r) => live && setAgents(r.agents))
        .catch(() => {});
    };
    read();
    const every = setInterval(read, EVERY_MS);
    return () => {
      live = false;
      clearInterval(every);
    };
  }, [room, parent, newest]);
  return agents;
}

function Now({ a }: { a: ChatThreadAgent }) {
  if (!a.working_on && !a.tool) return null;
  return (
    <span class="thread-agent-now">
      {a.working_on && <span class="thread-agent-chip">{a.working_on}</span>} {a.tool}
    </span>
  );
}

function Detail({ a }: { a: ChatThreadAgent }) {
  const api = usePlatformApi();
  const mine = a.owner === chat.me;
  const who = a.hosted
    ? `The project's agent on ${a.machine_name}. Its owners and editors can stop it.`
    : mine
      ? `Your agent on ${a.machine_name}. Only you can stop it.`
      : `${a.owner_name || "A teammate"}'s agent on ${a.machine_name}. Anyone here can message it, but only they can stop it.`;
  return (
    <div class="thread-agent-detail" data-thread-agent-detail>
      <p>{who}</p>
      {mine && !a.hosted && api.machine(a.id) && (
        <p>
          <button class="btn" data-thread-agent-open onClick={() => api.goToSession(a.id)}>
            Open on {a.machine_name}
          </button>
        </p>
      )}
      <p>Arugula shows its state, and its work item and tool name if its owner shares them. Never its output, tool arguments or paths.</p>
    </div>
  );
}

export function ThreadAgents({ agents, newest, phone }: { agents: ChatThreadAgent[]; newest: number | undefined; phone?: boolean }) {
  const [open, setOpen] = useState<string | null>(null);
  const [unfolded, setUnfolded] = useState(false);
  if (!agents.length || (agents.length === 1 && agents[0].hosted)) return null;
  const key = (a: ChatThreadAgent) => `${a.id}/${a.name}`;
  const shown = agents.find((a) => key(a) === open);
  const toggle = (a: ChatThreadAgent) => setOpen(open === key(a) ? null : key(a));
  const where = (a: ChatThreadAgent) => `${a.hosted ? "hosted" : `${a.owner_name || "a teammate"}'s`} · ${a.machine_name}`;
  if (agents.length === 1) {
    const a = agents[0];
    const line = readLine(a, newest);
    return (
      <section class="thread-agents thread-agents-one" aria-label="The agent in this thread" data-thread-agents="one">
        <button class={`thread-agent ${a.state}`} aria-expanded={!!shown} data-thread-agent={key(a)} onClick={() => toggle(a)}>
          <span class="thread-agent-state">{STATE[a.state]}</span>
          <span class="thread-agent-name">{agentName(a)}</span>
          <Now a={a} />
          {line && <span class={a.waiting ? "thread-agent-read waits" : "thread-agent-read"}>{line}</span>}
        </button>
        {shown && <Detail a={shown} />}
      </section>
    );
  }
  const folded = phone && !unfolded;
  return (
    <section class={phone ? "thread-agents phone" : "thread-agents"} aria-label="The agents in this thread" data-thread-agents="many">
      {phone && (
        <button class="thread-agents-sum" aria-expanded={unfolded} data-thread-agents-sum onClick={() => setUnfolded(!unfolded)}>
          {unfolded ? "▾" : "▸"} {summary(agents)}
        </button>
      )}
      {!folded && (
        <div class="thread-agents-cards">
          {agents.map((a) => {
            const line = readLine(a, newest);
            return (
              <button key={key(a)} class={`thread-agent ${a.state}`} aria-expanded={shown === a} data-thread-agent={key(a)} onClick={() => toggle(a)}>
                <div class="thread-agent-top">
                  <span class="thread-agent-name">{agentName(a)}</span>
                  {a.lead && <span class="thread-agent-lead">LEAD</span>}
                  <span class="thread-agent-state">{STATE[a.state]}</span>
                </div>
                <div class="thread-agent-where">{where(a)}</div>
                <Now a={a} />
                {line && <div class={a.waiting ? "thread-agent-read waits" : "thread-agent-read"}>{line}</div>}
              </button>
            );
          })}
        </div>
      )}
      {!folded && shown && <Detail a={shown} />}
    </section>
  );
}

/** Under a message: the agents that have read this far and no further, but
 * for the one that wrote it. */
export function SeenMarks({ agents, m }: { agents: ChatThreadAgent[]; m: ChatMessage }) {
  const wrote = (a: ChatThreadAgent) => m.agent?.machine === a.id && sessionSlug(m.agent?.session ?? "") === sessionSlug(a.name);
  const here = agents.filter((a) => !a.hosted && a.read_seq === m.seq && !wrote(a));
  if (!here.length) return null;
  return (
    <div class="thread-seen" title="How far each agent has read" data-thread-seen>
      {here.map((a) => (
        <span key={`${a.id}/${a.name}`}>{agentName(a)}</span>
      ))}
    </div>
  );
}
