// A connection per machine, drawing only the panes something on this page
// shows there (`client.only`): what a remote pane (#17) and a View's leaves
// (#503) stand on. A machine that can't be reached keeps retrying, so what it
// shows comes back by itself.

import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";

/** A host that dropped off the network closes nothing: its connection just
 * goes quiet. Ask a quiet host to answer this often... */
const HEARTBEAT_MS = 3000;
/** ...and give up on a link that said nothing for this long (counted from
 * the ask, see `Client.keepAlive`), or that hasn't connected in this long,
 * and connect again as after any drop. */
const SILENT_MS = 6000;

interface Link<V> {
  client: Client;
  /** Who's showing each of its panes. */
  views: Map<PaneId, Set<V>>;
  /** When the connect under way started (ms), if one is. */
  trying: number | null;
  /** Panes nobody shows any more, on their way out. */
  leaving: Map<PaneId, number>;
}

export class PaneLinks<V> {
  private hosts = new Map<string, Link<V>>();
  private timer: number | undefined;

  /**
   * @param make a full connection to `host`, or null when this page can't
   *   reach it.
   * @param lingerMs how long a pane nobody shows stays drawn: going from one
   *   place to another that shows it doesn't load it again.
   */
  constructor(
    private make: (host: string) => Client | null,
    private lingerMs = 0,
  ) {}

  /** Heartbeats, and dropping links that went quiet (as the fleet does). */
  private tick = () => {
    const now = Date.now();
    for (const l of this.hosts.values()) {
      const c = l.client;
      if (c.connected) {
        l.trying = null;
        c.keepAlive(now, HEARTBEAT_MS, SILENT_MS - HEARTBEAT_MS);
      } else if (c.linked) {
        l.trying ??= now;
        if (now - l.trying > SILENT_MS) {
          l.trying = null;
          c.drop("didn't connect in time");
        }
      } else {
        l.trying = null;
      }
    }
  };

  /** Show `pane` on `host` for `view`: the connection, or null. */
  take(host: string, pane: PaneId, view: V): Client | null {
    let link = this.hosts.get(host);
    if (!link) {
      const client = this.make(host);
      if (!client) return null;
      client.only = new Set();
      const l: Link<V> = { client, views: new Map(), trying: null, leaving: new Map() };
      client.connect();
      this.hosts.set(host, l);
      this.timer ??= window.setInterval(this.tick, 1000);
      link = l;
    }
    clearTimeout(link.leaving.get(pane));
    link.leaving.delete(pane);
    (link.views.get(pane) ?? link.views.set(pane, new Set()).get(pane)!).add(view);
    link.client.want(pane);
    return link.client;
  }

  release(host: string, pane: PaneId, view: V) {
    const link = this.hosts.get(host);
    const vs = link?.views.get(pane);
    if (!link || !vs?.delete(view) || vs.size) return;
    link.views.delete(pane);
    if (this.lingerMs === 0) return this.drop(host, link, pane);
    clearTimeout(link.leaving.get(pane));
    link.leaving.set(pane, window.setTimeout(() => this.drop(host, link, pane), this.lingerMs));
  }

  private drop(host: string, link: Link<V>, pane: PaneId) {
    link.leaving.delete(pane);
    if (link.views.has(pane)) return;
    link.client.unwant(pane);
    if (link.views.size > 0 || link.leaving.size > 0) return;
    link.client.close();
    this.hosts.delete(host);
    if (this.hosts.size === 0 && this.timer !== undefined) {
      window.clearInterval(this.timer);
      this.timer = undefined;
    }
  }

  /** For tests, and asking after a connection: the one to `host`, if any. */
  client(host: string): Client | undefined {
    return this.hosts.get(host)?.client;
  }

  /** Whether one of these connections is client `id` on the machine at `base`. */
  holds(base: string, id: number | null): boolean {
    if (id === null) return false;
    for (const { client } of this.hosts.values()) if (client.base === base && client.clientId === id) return true;
    return false;
  }
}
