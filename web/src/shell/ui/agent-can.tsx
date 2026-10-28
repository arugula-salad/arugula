// What your agent can do here: the things a person can ask an agent for on
// this machine, each as "Ask it to …" with a prompt to copy. The daemon
// makes the list from its MCP tools (`agent_can.get`: a line per tool, in
// `crates/daemon/src/mcp/tools.rs`), keeping those this machine offers (its
// Labs flags, a forge signed in to here, control and its grants), so a new
// tool shows here and a removed one goes. Getting started's Agents step
// shows it whole, the Start-an-agent dialog folded, and the Docs page's
// empty state only the Doc line.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { opPath, OPS } from "../ops";
import type { AgentCan } from "../../shared/proto";
import { CopyText } from "../../shared/ui/copy";

/** The lines from the daemon; none where it can't say (a guest, an older
 * daemon). */
function useAgentCan(client: Client): AgentCan[] | null {
  const [lines, setLines] = useState<AgentCan[] | null>(null);
  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const res = await client.request(OPS["agent_can.get"].method, opPath("agent_can.get"));
        const v = res.ok ? await res.json<AgentCan[]>().catch(() => null) : null;
        if (live) setLines(Array.isArray(v) ? v : []);
      } catch {
        if (live) setLines([]);
      }
    })();
    return () => {
      live = false;
    };
  }, [client]);
  return lines;
}

/** The lines, each with its example: copied, or, with `use`, put in the
 * prompt being written. */
function Lines({ lines, use }: { lines: AgentCan[]; use?: (prompt: string) => void }) {
  return (
    <ul class="agent-can-lines">
      {lines.map((l) => (
        <li key={l.tool} data-agent-can={l.tool}>
          <span class="agent-can-ask">Ask it to {l.ask}.</span>
          {use ? (
            <button type="button" class="link agent-can-example" data-agent-can-use title="Put it in the prompt" onClick={() => use(l.example)}>
              “{l.example}”
            </button>
          ) : (
            <CopyText inline text={l.example} data-agent-can-example />
          )}
        </li>
      ))}
    </ul>
  );
}

/** The card. `folded`: a closed "What it can do here" (the dialog's);
 * `only`: that tool's line alone, with no heading (the Docs page's). */
export function AgentCanCard({ client, folded, only, use }: { client: Client; folded?: boolean; only?: string; use?: (prompt: string) => void }) {
  const all = useAgentCan(client);
  const lines = only ? all?.filter((l) => l.tool === only) : all;
  if (!lines?.length) return null;
  if (only) return <Lines lines={lines} use={use} />;
  if (folded) {
    return (
      <details class="agent-can" data-agent-can-card>
        <summary>What it can do here</summary>
        <Lines lines={lines} use={use} />
      </details>
    );
  }
  return (
    <div class="agent-can" data-agent-can-card>
      <h3>What your agent can do here</h3>
      <Lines lines={lines} use={use} />
    </div>
  );
}
