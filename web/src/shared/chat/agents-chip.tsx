// Whose agents reach a room, as one chip in its header (#122), instead of a
// line across the top of every visit. It opens a popover with each owner's
// machines and what their agents may do there, and a link to the grants for
// the reader's own. Nothing an agent can read is left out: the popover is
// the old lines, complete. (M197 disclosed them to everyone in the room;
// that hasn't changed.)

import { useEffect, useRef, useState } from "preact/hooks";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { useSubscribe } from "../ui/hooks";
import { agentOwners, chipLabel } from "./agents";
import { closeOnEscape } from "./escape";

export function AgentsChip({ room }: { room: string }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const api = usePlatformApi();
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const agents = chat.agents(room);
  useEffect(() => setOpen(false), [room]);
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    const key = closeOnEscape(() => setOpen(false));
    addEventListener("pointerdown", down, true);
    addEventListener("keydown", key);
    return () => {
      removeEventListener("pointerdown", down, true);
      removeEventListener("keydown", key);
    };
  }, [open]);
  if (!agents.length) return null;
  const owners = agentOwners(agents);
  return (
    <div class="agents-chip" ref={box} data-room-agents>
      <button class={open ? "chat-info agents-chip-btn on" : "chat-info agents-chip-btn"} aria-expanded={open} aria-haspopup="dialog" title="Agents that can read this room" data-agents-chip onClick={() => setOpen(!open)}>
        <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="5.5" width="10" height="7" rx="2" />
          <path d="M8 5.5V3M6.5 3h3" />
          <circle cx="6" cy="9" r=".6" fill="currentColor" />
          <circle cx="10" cy="9" r=".6" fill="currentColor" />
        </svg>
        <span class="agents-chip-text">{chipLabel(agents)}</span>
      </button>
      {open && (
        <div class="agents-pop" role="dialog" aria-label="Agents that can read this room" data-agents-pop>
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
      )}
    </div>
  );
}
