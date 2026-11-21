// Settings › Machines in the app (Arugula #1107): a machine's own
// settings (shell/ui/machine-settings), read through the app's own client
// to it (./machine-client), the hub's machine token on app.arugula.io.
// Each section's summary says when the app can't reach the machine: the
// hub doesn't let it from this host, it's offline, or it doesn't answer.
// A chunk of its own (./settings), so Settings doesn't load the daemon's
// client until Machines is shown.

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { usePlatformApi } from "../shared/platform-api";
import { useSubscribe } from "../shared/ui/hooks";
import { ControlRow } from "../platform/settings/owner-machines";
import { MachineSections, type Away } from "../shell/ui/machine-settings";
import { useLive, type Reach } from "./machine-client";

/** How long a machine the hub lists may take to answer before the sections say it doesn't. */
const PATIENCE_MS = 15_000;

export function MachineSettingsPlace({
  machine,
  name,
  reach,
  grants,
  children,
}: {
  machine: string;
  name: string;
  reach: Reach;
  grants?: ComponentChildren;
  children?: ComponentChildren;
}) {
  const s = reach.session;
  useSubscribe((fn) => s?.subscribe(fn) ?? (() => {}));
  const live = useLive(machine, reach);
  const client = live === "unreachable" ? null : live;
  const connected = !!client?.connected;
  // Waited long enough for it.
  const [late, setLate] = useState(false);
  useEffect(() => {
    setLate(false);
    if (connected) return;
    const t = setTimeout(() => setLate(true), PATIENCE_MS);
    return () => clearTimeout(t);
  }, [machine, connected]);
  const api = usePlatformApi();
  const page = api.controlHref?.(`machine=${machine}`);
  // The machine's own page, on control's, where its settings are while the app can't reach it.
  const out = page ? (
    <ul class="platform-list settings-rows">
      <ControlRow href={page} label={`Open ${name}'s page`} why="Its settings are under This machine in its menu." hook="machine" />
    </ul>
  ) : undefined;
  const offline = s?.daemons.find((d) => d.id === machine)?.online === false;
  const away: Away | undefined =
    live === "unreachable"
      ? { summary: "Can't reach it from here", line: `This page can't reach ${name} yet, so its settings are on its own page.`, out }
      : offline && !connected
        ? { summary: "Offline", line: `${name} is offline. Its settings show here once it's back.` }
        : late && !connected
          ? { summary: "Not answering", line: `${name} isn't answering. Its daemon may be older than this page: update it.`, out }
          : undefined;
  return (
    <MachineSections client={client} away={away} grants={grants}>
      {children}
    </MachineSections>
  );
}
