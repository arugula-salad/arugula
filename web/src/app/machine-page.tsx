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
import { SwarmView } from "../shell/swarm/view";
import { AgentDialogLayer, startAgent } from "../shell/ui/agent-dialog";
import { measureCell, type Cell } from "../shell/ui/cells";
import { installKeys } from "../shell/ui/keys";
import { MachineView, Swarm } from "../shell/views";
import { names } from "../platform/links";
import { session as platform } from "../platform/session";
import { current, follow, go, pathOf } from "../platform/router";
import type { Fleet, FleetHost } from "../shell/fleet";
import { ROLE_WORDS, rowsOf, sessionsLine, sortRows } from "./machine-list";
import type { AppSession } from "./session";
import type { Client } from "../shell/client";
import { OPS, opPath } from "../shell/ops";
import type { DocLinkUrl, DocMoveRequest, DocOpened, DocOpenRequest, DocRow } from "../shared/proto.gen";
import { notePane } from "../platform/places-kept";
import { popOutIn } from "../shell/ui/panes/pop-out";
import { PanesList } from "./panes-list";
import { newSessionsIn } from "../shell/new-session";
import { projectRef } from "../shell/ui/session-project";
import { inShown, onShownProject, projectOf, showProject } from "../shell/project-view";

export interface Booted {
  machines: Machines;
  cell: Cell;
}

let booted: Promise<Booted> | null = null;
/** The machines, once booted: what `panes` reads. */
let up: Booted | null = null;

