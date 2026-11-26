// The app's own client to one of its machines (Arugula #1019, #1107), for
// the places that draw a machine's things outside its page: a board
// drawer's live session (./live-pane) and Settings › Machines
// (./machine-settings). Through the hub with a machine token, or a
// daemon's own origin; one client per machine, shared while something
// holds it, closed a little after the last lets go.

import { useEffect, useState } from "preact/hooks";
import { Client, type E2ETarget } from "../shell/client";
import { directory } from "../shell/hosts";
import { useSubscribe } from "../shared/ui/hooks";
import type { AppSession } from "./session";

/** How the app reaches a machine by control's id for it: the session's
 * target, or a daemon's own origin (`own`, on a daemon's own page). A
 * daemon's own page knows its hosts by name, not id: there, a machine
 * `own` doesn't name is a host in its list, reached as the shell does. */
export interface Reach {
  session: AppSession | null;
  own?: (machine: string) => string | undefined;
}

/** The client of each machine held: by a drawer's pane and its question, or by Settings. */
const held = new Map<string, { client: Client; n: number; timer?: ReturnType<typeof setTimeout> }>();
const started = new WeakSet<AppSession>();

function where(machine: string, reach: Reach): { base: string; target?: E2ETarget } | "wait" | null {
  const s = reach.session;
  if (!s) {
    const base = reach.own?.(machine) ?? (directory.names.includes(machine) ? directory.base(machine) : undefined);
    return base === undefined ? null : { base };
  }
  if (!started.has(s)) {
    started.add(s);
    s.start();
  }
  if (s.reachable === false) return null;
  const target = s.target(machine);
  if (target) return { base: `e2e:${machine}`, target };
  return s.phase === "loading" || s.reachable === null ? "wait" : null;
}

function hold(machine: string, at: { base: string; target?: E2ETarget }): Client {
  const h = held.get(machine);
  if (h) {
    clearTimeout(h.timer);
    h.n++;
    return h.client;
  }
  const client = new Client(at.base, at.target);
  client.connect();
  held.set(machine, { client, n: 1 });
  return client;
}

function letGo(machine: string) {
  const h = held.get(machine);
  if (!h || --h.n > 0) return;
  h.timer = setTimeout(() => {
    if (h.n > 0) return;
    h.client.close();
    held.delete(machine);
  }, 2000);
}

// For end-to-end tests: the clients the drawer holds.
Object.assign(window, { __live: { client: (machine: string) => held.get(machine)?.client } });

/** The machine's client while the component is up: null while it is
 * reached, "unreachable" when the app can't reach it from here. */
export function useLive(machine: string, reach: Reach): Client | null | "unreachable" {
  const s = reach.session;
  useSubscribe((fn) => s?.subscribe(fn) ?? (() => {}));
  const at = where(machine, reach);
  const ready = at !== null && at !== "wait";
  const [client, setClient] = useState<Client | null>(null);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!ready) return;
    const c = hold(machine, at);
    setClient(c);
    // Each change the machine announces redraws it (useSubscribe would hear only the first client).
    const off = c.subscribe(() => tick((n) => n + 1));
    return () => {
      off();
      setClient(null);
      letGo(machine);
    };
  }, [machine, ready]);
  if (at === null) return "unreachable";
  return client;
}

