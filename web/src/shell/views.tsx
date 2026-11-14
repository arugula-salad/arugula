// What a page draws of its machines (Arugula #989): the machine shown (its
// sessions, tabs and panes) and the swarm. Control's page and the daemon's
// own draw them in their own frame (./main.tsx); the app on app.arugula.io
// will draw them in its.

import { useEffect, useState } from "preact/hooks";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import type { Client } from "./client";
import { directory } from "./hosts";
import type { Machines } from "./machines";
import { closeSwarm, onSwarmRoute, swarmRoute } from "./swarm/route";
import { SwarmView } from "./swarm/view";
import { App, type Frame } from "./ui/app";
import { AppBar } from "./ui/app-bar";
import type { Cell } from "./ui/cells";
import { PopOutPage } from "./ui/panes/pop-out";

/** The machine shown: its sessions, tabs and panes, or, popped out into a
 * window of its own (#497, `pop`), one pane alone. */
export function MachineView({ client, cell, pop, frame }: { client: Client; cell: Cell; pop?: { daemon?: string; pane: number } | null; frame?: Frame }) {
  return pop ? <PopOutPage client={client} pane={pop.pane} daemon={pop.daemon} /> : <App key={client.base} client={client} cell={cell} frame={frame} />;
}

/** Between knowing the account's machines and showing one. */
export function Connecting({ name }: { name: string }) {
  return (
    <div class="empty" data-connecting>
      <p>Connecting to {name || "your machine"}…</p>
    </div>
  );
}

/** The swarm (M26) over the tab view, when `/#swarm` says so. Not on a
 * link's page, nor while the account isn't `ready`. */
export function Swarm({ machines, client, ready }: { machines: Machines; client: Client; ready: () => boolean }) {
  const [, set] = useState(0);
  useEffect(() => onSwarmRoute(() => set((n) => n + 1)), []);
  // The home machine's name arrives with the directory, after a cold load.
  useSubscribe((fn) => directory.subscribe(fn));
  // The shared bar shows once the shown machine has answered.
  useSubscribe((fn) => client.subscribe(fn));
  const phone = usePhone();
  const route = swarmRoute();
  if (!route || machines.link || !ready()) return null;
  const f = route.focus;
  // A notification through control names its daemon; else it's this page's.
  const host = f ? (f.daemon ? directory.list?.hosts.find((h) => h.id === f.daemon)?.name : (directory.home ?? directory.names[0])) : undefined;
  const view = (bar: boolean) => (
    <SwarmView fleet={machines.fleet} back={closeSwarm} focus={f && host ? { host, pane: f.pane } : null} board={route.board} inBar={bar} />
  );
  // The bar every place shares, over it, once a machine is shown; a phone
  // has its own header, and the swarm its Panes button.
  if (!client.state || phone) return view(false);
  return (
    <div class="swarm-shell">
      <AppBar client={client} at="swarm" all />
      {view(true)}
    </div>
  );
}
