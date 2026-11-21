// Settings › Machines in the app (Arugula #1107): a machine's own
// settings (shell/ui/machine-settings), read through the app's own client
// to it (./machine-client), the hub's machine token on app.arugula.io.
// When the app can't reach the machine, its row and one note over each
// section say why (the hub doesn't let it from this host, it's offline, or
// it doesn't answer) and what to do, with its parts dimmed: there's no
// sending people to the machine's page for them (Arugula #1314).
// A chunk of its own (./settings), so Settings doesn't load the daemon's
// client until Machines is shown.

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { useSubscribe } from "../shared/ui/hooks";
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
  const offline = s?.daemons.find((d) => d.id === machine)?.online === false;
  const away: Away | undefined =
    live === "unreachable"
      ? {
          summary: "Can't reach it from here",
          line: `Can't reach ${name} from here: Arugula cloud doesn't give this page a way to it. These show here once it does; until then they're on the machine itself.`,
        }
      : offline && !connected
        ? { summary: "Offline", line: `${name} is offline. These show here once it's back online.` }
        : late && !connected
          ? {
              summary: "Not answering",
              line: (
                <>
                  {name} isn&apos;t answering. Its daemon may be older than this page: on the machine, run <code>arugulad update</code>. These
                  show here once it answers.
                </>
              ),
            }
          : undefined;
  return (
    <MachineSections client={client} away={away} grants={grants}>
      {children}
    </MachineSections>
  );
}
