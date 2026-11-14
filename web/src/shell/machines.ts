// The machines a page reaches (Arugula #989): one full client for the
// machine shown (M4a) and a summaries-only fleet of every machine (M25),
// kept apart from the page that draws them. Control's page and the daemon's
// own (./main.tsx) use it today; the app on app.arugula.io will, with a
// session of its own in place of control's.

import { Client, type E2ETarget } from "./client";
import type { ControlSession } from "./control";
import { Fleet, type HostRef } from "./fleet";
import { directory } from "./hosts";
import { setFleet } from "./ui/hosts";
import { shareSession } from "./ui/people";

/** What the machines need of an account: control's session on control's
 * page. With none, the page is a daemon's own and reaches its hosts
 * directly. */
export type MachineSource = Pick<ControlSession, "phase" | "daemons" | "stale" | "teams" | "login" | "subscribe" | "target">;

/** The account's machines, into the host list. */
export function followSource(s: MachineSource) {
  s.subscribe(() =>
    directory.setControl(
      s.daemons.map((d) => ({
        name: d.name,
        id: d.id,
        urls: d.urls,
        transport: "control" as const,
        added_ms: 0,
        last_seen_ms: d.last_seen,
        status: d.online ? "online" : "offline",
      })),
      s.stale,
    ),
  );
}

export class Machines {
  /** The client of the machine shown. Switching machines closes it and
   * opens one to the other daemon, so hidden machines hold no connection. */
  client: Client;
  readonly fleet: Fleet;
  private swaps = new Set<() => void>();
  private swapped = new Set<() => void>();

  /** `link`: a read-only link's one daemon (M19), and no host list. */
  constructor(
    readonly source: MachineSource | null,
    readonly link?: E2ETarget,
  ) {
    this.client = this.make(directory.base());
    this.fleet = new Fleet((h) => {
      if (source) {
        const t = h.id ? source.target(h.id) : undefined;
        return t ? new Client(`e2e:${h.id}`, t, true) : null;
      }
      return new Client(directory.base(h.name), undefined, true);
    });
    this.fleet.onOpen = (host, pane, share, reveal, missed) => {
      directory.select(host);
      // The host's client is made when the directory switches, then connects:
      // show the pane once it knows it.
      const until = Date.now() + 15_000;
      const go = () => {
        const client = this.client;
        if (client.base === directory.base(host) && client.info(pane)) {
          client.setActive(pane);
          if (share) this.shareFrom(pane);
          // A chat quote (M123): its lines in the pane, once it has drawn.
          // A search hit (native #245) says so when it's scrolled out.
          if (reveal)
            requestAnimationFrame(() => {
              if (!client.panes.get(pane)?.view.reveal(reveal) && missed) client.toast(missed);
            });
          return;
        }
        if (Date.now() < until) setTimeout(go, 100);
      };
      go();
    };
  }

  private make(base: string): Client {
    const { source, link } = this;
    if (link) return new Client(`e2e:${link.daemon.id}`, link);
    if (source && base.startsWith("e2e:")) return new Client(base, source.target(base.slice(4)));
    return new Client(base);
  }

  /** Hear each swap to another machine's client. */
  onSwap(fn: () => void): () => void {
    this.swaps.add(fn);
    return () => void this.swaps.delete(fn);
  }

  /** Call `done` with the shown machine's client, again on each change it
   * announces and on each swap to another, until it returns true. What the
   * page opens from its address on load needs this: on control's page the
   * client at first is a stand-in on control's own origin, there until the
   * machine list comes and `follow` swaps in the machine's (relayed) one. A
   * request through the stand-in reaches control, not the machine. */
  untilShown(done: (c: Client) => boolean) {
    let on: Client | null = null;
    let off = () => {};
    const check = () => {
      if (done(this.client)) {
        off();
        this.swapped.delete(check);
        return;
      }
      // Once per client: re-adding a listener while it emits would revisit it.
      if (on === this.client) return;
      off();
      on = this.client;
      off = this.client.subscribe(check);
    };
    this.swapped.add(check);
    check();
  }

  /** Every machine at once, for the swarm. Not on a link's page. */
  startFleet() {
    const { fleet, source } = this;
    setFleet(fleet);
    if (this.link) return;
    // Teams load after the directory: their names come with them (M30).
    source?.subscribe(() => {
      if (source.login) fleet.me = source.login;
      fleet.setHosts(this.fleetHosts());
    });
    directory.subscribe(() => fleet.setHosts(this.fleetHosts()));
    fleet.setHosts(this.fleetHosts());
    fleet.start();
  }

  private fleetHosts(): HostRef[] {
    const { source } = this;
    return directory.names.map((name) => {
      const h = directory.find(name);
      const d = source?.daemons.find((x) => x.id === h?.id);
      return {
        name,
        id: h?.id,
        transport: name === directory.home ? "home" : (h?.transport ?? "tailnet"),
        status: h?.status,
        owner: d?.account ? (d.owner_name ?? d.account) : undefined,
        team: d?.team ?? null,
        ownerId: d?.account,
        teamName: d?.team ? source?.teams.find((t) => t.team === d.team)?.roster.name : undefined,
        hosted: !!d?.hosted,
      };
    });
  }

  /** Follow the host list: a client for the machine it shows. A link's
   * page shows its one daemon, so it doesn't. */
  followDirectory() {
    if (this.link) return;
    const follow = () => {
      const base = directory.base();
      if (base === this.client.base) return;
      this.client.close();
      this.client = this.make(base);
      for (const fn of [...this.swaps]) fn();
      for (const check of [...this.swapped]) check();
    };
    directory.subscribe(follow);
    // Control's machine list may have come while the page waited on the
    // fonts (#749): its change was announced before anyone listened, and
    // the page would show "Add a machine" until the next refresh.
    follow();
    void directory.refresh();
  }

  /** Show `pane`: on `daemon`'s machine when named (a notification through
   * control), else the home daemon's (notifications come from it). */
  openPane(pane: number, daemon?: string, share = false, follow?: string) {
    const host = daemon ? directory.list?.hosts.find((h) => h.id === daemon)?.name : undefined;
    if (host) directory.select(host);
    else if (directory.home !== null) directory.select(directory.home);
    let watched = false;
    this.untilShown((client) => {
      if (!client.info(pane)) return false;
      client.setActive(pane);
      if (share) this.shareFrom(pane);
      // Once: watching emits, and this runs again from inside that.
      if (follow && !watched) {
        watched = true;
        client.watch(follow, pane);
      }
      return true;
    });
  }

  /** #237: the share picker for a pane's session, from a card or a push,
   * once the pane is shown. Only its owner shares (as in the session menu);
   * anyone else just gets the pane. */
  private shareFrom(pane: number) {
    const s = this.client.sessionOfPane(pane);
    if (s !== null && !this.client.state?.roles) shareSession(s);
  }
}