/** The page's machines, made once. */
export function boot(session: AppSession | null): Promise<Booted> {
  return (booted ??= (async () => {
    // The route names the machine shown, not `?machine=`.
    directory.keepsAddress = false;
    popOutIn.app = true;
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

/** The machines booted as for their own places, calling `fn` as they and
 * their panes change (`PlatformApi.machineDocs`' watch). */
export function watchMachines(session: AppSession | null, fn: () => void): () => void {
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

/** Whether the reader's own machine `id` answers now, so it can be asked
 * to move its Docs (`PlatformApi.machineDocs`). */
export function docsHere(id: string, session: AppSession | null): boolean {
  const h = hostById(id, session);
  return !!h && !h.owner && h.state === "connected";
}

/** Ask machine `id` to give its Doc `doc` to `project` (its `doc move`):
 * null once it has, else why not. */
export async function moveDoc(id: string, doc: string, project: string, session: AppSession | null): Promise<string | null> {
  const h = hostById(id, session);
  if (!h || !up) return "that machine isn't reached from here";
  try {
    const res = await up.machines.fleet.request(h.name, OPS["doc.move"].method, opPath("doc.move", { id: doc }), { project } satisfies DocMoveRequest);
    if (res.ok) return null;
    return (await res.json<{ error?: string }>().catch(() => null))?.error ?? `HTTP ${res.status}`;
  } catch (e) {
    return (e as Error).message;
  }
}

/** Ask the reader's own machine `id`, which must answer now, for `op`:
 * its answer, or its `{error}` thrown. */
async function ask<T>(id: string, session: AppSession | null, op: keyof typeof OPS, path: string, body?: unknown): Promise<T> {
  const h = hostById(id, session);
  if (!h || !up || h.owner || h.state !== "connected") throw new Error("its machine is offline");
  const res = await up.machines.fleet.request(h.name, OPS[op].method, path, body);
  const v = await res.json<T & { error?: string }>().catch(() => null);
  if (!res.ok || !v) throw new Error(v?.error ?? `${h.name} said ${res.status}`);
  return v;
}

/** What machine `id`'s Docs shared by link are titled, by link id
 * (`PlatformApi.docLinks`, Arugula #1315). */
export async function linkTitles(id: string, session: AppSession | null): Promise<Map<string, string>> {
  const docs = await ask<DocRow[]>(id, session, "docs.list", opPath("docs.list"));
  return new Map((Array.isArray(docs) ? docs : []).flatMap((d) => (d.shared?.link ? [[d.shared.link, d.title] as const] : [])));
}

/** A link's URL, key and all, from the machine that shared it. */
export async function linkUrl(id: string, link: string, session: AppSession | null): Promise<string> {
  const v = await ask<Partial<DocLinkUrl>>(id, session, "doc_link.get", opPath("doc_link.get", { id: link }));
  if (!v.url) throw new Error("it has no link");
  return v.url;
}

/** The pane machine `id` opens a link's Doc in. */
export async function openLink(id: string, link: string, session: AppSession | null): Promise<number> {
  const v = await ask<Partial<DocOpened>>(id, session, "doc.open", opPath("doc.open"), { link } satisfies DocOpenRequest);
  if (typeof v.pane !== "number") throw new Error("it opened none");
  return v.pane;
}

/** Whether machine `id` took a link down itself (clearing its Doc's block). */
export async function unshareLink(id: string, link: string, session: AppSession | null): Promise<boolean> {
  const h = hostById(id, session);
  if (!h || !up || h.owner || h.state !== "connected") return false;
  const res = await up.machines.fleet.request(h.name, OPS["doc_link.delete"].method, opPath("doc_link.delete", { id: link }));
  return res.ok;
}

/** A machine's name, by control's id. */
export const machineName = (id: string) => hostOf(id);

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
  useSubscribe((fn) => platform.subscribe(fn));
  // Everything here shows this project's sessions only (Arugula #1164), and a session made here starts in it (Arugula #1163).
  const p = platform.project(project);
  const shown = { id: project, name: p?.name ?? "this project", personal: platform.personal()?.id === project };
  useEffect(() => {
    // With its team, so the team watches a new session at once (Arugula #1162).
    newSessionsIn(p ? projectRef(p) : { id: project, name: "" });
    return () => newSessionsIn(null);
  }, [project, p?.name, p?.owner?.kind === "team" ? p.owner.id : null]);
  useLayoutEffect(() => {
    showProject({ ...shown, machines: () => go(pathOf({ page: "machines", project, at: { at: "machines" } }, names)) });
    machines.client.reselect();
  }, [project, shown.name, shown.personal]);
  useEffect(() => () => showProject(null), []);
  // A list in the platform's panel: the machines, or Build's home, every machine's panes (Arugula #1083).
  const list = (at.at === "machines" && at.pane === undefined && !at.name) || at.at === "panes";
  // The shell's keys while a machine's page or Swarm is shown, and only then.
  useEffect(() => (list ? undefined : installKeys(() => machines.client)), [list]);
  const [, tick] = useState(0);
  useEffect(() => machines.onSwap(() => tick((n) => n + 1)), []);
  // The machine and pane the address names, once the machines are known.
  const known = !!directory.list;
  const machine = at.at === "machine" ? at.machine : undefined;
  const name = at.at === "machines" ? at.name : undefined;
  const pane = at.at === "swarm" || at.at === "panes" ? undefined : at.pane;
  useEffect(() => {
    if (at.at === "swarm" || at.at === "panes" || list || !known) return;
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
    const back = () => go(pathOf({ page: "machines", project, at: { at: "panes" } }, names));
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
        {at.at === "panes" ? <PanesList project={project} fleet={machines.fleet} machine={at.machine} agents={!!at.agents} /> : <MachinesList project={project} fleet={machines.fleet} session={session} />}
      </main>
    );
  const client = machines.client;
  // The home machine's has no daemon: the pop-out page takes it from the directory.
  const pop = at.pop && at.pane !== undefined ? { daemon: at.at === "machine" ? at.machine : undefined, pane: at.pane } : null;
  return (
    // What a pane's thread and blocks reach the machine through (M147).
    <PlatformApiContext.Provider value={shellPlatformApi(client)}>
      <MachineView client={client} cell={cell} pop={pop} frame="app" onSwarm={() => go(pathOf({ page: "machines", project, at: { at: "swarm" } }, names))} />
      {!pop && <KeepPane key={client.base} project={project} client={client} asked={at.pane} />}
      {at.pane !== undefined && <PaneProject key={client.base} project={project} client={client} at={at} />}
      {/* The machine page's own Swarm (`#swarm`), as on control's page. */}
      <Swarm machines={machines} client={client} ready={() => true} />
      <AgentDialogLayer />
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
  // Only a pane of this project's: one of another's is about to be left.
  const sid = pane === null || pane === undefined ? null : client.sessionOfPane(pane);
  const ours = client.shownSessions().some((s) => s.id === sid);
  useEffect(() => {
    if (pane === null || !settled.current || !ours) return;
    const at: MachineAt = machine ? { at: "machine", machine, pane } : { at: "machines", pane };
    const path = pathOf({ page: "machines", project, at }, names);
    notePane(project, path);
    if (current().page === "machines" && location.pathname + location.search !== path) go(path, true);
  }, [pane, machine, settled.current, ours]);
  return null;
}

/** A link to a pane in another of your projects (a notification, Needs
 * you, a pinned pane: #1161, decision 3) switches to that project first,
 * so the tile always matches what's shown. */
function PaneProject({ project, client, at }: { project: string; client: Client; at: MachineAt }) {
  useSubscribe((fn) => client.subscribe(fn));
  const pane = at.at === "swarm" || at.at === "panes" ? undefined : at.pane;
  const sid = pane === undefined || !client.state ? null : client.sessionOfPane(pane);
  const s = sid === null ? undefined : client.state?.sessions.find((x) => x.id === sid);
  const yours = !!client.state && !client.state.roles;
  const its = s && !inShown(s.project, yours) ? projectOf(s.project, yours, platform.personal()?.id) : null;
  const to = its && its !== project && platform.project(its) ? pathOf({ page: "machines", project: its, at }, names) : null;
  useEffect(() => {
    if (to) go(to, true);
  }, [to]);
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
 * on. Cloud workspaces are among them, marked; the project's Runs and
 * Workspaces are its header's (Arugula #1082). */
function MachinesList({ project, fleet, session }: { project: string; fleet: Fleet; session: AppSession | null }) {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => fleet.subscribe(fn));
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  useSubscribe((fn) => platform.subscribe(fn));
  const phone = usePhone();
  useSubscribe(onShownProject);
  const hosted = (id: string | undefined) => (id ? session?.daemons.find((d) => d.id === id)?.hosted : undefined);
  const name = platform.project(project)?.name ?? "this project";
  // This project's (Arugula #1164): your machines, its cloud workspaces,
  // and any other machine with a session of it you may see.
  const rows = sortRows(rowsOf(fleet.list, fleet.panes, (id) => hosted(id)?.role, (p, yours) => inShown(p, yours))).filter((r) =>
    r.cloud ? hosted(r.id)?.project === project || !!r.sessions : !r.owner || !!r.sessions,
  );
  // The project's runs and cloud workspaces are here, under Build (Arugula #1082): a run is a session in a cloud workspace.
  const runs = pathOf({ page: "runs", project }, names);
  const workspaces = pathOf({ page: "workspaces", project }, names);
  return (
    <section class="platform-section sw-page" data-machines-list>
      <PageHeader
        title="Machines"
        actions={
          <>
            <a class="secondary" href={runs} data-machines-runs onClick={(e) => follow(e, runs)}>
              Runs
            </a>
            {platform.hosted && (
              <a class="secondary" href={workspaces} data-machines-workspaces onClick={(e) => follow(e, workspaces)}>
                Workspaces
              </a>
            )}
          </>
        }
      />
      {directory.list === null ? (
        <p class="dim">Loading…</p>
      ) : rows.length ? (
        <ul class={`platform-list ${phone ? "card-list" : "card-grid"}`}>
          {rows.map((r) => {
            const at: MachineAt = r.id ? { at: "machine", machine: r.id } : { at: "machines", name: r.name };
            const to = `/p/${project}${machinesPath(at)}`;
            const line = sessionsLine(r, name);
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
