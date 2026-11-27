// The machines in the app (Arugula #989): `PlatformApi.Machines`. The
// machine page and Swarm are the shell's, in a chunk of their own
// (./machine-page), so the project's other places don't load terminals.
// While the hub can't reach machines from this host, each place opens on
// control's page, as the links did before.

import type { ComponentChildren, ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { AgentPanelProps } from "../shared/kit/panel";
import type { MachineAt } from "../shared/machine-paths";
import type { OverviewZoom, PlatformApi } from "../shared/platform-api";
import { useSubscribe } from "../shared/ui/hooks";
import { OnControl } from "../platform/pages/machines";
import type { Fleet } from "../shell/fleet";
import type { AppSession } from "./session";

type Place = ComponentType<{ project: string; at: MachineAt }>;
type Page = typeof import("./machine-page");
type OverviewPage = typeof import("./overview");
type PanelPage = typeof import("./panel");

/** The machine page's chunk, once asked for. */
let loading: Promise<Page> | null = null;
const load = () => (loading ??= import("./machine-page"));
let loadingOverview: Promise<OverviewPage> | null = null;
const loadOverview = () => (loadingOverview ??= import("./overview"));
let loadingInbox: Promise<typeof import("./inbox")> | null = null;
const loadInbox = () => (loadingInbox ??= import("./inbox"));
let loadingPanel: Promise<PanelPage> | null = null;
const loadPanel = () => (loadingPanel ??= import("./panel"));

/** `PlatformApi.Machines` over `session`: the hub's machines, reached with
 * machine tokens. `session` null is a daemon's own page (its machines
 * directly), which the end-to-end specs draw the app's frame on. */
export function appMachines(session: () => AppSession | null): Place {
  return function Machines({ project, at }) {
    const s = session();
    useSubscribe((fn) => s?.subscribe(fn) ?? (() => {}));
    const [page, setPage] = useState<Page | null>(null);
    const reachable = !s || s.reachable !== false;
    useEffect(() => {
      if (!reachable) return;
      let live = true;
      void load().then((p) => live && setPage(() => p));
      return () => void (live = false);
    }, [reachable]);
    if (!reachable) return <OnControl at={at} />;
    if (!page) return <p class="platform-section dim" data-machines-loading>Reaching your machines…</p>;
    return <page.MachinesPlace project={project} at={at} session={s} />;
  };
}

/** `PlatformApi.machineDocs` over `session` (Arugula #971): the machine
 * page's chunk, loaded and its machines booted once a view watches, and a
 * machine's Docs moved by asking it. While the hub can't reach machines
 * from this host, none is here. */
export function appMachineDocs(session: () => AppSession | null): NonNullable<PlatformApi["machineDocs"]> {
  let page: Page | null = null;
  return {
    watch(fn) {
      const s = session();
      if (s && s.reachable === false) return () => {};
      let off: (() => void) | null = null;
      let live = true;
      void load().then((p) => {
        page = p;
        if (live) off = p.watchMachines(s, fn);
      });
      return () => {
        live = false;
        off?.();
      };
    },
    here: (machine) => !!page?.docsHere(machine, session()),
    move: async (machine, doc, project) => (await load()).moveDoc(machine, doc, project, session()),
  };
}

/** `PlatformApi.docLinks` over `session` (Arugula #1315): the Docs the
 * reader shared by link, through the machine that shared each, with the
 * machine page's chunk loaded and its machines booted once a view watches.
 * While the hub can't reach machines from this host, none is here. */
export function appDocLinks(session: () => AppSession | null): NonNullable<PlatformApi["docLinks"]> {
  const docs = appMachineDocs(session);
  let page: Page | null = null;
  const loaded = async () => (page = await load());
  return {
    watch(fn) {
      void loaded();
      return docs.watch(fn);
    },
    here: docs.here,
    name: (machine) => page?.machineName(machine),
    titles: async (machine) => (await loaded()).linkTitles(machine, session()),
    url: async (machine, link) => (await loaded()).linkUrl(machine, link, session()),
    open: async (machine, link) => (await loaded()).openLink(machine, link, session()),
    unshare: async (machine, link) => (await loaded()).unshareLink(machine, link, session()),
  };
}

/** `PlatformApi.ownPane` over `session` (Arugula
 * iss_89b78ece43ac3d6271f78d797cdbdc61): whether a board's lead is a pane
 * of the reader's own, with the machine page's chunk loaded and its
 * machines booted once a view watches. While the hub can't reach machines
 * from this host, none is. */
export function appOwnPane(session: () => AppSession | null): NonNullable<PlatformApi["ownPane"]> {
  const docs = appMachineDocs(session);
  let page: Page | null = null;
  return {
    watch(fn) {
      void load().then((p) => (page = p));
      return docs.watch(fn);
    },
    here: (machine, pane) => !!page?.paneHere(machine, pane, session()),
  };
}

/** `PlatformApi.Inbox` over `session` (Lift 6d, #1486): the Inbox, in a
 * chunk of its own, drawn at once; a pane's ask answers in place once the
 * machines are reached, and opens its pane while they aren't (or can't be:
 * the hub doesn't reach them from this host). */
export function appInbox(session: () => AppSession | null): ComponentType {
  return function Inbox() {
    const s = session();
    useSubscribe((fn) => s?.subscribe(fn) ?? (() => {}));
    const [page, setPage] = useState<typeof import("./inbox") | null>(null);
    const [fleet, setFleet] = useState<Fleet | null>(null);
    const reachable = !s || s.reachable !== false;
    useEffect(() => {
      let live = true;
      void loadInbox().then((p) => live && setPage(() => p));
      return () => void (live = false);
    }, []);
    useEffect(() => {
      if (!reachable) return;
      let live = true;
      void load()
        .then((p) => p.boot(s))
        .then((b) => live && setFleet(b.machines.fleet));
      return () => void (live = false);
    }, [reachable]);
    if (!page) return <p class="dim">Loading…</p>;
    return <page.InboxPlace fleet={reachable ? fleet : null} />;
  };
}

/** `PlatformApi.Overview` over `session` (Arugula inversion, L6): the
 * project's Overview, in a chunk of its own with the machine page's. While
 * the hub can't reach machines from this host there is no Overview: the
 * project's page as it was (its children) is, and only then (#1088). */
export function appOverview(session: () => AppSession | null): ComponentType<{ project: string; board?: string; card?: string; zoom?: OverviewZoom; children?: ComponentChildren }> {
  return function Overview({ project, board, card, zoom, children }) {
    const s = session();
    useSubscribe((fn) => s?.subscribe(fn) ?? (() => {}));
    const [page, setPage] = useState<OverviewPage | null>(null);
    const reachable = !s || s.reachable !== false;
    useEffect(() => {
      if (!reachable) return;
      let live = true;
      void loadOverview().then((p) => live && setPage(() => p));
      return () => void (live = false);
    }, [reachable]);
    if (!reachable) return <>{children}</>;
    if (!page)
      return (
        <p class="dim" data-overview-loading>
          Reaching your machines…
        </p>
      );
    return <page.OverviewPlace project={project} board={board} card={card} zoom={zoom} session={s} />;
  };
}

/** `PlatformApi.AgentPanel` over `session` (Lift 4, #1480): the agent panel
 * (./panel), in the chunk the Overview loads too. While the hub can't reach
 * machines from this host, only what goes over it (`above`) shows. */
export function appAgentPanel(session: () => AppSession | null): ComponentType<AgentPanelProps> {
  return function AgentPanel(props) {
    const s = session();
    useSubscribe((fn) => s?.subscribe(fn) ?? (() => {}));
    const [page, setPage] = useState<PanelPage | null>(null);
    const reachable = !s || s.reachable !== false;
    useEffect(() => {
      if (!reachable) return;
      let live = true;
      void loadPanel().then((p) => live && setPage(() => p));
      return () => void (live = false);
    }, [reachable]);
    if (!reachable || !page) return <>{props.above}</>;
    return <page.AgentPanelPlace {...props} session={s} />;
  };
}
