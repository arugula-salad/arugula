// An agent's name where its message shows (#632): by what it is, in teal
// ("Design lift lead", "Chief of staff", else its session's name, Arugula
// #1582), with its owner, machine and session in the tooltip. In a
// message's head it opens that pane when the reader reaches the machine
// (their own, or one shared with them); otherwise it's plain text and the
// tooltip says whose it is.

import type { ChatAgent, ChatMessage } from "../proto";
import { agentOf } from "../agent-label";
import { chatAgentName } from "../agent-name";
import { usePlatformApi } from "../platform-api";
import { useSubscribe } from "../ui/hooks";
import { authorLabel, machineHandle, sessionSlug } from "./agents";
import { useLedBoards } from "./project-boards";

/** `machine/tab`, the handle that reaches it, or the machine alone for a
 * post that names no tab (an older daemon, a machine-level post). */
export function agentHandle(a: ChatAgent): string {
  const session = a.session ? sessionSlug(a.session) : "";
  const machine = machineHandle(a.machine_name) || "a machine that left";
  return session ? `${machine}/${session}` : machine;
}

/** Whose agent it is, on which machine and in which session, for the
 * name's tooltip: what the name itself leaves out. */
function ownerTip(m: ChatMessage): string {
  const owner = m.author ? m.name || "someone" : "a deleted account";
  const on = m.agent!.machine_name || "a machine that left";
  return `${agentOf(owner)}, on ${on}${m.agent!.session ? ` · ${agentHandle(m.agent!)}` : ""}`;
}

/** The author's name in a message's head, a hit or a saved item: a person's
 * as written, an agent's by what it is (`chatAgentName`) in teal. With
 * `open`, the agent's opens its pane where the reader can reach it. */
export function AuthorName({ m, open, tag: Tag = "b" }: { m: ChatMessage; open?: boolean; tag?: "b" | "span" }) {
  const api = usePlatformApi();
  useSubscribe((fn) => api.subscribe(fn));
  const led = useLedBoards();
  const agent = m.agent;
  if (!agent) return <Tag class={open ? "thread-name" : undefined}>{authorLabel(m)}</Tag>;
  // As people read it; `data-agent-name` keeps the handle that reaches it.
  const handle = chatAgentName(agent, led);
  const routes = agentHandle(agent);
  const owner = ownerTip(m);
  const target = open && agent.session ? api.agentPane(agent.machine, agent.session) : undefined;
  if (!open || !target) {
    // Not the reader's to open (another's machine, or a page with no
    // machines): plain, and the tooltip says whose it is. A machine they do
    // reach whose tab is gone says so.
    const gone = open && !!agent.session && !!api.machine(agent.machine);
    return (
      <Tag class={open ? "thread-name chat-agent-name" : "chat-agent-name"} title={gone ? `${owner}. Its tab isn't open now` : owner} data-agent-name={routes} data-agent-shown={handle}>
        {handle}
      </Tag>
    );
  }
  return (
    <button
      type="button"
      class="thread-name chat-agent-name"
      title={target.up ? `${owner}. Open its pane` : `${owner}. ${agent.machine_name} is asleep`}
      data-agent-name={routes}
      data-agent-shown={handle}
      data-opens="1"
      onClick={() => api.goToPane(agent.machine, target.pane, null)}
    >
      {handle}
    </button>
  );
}
