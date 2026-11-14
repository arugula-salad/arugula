// The machines inside the app (Arugula #989): the shell's machine page,
// unchanged but for the shell's rail (the app's is beside it), and Swarm,
// at the project's `/machines…` places. One `Machines` for the page, made
// the first time one of these is shown: a client for the machine shown and
// a summaries-only fleet of every machine, over the app's session (or, on
// a daemon's own page, the daemon's hosts).

import "../shell/shell-chrome.css";
import "../shell/pane-chrome.css";
import "../shell/interaction-chrome.css";
import "../shell/block-chrome.css";
import "../shell/phone-sheet.css";
import "../shell/phone-machines.css";
import "../shell/docs-page.css";
import "../shell/settings-page.css";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { machinesPath, type MachineAt } from "../shared/machine-paths";
import { PlatformApiContext } from "../shared/platform-api";
import { EmptyState } from "../shared/ui/block-states";
import { Card } from "../shared/ui/card";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { PageHeader } from "../shared/ui/page-header";
import { Chip, StateChip } from "../shared/ui/state-chip";
import { directory } from "../shell/hosts";
import { followSource, Machines } from "../shell/machines";
import { shellPlatformApi } from "../shell/platform-api";
import { whatItDoes } from "../shell/swarm/model";
import { SwarmView } from "../shell/swarm/view";
import { AgentDialogLayer, startAgent } from "../shell/ui/agent-dialog";
import { measureCell, type Cell } from "../shell/ui/cells";
import { installKeys } from "../shell/ui/keys";
import { MachineView, Swarm } from "../shell/views";
import { names } from "../platform/links";
import { current, follow, go, pathOf } from "../platform/router";
import type { Fleet, FleetHost } from "../shell/fleet";
import { ROLE_WORDS, rowsOf, sessionsLine, sortRows } from "./machine-list";
import type { AppSession } from "./session";
import type { Client } from "../shell/client";
import { LiveSlot } from "../shell/ui/panes/live-slot";
import { notePane } from "../platform/places-kept";

interface Booted {
  machines: Machines;
  cell: Cell;
}

let booted: Promise<Booted> | null = null;
/** The machines, once booted: what `panes` reads. */
let up: Booted | null = null;

/** The page's machines, made once. */
function boot(session: AppSession | null): Promise<Booted> {
  return (booted ??= (async () => {
    // The route names the machine shown, not `?machine=`.
    directory.keepsAddress = false;
    if (session) {
      followSource(session);
      session.start();
      await answered(session);
    }
    const machines = new Machines(session);
    // With a session, only a machine's own client connects: until the
    // list comes, the client is a stand-in on this page's origin.
    const connect = () => {
      if (!session || machines.client.e2e) machines.client.connect();
    };
    machines.startFleet();
    machines.onSwap(connect);
    machines.followDirectory();
    connect();
    // A pane opened from Swarm (or a card) shows on its machine's page.
    const opened = machines.fleet.onOpen;
    machines.fleet.onOpen = (host, pane, ...rest) => {
      opened?.(host, pane, ...rest);
      const r = current();
      const id = directory.find(host)?.id;
      if (r.page === "machines") go(pathOf({ page: "machines", project: r.project, at: id ? { at: "machine", machine: id, pane } : { at: "machines", name: host, pane } }, names));
    };
    // For end-to-end tests, as the shell's page has `window.__arugula`.
    Object.assign(window, {
      __app: {
        get client() {
          return machines.client;
        },
        text: (pane: number) => machines.client.panes.get(pane)?.view.text() ?? "",
      },
    });
    // The visible height excludes a phone's on-screen keyboard, so the
    // key bar sits just above it.
    const vv = window.visualViewport;
    const fit = () => document.documentElement.style.setProperty("--app-height", `${vv?.height ?? innerHeight}px`);
    vv?.addEventListener("resize", fit);
    fit();
    up = { machines, cell: await measureCell() };
    return up;
  })());
}

/** `PlatformApi.panes`' reads (a project's Sessions, Arugula inversion
 * L3): the machines booted as for their own places, and every machine's
 * panes from the fleet's summaries. */
export function watchPanes(session: AppSession | null, fn: () => void): () => void {
  let off: (() => void) | null = null;
  let live = true;
  void boot(session).then(({ machines }) => {
    if (!live) return;
    const offs = [machines.fleet.subscribe(fn), directory.subscribe(fn), session?.subscribe(fn) ?? (() => {})];
    off = () => offs.forEach((o) => o());
    fn();
  });
  return () => {
    live = false;
    off?.();
  };
}

/** A machine by control's id. On a daemon's own page (no session) its
 * hosts carry no ids: the page's own daemon stands in, as `openPane` does. */
function hostById(id: string, session: AppSession | null): FleetHost | undefined {
  const fleet = up?.machines.fleet;
  return fleet?.hostById(id) ?? (session ? undefined : fleet?.list.find((h) => h.name === directory.home));
}

