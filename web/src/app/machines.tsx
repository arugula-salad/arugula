// The machines in the app (Arugula #989): `PlatformApi.Machines`. The
// machine page and Swarm are the shell's, in a chunk of their own
// (./machine-page), so the project's other places don't load terminals.
// While the hub can't reach machines from this host, each place opens on
// control's page, as the links did before.

import type { ComponentChildren, ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { MachineAt } from "../shared/machine-paths";
import { useSubscribe } from "../shared/ui/hooks";
import { OnControl } from "../platform/pages/machines";
import type { AppSession } from "./session";

type Place = ComponentType<{ project: string; at: MachineAt }>;
type Page = typeof import("./machine-page");
type OverviewPage = typeof import("./overview");

/** The machine page's chunk, once asked for. */
let loading: Promise<Page> | null = null;
const load = () => (loading ??= import("./machine-page"));
let loadingOverview: Promise<OverviewPage> | null = null;
const loadOverview = () => (loadingOverview ??= import("./overview"));

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

/** `PlatformApi.Overview` over `session` (Arugula inversion, L6): the
 * project's Overview, in a chunk of its own with the machine page's. While
 * the hub can't reach machines from this host, the project's page as it
 * was (its children). */
export function appOverview(session: () => AppSession | null): ComponentType<{ project: string; board?: string; card?: string; children?: ComponentChildren }> {
  return function Overview({ project, board, card, children }) {
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
        <>
          <p class="dim" data-overview-loading>
            Reaching your machines…
          </p>
          {children}
        </>
      );
    return (
      <page.OverviewPlace project={project} board={board} card={card} session={s}>
        {children}
      </page.OverviewPlace>
    );
  };
}
