// Loading the Labs chunk (./labs). Core code calls `loadLabs()` only when the
// machine it is talking to has labs (`Client.hasLabs()`), so a session
// without them never fetches the chunk. `labsNow()` is the module once it has
// arrived, for code that has to answer at once (a menu being built).

import type { Client } from "./client";
import type { AppsWhere } from "./ui/apps";

export type Labs = typeof import("./labs");

let loaded: Labs | null = null;
let pending: Promise<Labs> | null = null;
const waiting = new Set<() => void>();

/** The Labs chunk, fetched once. A failed fetch can be tried again. */
export function loadLabs(): Promise<Labs> {
  pending ??= import("./labs").then(
    (m) => {
      loaded = m;
      waiting.forEach((fn) => fn());
      return m;
    },
    (e) => {
      pending = null;
      throw e;
    },
  );
  return pending;
}

/** The Labs module if it has been loaded, else null. */
export function labsNow(): Labs | null {
  return loaded;
}

/** Run `fn` when the Labs chunk has loaded; returns how to stop waiting. */
export function onLabsLoaded(fn: () => void): () => void {
  waiting.add(fn);
  return () => void waiting.delete(fn);
}

function failed(what: string, client: Client | null) {
  return (e: unknown) => {
    console.error(`${what} didn't load`, e);
    client?.toast(`Couldn't load ${what}`);
  };
}

/** The studio apps picker (a Labs menu item). */
export function pickApp(client: Client, where: AppsWhere, phone = false) {
  void loadLabs().then((l) => l.pickApp(client, where, phone), failed("studio apps", client));
}

/** The sandboxes page (a Labs menu item). */
export function openSandboxes(client: Client | null = null) {
  void loadLabs().then((l) => l.openSandboxes(), failed("sandboxes", client));
}