/** Whose a machine is: its owner's name, null for the reader's own. */
export function ownerOf(id: string, session: AppSession | null): string | null | undefined {
  const h = hostById(id, session);
  return h ? (h.owner ?? null) : undefined;
}

/** What a pane says it's doing (a swarm card's line), and its branch. */
export function paneOf(id: string, pane: number, session: AppSession | null): { line: string; branch: string | null } | undefined {
  const h = hostById(id, session);
  const p = h && up?.machines.fleet.panes.find((x) => x.host === h.name && x.id === pane);
  return p ? { line: whatItDoes(p), branch: p.info.branch ?? null } : undefined;
}

/** Once the session has first said what it has. */
function answered(s: AppSession): Promise<void> {
  return new Promise((done) => {
    if (s.phase !== "loading") return done();
    const off = s.subscribe(() => {
      if (s.phase === "loading") return;
      off();
      done();
    });
  });
}

const hostOf = (id: string) => directory.list?.hosts.find((h) => h.id === id)?.name;

export function MachinesPlace({ project, at, session }: { project: string; at: MachineAt; session: AppSession | null }) {
  const [b, setB] = useState<Booted | null>(null);
  useEffect(() => {
    let live = true;
    void boot(session).then((x) => live && setB(x));
    return () => void (live = false);
  }, []);
  if (!b) return <p class="platform-section dim" data-machines-loading>Reaching your machines…</p>;
  return <Place project={project} at={at} b={b} session={session} />;
}

function Place({ project, at, b: { machines, cell }, session }: { project: string; at: MachineAt; b: Booted; session: AppSession | null }) {
  useSubscribe((fn) => directory.subscribe(fn));
  const list = at.at === "machines" && at.pane === undefined && !at.name;
  // The shell's keys while a machine's page or Swarm is shown, and only then.
  useEffect(() => (list ? undefined : installKeys(() => machines.client)), [list]);
  const [, tick] = useState(0);
  useEffect(() => machines.onSwap(() => tick((n) => n + 1)), []);
  // The machine and pane the address names, once the machines are known.
  const known = !!directory.list;
  const machine = at.at === "machine" ? at.machine : undefined;
  const name = at.at === "machines" ? at.name : undefined;
  const pane = at.at === "swarm" ? undefined : at.pane;
  useEffect(() => {
    if (at.at === "swarm" || list || !known) return;
    if (name && directory.find(name)) directory.select(name);
    if (pane !== undefined) machines.openPane(pane, machine, at.at === "machine" && !!at.share, at.at === "machine" ? at.follow : undefined);
    else if (machine && hostOf(machine)) directory.select(hostOf(machine)!);
    if (at.at === "machine" && at.agent)
      machines.untilShown((c) => {
        if (!c.connected) return false;
        startAgent(c, {});
        return true;
      });
  }, [project, machine, name, pane, known, list]);

  if (at.at === "swarm") {
    const f = at.focus;
    const host = f ? (f.machine ? hostOf(f.machine) : (directory.home ?? directory.names[0])) : undefined;
    const back = () => go(pathOf({ page: "machines", project, at: { at: "machines" } }, names));
    return (
      <div class="app-swarm" data-app-swarm>
        <BesideRail />
        <SwarmView fleet={machines.fleet} back={back} focus={f && host ? { host, pane: f.pane } : null} board={at.board ?? null} />
      </div>
    );
  }
  if (list)
    return (
      // In the panel the project's other pages sit in.
      <main class="platform-main">
        <MachinesList project={project} fleet={machines.fleet} session={session} />
      </main>
    );
  const client = machines.client;
  const pop = at.at === "machine" && at.pop && at.pane !== undefined ? { daemon: at.machine, pane: at.pane } : null;
  return (
    // What a pane's thread and blocks reach the machine through (M147).
    <PlatformApiContext.Provider value={shellPlatformApi(client)}>
      <MachineView client={client} cell={cell} pop={pop} frame="app" onSwarm={() => go(pathOf({ page: "machines", project, at: { at: "swarm" } }, names))} />
      {!pop && <KeepPane key={client.base} project={project} client={client} asked={at.pane} />}
      {/* The machine page's own Swarm (`#swarm`), as on control's page. */}
      <Swarm machines={machines} client={client} ready={() => true} />
      <AgentDialogLayer />
    </PlatformApiContext.Provider>
  );
}

/** A pane live, at its own size, for a project's Sessions (Open): its
 * machine is the one shown, as on its own page, and the pane moves into
 * this slot until it closes. */
