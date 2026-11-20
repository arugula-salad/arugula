// Pinning a session from the shell (Arugula
// iss_c0fd27ac196647fa7c857dc3659a0098): which machine a pin names, and
// the menu's Pin session. A pin is control's id for the machine, so a page
// that knows none (a daemon's own page, a machine not joined to an
// account) has no pins and offers none.

import type { Client } from "./client";
import { directory } from "./hosts";
import type { MenuItem } from "../shared/ui/menu";
import { isPinned, pin, pinnedOn, tidy, unpin } from "../shared/session-pins";
import { sessionPins } from "../shared/session-pins-store";

/** The page's own daemon's id in control's directory, where the page is a
 * stand-in for the app's (a fixture page over the suite's daemon). */
let home: string | null = null;
export function setHomeMachine(id: string | null) {
  home = id;
}

/** Control's id for the host named `name`, if this page knows it. */
export function machineOfHost(name: string | undefined): string | undefined {
  if (!name) return undefined;
  return directory.find(name)?.id ?? (name === directory.home ? (home ?? undefined) : undefined);
}

/** The machine `client` shows, as a pin names it, when this page can pin:
 * it knows the machine's id and the platform keeps pins for it. Asks for
 * the pins the first time. */
export function pinsMachine(client: Client): string | undefined {
  const machine = client.e2e?.daemon.id ?? machineOfHost(directory.current);
  if (!machine) return undefined;
  sessionPins.want();
  return sessionPins.on ? machine : undefined;
}

/** The sessions pinned on the machine `client` shows, in pin order. */
export const pinnedHere = (client: Client): number[] => pinnedOn(sessionPins.pins, pinsMachine(client));

/** Pin or unpin `session`, dropping the pins of this machine's sessions
 * that are gone while the list is written anyway. */
export function togglePin(client: Client, session: number) {
  const machine = pinsMachine(client);
  if (!machine) return;
  const there = client.state?.sessions.map((s) => s.id) ?? [];
  void sessionPins.edit((pins) => {
    const kept = tidy(pins, machine, there);
    return isPinned(kept, machine, session) ? unpin(kept, machine, session) : pin(kept, machine, session);
  });
}

/** Pin session or Unpin session, first in the session's menu; none where
 * the page can't pin. Anyone who sees a session may: it changes nothing
 * on the machine. */
export function pinItems(client: Client, session: number): MenuItem[] {
  const machine = pinsMachine(client);
  if (!machine) return [];
  const on = isPinned(sessionPins.pins, machine, session);
  return [
    {
      label: on ? "Unpin session" : "Pin session",
      hint: on ? "Take it out of Pinned" : "Keep it in Pinned, below Needs you, on every device",
      run: () => togglePin(client, session),
      attrs: { "data-session-pin": on ? "on" : "off" },
    },
  ];
}
