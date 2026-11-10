// Whose agents reach a room, as one chip in its header (#122), instead of a
// line across the top of every visit. It opens a popover with each owner's
// machines and what their agents may do there, and a link to the grants for
// the reader's own. Nothing an agent can read is left out: the popover is
// the old lines, complete. (M197 disclosed them to everyone in the room;
// that hasn't changed.) The popover is shared/ui/menu.tsx's.

import { useEffect, useRef, useState } from "preact/hooks";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { useSubscribe } from "../ui/hooks";
import { agentOwners, chipLabel } from "./agents";
import { Popover, pressedShut } from "../ui/menu";
import { MachineIcon } from "../ui/icons";

export function AgentsChip({ room }: { room: string }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const api = usePlatformApi();
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const agents = chat.agents(room);
  useEffect(() => setOpen(false), [room]);
  if (!agents.length) return null;
  const owners = agentOwners(agents);
  return (
    <div class="agents-slot" data-room-agents>
      <button
        ref={button}
        class={open ? "chat-info bar-btn agents-chip-btn on" : "chat-info bar-btn agents-chip-btn"}
        aria-expanded={open}
        aria-haspopup="dialog"
        title="Agents that can read this room"
        data-agents-chip
        // A press on it shut the popover: this click is the same tap.
        onClick={() => pressedShut(button.current) || setOpen(!open)}
      >
        <MachineIcon />
        <span class="agents-chip-text">{chipLabel(agents)}</span>
      </button>
      {open && (
        <Popover from={button} end label="Agents that can read this room" attrs={{ "data-agents-pop": true }} onClose={() => setOpen(false)}>
          <div class="agents-pop">
            <p class="agents-pop-lede">Agents on these machines can read this room. Everyone in it sees this list.</p>
            {owners.map((o) => (
              <section key={o.owner} data-agents-owner={o.owner}>
                <h3>{o.owner === chat.me ? `${o.name} (you)` : o.name}</h3>
                <ul>
                  {o.machines.map((m) => (
                    <li key={m.machine} data-agents-machine={m.machine}>
                      <span class="agents-pop-machine">{m.name}</span>
                      <span class="agents-pop-does">{m.does}</span>
                    </li>
                  ))}
                </ul>
              </section>
            ))}
            {api.nav.openGrants && (
              <button
                class="agents-pop-link"
                data-agents-grants
                onClick={() => {
                  setOpen(false);
                  api.nav.openGrants?.();
                }}
              >
                Manage your agents' access
              </button>
            )}
          </div>
        </Popover>
      )}
    </div>
  );
}
