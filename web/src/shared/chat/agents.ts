// Agents in chat (M197): how a message an agent wrote is labelled, and the
// line a room shows when an agent grant reaches it. Plain functions, for
// ./room and the search results, and their tests
// (scripts/agents.test.mjs).

import type { ChatMessage, ChatRoomAgent } from "../proto";

/** A line the platform wrote in a room (#122: a grant let agents in), not
 * a person: no author, and `via` says so. Drawn as a note, not a message. */
export function isSystemLine(m: ChatMessage): boolean {
  return m.via === "system" && !m.author;
}

/** Who wrote a message, as it's shown: its author's name, or "<owner>'s
 * agent on <machine>" for one an agent wrote (M197), everywhere it shows. */
export function authorLabel(m: ChatMessage): string {
  if (isSystemLine(m)) return "Arugula";
  // What an agent session said in its thread (Arugula #215): the session's.
  if (!m.author && m.via?.startsWith("said:ses_")) return `Agent session ${m.via.slice("said:ses_".length, "said:ses_".length + 8)}`;
  const name = m.author ? m.name || "someone" : "a deleted account";
  return m.agent ? `${name}'s agent on ${m.agent.machine_name || "a machine that left"}` : name;
}

/** "a, b and c". */
function listed(names: string[]): string {
  return names.length < 2 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

/** What a grant's powers let an agent do in a room. */
const DOES: [string, string][] = [
  ["threads.read", "read"],
  ["threads.post", "post"],
  ["issues.write", "edit this issue"],
];

/** What a room says of the agents a grant lets in (M197): one line per
 * owner and what theirs may do, "Jake's agents on geek can read and post
 * here". */
export function agentLines(agents: ChatRoomAgent[]): string[] {
  const by = new Map<string, { owner: string; machines: string[]; what: string }>();
  for (const a of agents) {
    const what = listed(DOES.filter(([p]) => a.powers.includes(p)).map(([, w]) => w));
    if (!what) continue;
    const key = `${a.owner}\n${what}`;
    const had = by.get(key) ?? { owner: a.owner_name || "Someone", machines: [], what };
    had.machines.push(a.machine_name || a.machine);
    by.set(key, had);
  }
  return [...by.values()].map((g) => `${g.owner}'s agents on ${listed(g.machines)} can ${g.what} here`);
}


/** One owner's machines in a room and what their agents may do there, for
 * the header chip's popover (#122). */
export interface AgentOwner {
  owner: string;
  name: string;
  machines: { machine: string; name: string; does: string }[];
}

/** Who reaches a room, by owner then machine, nothing left out: a machine
 * whose grant gives no power the room names still shows, with "nothing
 * here", so no agent that can read a room is hidden from its people. */
export function agentOwners(agents: ChatRoomAgent[]): AgentOwner[] {
  const by = new Map<string, AgentOwner>();
  for (const a of agents) {
    const g = by.get(a.owner) ?? { owner: a.owner, name: a.owner_name || "Someone", machines: [] };
    g.machines.push({
      machine: a.machine,
      name: a.machine_name || a.machine,
      does: listed(DOES.filter(([p]) => a.powers.includes(p)).map(([, w]) => w)) || "nothing here",
    });
    by.set(a.owner, g);
  }
  const out = [...by.values()].sort((x, y) => x.name.localeCompare(y.name));
  for (const g of out) g.machines.sort((x, y) => x.name.localeCompare(y.name));
  return out;
}

/** The chip's words: "3 machines' agents". */
export function chipLabel(agents: ChatRoomAgent[]): string {
  const n = new Set(agents.map((a) => a.machine)).size;
  return `${n} ${n === 1 ? "machine's" : "machines'"} agents`;
}
