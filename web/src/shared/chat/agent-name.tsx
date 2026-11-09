// An agent's name where its message shows (#632): its tab as a mention tags
// it, `machine/tab`, in teal, with its owner in the tooltip. In a message's
// head it opens that pane when the reader reaches the machine (their own, or
// one shared with them); otherwise it's plain text and the tooltip says whose
// it is.

import type { ChatAgent, ChatMessage } from "../proto";
import { usePlatformApi } from "../platform-api";
import { useSubscribe } from "../ui/hooks";
import { authorLabel, machineHandle, sessionSlug } from "./agents";

/** `machine/tab`, or the machine alone for a post that names no tab (an
 * older daemon, a machine-level post). */
export function agentHandle(a: ChatAgent): string {
  const machine = machineHandle(a.machine_name) || "a machine that left";
  const tab = a.session ? sessionSlug(a.session) : "";
  return tab ? `${machine}/${tab}` : machine;
}

/** Whose agent it is, for the name's tooltip. */
function ownerTip(m: ChatMessage): string {
  const owner = m.author ? m.name || "someone" : "a deleted account";
  const on = m.agent!.machine_name || "a machine that left";
  return `${owner}'s agent, on ${on}`;
}

/** The author's name in a message's head, a hit or a saved item: a person's
 * as written, an agent's as `machine/tab` in teal. With `open`, the agent's
 * opens its pane where the reader can reach it. */
export function AuthorName({ m, open, tag: Tag = "b" }: { m: ChatMessage; open?: boolean; tag?: "b" | "span" }) {
  const api = usePlatformApi();
  useSubscribe((fn) => api.subscribe(fn));
  const agent = m.agent;
  if (!agent) return <Tag class={open ? "thread-name" : undefined}>{authorLabel(m)}</Tag>;
  const handle = agentHandle(agent);
  const owner = ownerTip(m);
  const target = open && agent.session ? api.agentPane(agent.machine, agent.session) : undefined;
  if (!open || !target) {
    // Not the reader's to open (another's machine, or a page with no
    // machines): plain, and the tooltip says whose it is. A machine they do
    // reach whose tab is gone says so.
    const gone = open && !!agent.session && !!api.machine(agent.machine);
    return (
      <Tag class={open ? "thread-name chat-agent-name" : "chat-agent-name"} title={gone ? `${owner}. Its tab isn't open now` : owner} data-agent-name={handle}>
        {handle}
      </Tag>
    );
  }
  return (
    <button
      type="button"
      class="thread-name chat-agent-name"
      title={target.up ? `${owner}. Open its pane` : `${owner}. ${agent.machine_name} is asleep`}
      data-agent-name={handle}
      data-opens="1"
      onClick={() => api.goToPane(agent.machine, target.pane, null)}
    >
      {handle}
    </button>
  );
}