export function LiveHere({ session, machine, pane }: { session: AppSession | null; machine: string; pane: number }) {
  const [b, setB] = useState<Booted | null>(up);
  useEffect(() => {
    let live = true;
    void boot(session).then((x) => live && setB(x));
    return () => void (live = false);
  }, []);
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => b?.machines.fleet.subscribe(fn) ?? (() => {}));
  const [, tick] = useState(0);
  useEffect(() => b?.machines.onSwap(() => tick((n) => n + 1)), [b]);
  const host = b ? hostById(machine, session) : undefined;
  useEffect(() => {
    if (host && directory.current !== host.name) directory.select(host.name);
  }, [host?.name]);
  if (!b) return <p class="dim" data-live-loading>Reaching your machines…</p>;
  if (!host) return <p class="dim" data-live-missing>That machine isn't one this page reaches.</p>;
  return <ClientLive key={b.machines.client.base} client={b.machines.client} pane={pane} />;
}

function ClientLive({ client, pane }: { client: Client; pane: number }) {
  useSubscribe((fn) => client.subscribe(fn));
  if (!client.connected || !client.state) return <p class="dim" data-live-loading>Reaching the machine…</p>;
  if (!client.info(pane)) return <p class="dim" data-live-missing>That pane has closed.</p>;
  return (
    <PlatformApiContext.Provider value={shellPlatformApi(client)}>
      <LiveSlot client={client} pane={pane} />
    </PlatformApiContext.Provider>
  );
}

/** The pane the machine's page shows is the address, and your pane in this
 * project (Arugula #989, decision 5): a reload, or Machines on the rail,
 * comes back to it. Not before the pane the address `asked` for has shown
 * (or is gone), so the one the page restored can't take its place. */
function KeepPane({ project, client, asked }: { project: string; client: Client; asked: number | undefined }) {
  useSubscribe((fn) => client.subscribe(fn));
  const settled = useRef(asked === undefined);
  const pane = client.connected && client.state ? client.active() : null;
  if (!settled.current && pane !== null && (pane === asked || !client.info(asked!))) settled.current = true;
  const machine = client.e2e?.daemon.id ?? directory.find(directory.current)?.id;
  useEffect(() => {
    if (pane === null || !settled.current) return;
    const at: MachineAt = machine ? { at: "machine", machine, pane } : { at: "machines", pane };
    const path = pathOf({ page: "machines", project, at }, names);
    notePane(project, path);
    if (current().page === "machines" && location.pathname + location.search !== path) go(path, true);
  }, [pane, machine, settled.current]);
  return null;
}

/** The swarm fills the window, beside the rail where there is one, as on
 * control's page (style.css's `data-workspace`). */
function BesideRail() {
  const phone = usePhone();
  useLayoutEffect(() => {
    if (phone) return;
    document.documentElement.dataset.workspace = "new";
    return () => void delete document.documentElement.dataset.workspace;
  }, [phone]);
  return null;
}

/** The team's machines: each one's state, whose it is and what you may do
 * there, its sessions and the projects they serve, and its page one click
 * on. Cloud workspaces are among them, marked. */
function MachinesList({ project, fleet, session }: { project: string; fleet: Fleet; session: AppSession | null }) {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => fleet.subscribe(fn));
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  const phone = usePhone();
  const rows = sortRows(rowsOf(fleet.list, fleet.panes, (id) => session?.daemons.find((d) => d.id === id)?.hosted?.role));
  return (
    <section class="platform-section sw-page" data-machines-list>
      <PageHeader title="Machines" />
      {directory.list === null ? (
        <p class="dim">Loading…</p>
      ) : rows.length ? (
        <ul class={`platform-list ${phone ? "card-list" : "card-grid"}`}>
          {rows.map((r) => {
            const at: MachineAt = r.id ? { at: "machine", machine: r.id } : { at: "machines", name: r.name };
            const to = `/p/${project}${machinesPath(at)}`;
            const line = sessionsLine(r);
            return (
              <li key={r.name}>
                <Card as="a" tone={r.needsYou ? "accent" : r.online ? "ok" : "muted"} href={to} data-machine-link={r.id ?? r.name} onClick={(e) => follow(e, to)}>
                  <span class="card-head">
                    <b class="card-title">{r.name}</b>
                    <StateChip tone={r.online ? "ok" : "muted"} data-machine-state>
                      {r.online ? "online" : "offline"}
                    </StateChip>
                    {r.cloud && (
                      <Chip kind="tag" data-machine-cloud>
                        Cloud
                      </Chip>
                    )}
                  </span>
                  <p class="card-meta">{[r.owner ? `${r.owner}'s` : "Yours", ROLE_WORDS[r.role]].join(" · ")}</p>
                  {line && (
                    <p class="card-meta" data-machine-sessions>
                      {line}
                    </p>
                  )}
                  {!phone && r.projects.length > 0 && (
                    <span class="card-head">
                      {r.projects.map((p) => (
                        <Chip key={p} kind="tag">
                          {p}
                        </Chip>
                      ))}
                    </span>
                  )}
                </Card>
              </li>
            );
          })}
        </ul>
      ) : (
        <EmptyState where="page" title="No machines yet." why="A machine shows here once it has joined your team. Its sessions and who may steer them come with it." data-machines-empty />
      )}
    </section>
  );
}
