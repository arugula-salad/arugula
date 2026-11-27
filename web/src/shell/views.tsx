// What a page draws of its machines (Arugula #989): the machine shown (its
// sessions, tabs and panes). Control's page and the daemon's
// own draw them in their own frame (./main.tsx); the app on app.arugula.io
// will draw them in its.

import { useEffect } from "preact/hooks";
import { PageGuard } from "../shared/ui/guard";
import type { Client } from "./client";
import { App, type Frame } from "./ui/app";
import type { Cell } from "./ui/cells";
import { PopOutPage } from "./ui/panes/pop-out";

/** The machine shown: its sessions, tabs and panes, or, popped out into a
 * window of its own (#497, `pop`), one pane alone. */
export function MachineView({ client, cell, pop, frame }: { client: Client; cell: Cell; pop?: { daemon?: string; pane: number } | null; frame?: Frame }) {
  // The workspace guards its own places (ui/app.tsx, Arugula #615).
  return pop ? (
    <PageGuard>
      <PopOutPage client={client} pane={pop.pane} daemon={pop.daemon} />
    </PageGuard>
  ) : (
    <App key={client.base} client={client} cell={cell} frame={frame} />
  );
}

/** Between knowing the account's machines and showing one. */
export function Connecting({ name }: { name: string }) {
  return (
    <div class="empty" data-connecting>
      <p>Connecting to {name || "your machine"}…</p>
    </div>
  );
}

/** An old `#swarm` or `#agents` link (Swarm and the Agents page went in
 * Lift 6a, #1483; the fragments stay forever): it lands on Machines, at the
 * pane a `#swarm` named where this page has it. */
export function OldLinkLanding({ client }: { client: Client }) {
  useEffect(() => {
    const land = () => {
      const m = /^#(?:swarm(?:=(?:board:[\w-]+|(?:[0-9a-f]+\.)?(\d+)))?|agents(?:=team)?)$/.exec(location.hash);
      if (!m) return;
      history.replaceState(history.state, "", location.pathname + location.search);
      dispatchEvent(new HashChangeEvent("hashchange"));
      const pane = m[1] ? Number(m[1]) : undefined;
      if (pane !== undefined && client.info(pane)) client.setActive(pane);
    };
    land();
    addEventListener("hashchange", land);
    return () => removeEventListener("hashchange", land);
  }, [client]);
  return null;
}
