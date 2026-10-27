// The port hint's fact: a port listening from a pane of the shown tab
// (`pane.ports`, #150). The daemon doesn't announce a port, so the client
// asks, and only when the pane gives a reason to: a program runs in it
// (not the shell) and it started, or printed, since the last answer (a dev
// server says its URL once it listens). At most every EVERY_MS a pane, and
// only while the hint is unseen with Tips on (./index asks only then).

import { paneIds, type Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { panePorts } from "../blocks/browser";
import type { Fact } from "./index";
import { factChanged } from "./facts";

const EVERY_MS = 2000;

interface Asked {
  /** What the pane looked like when last asked about. */
  key: string;
  at: number;
  busy: boolean;
  timer: number;
  /** Its own listening ports, as last answered. */
  ports: number[];
}

const asked = new WeakMap<Client, Map<PaneId, Asked>>();

export function listeningPort(client: Client): Fact | null {
  if (!client.showsBlocks()) return null;
  const tab = client.tabView();
  if (!tab) return null;
  for (const id of paneIds(tab)) {
    const info = client.info(id);
    // On this host, a program's, and someone who may open things beside it.
    if (info?.type !== "terminal" || !info.command || info.host != null || client.machine(id) || !client.mayType(id)) continue;
    const port = ownPort(client, id, `${info.command}|${info.current?.started_ms ?? ""}|${info.activity?.last_ms ?? ""}`);
    if (port) return { pane: id, args: { pane: id, port } };
  }
  return null;
}

/** The pane's first own port, as last answered; asks again when `key` says
 * the pane moved on. */
function ownPort(client: Client, pane: PaneId, key: string): number | undefined {
  let mine = asked.get(client);
  if (!mine) asked.set(client, (mine = new Map()));
  let a = mine.get(pane);
  if (!a) mine.set(pane, (a = { key: "", at: 0, busy: false, timer: 0, ports: [] }));
  if (a.key !== key && !a.busy && !a.timer) {
    const wait = a.at + EVERY_MS - Date.now();
    const entry = a;
    const ask = () => {
      entry.timer = 0;
      entry.key = key;
      entry.at = Date.now();
      entry.busy = true;
      void panePorts(client, { split: pane, local: true }).then((ports) => {
        entry.busy = false;
        entry.ports = (ports ?? []).filter((p) => p.pane).map((p) => p.port);
        factChanged();
      });
    };
    if (wait <= 0) ask();
    else a.timer = window.setTimeout(() => (a.timer = 0, factChanged()), wait);
  }
  return a.ports[0];
}
