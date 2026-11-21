// Settings › Machines in the app (Arugula #1107):
// `PlatformApi.MachineSettings`, over the app's own clients to its
// machines. The sections are in a chunk of their own (./machine-settings),
// loaded when Machines is first shown.

import type { ComponentChildren, ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { AppSession } from "./session";

type Chunk = typeof import("./machine-settings");

let loading: Promise<Chunk> | null = null;
const load = () => (loading ??= import("./machine-settings"));

/** Machines' sections over `session`: the hub's machines, reached
 * with machine tokens. `session` null is a daemon's own page, which
 * reaches a machine at the origin `own` gives for its id. */
export function appMachineSettings(
  session: () => AppSession | null,
  own?: (machine: string) => string | undefined,
): ComponentType<{ machine: string; name: string; grants?: ComponentChildren; children?: ComponentChildren }> {
  return function MachineSettings({ machine, name, grants, children }) {
    const [chunk, setChunk] = useState<Chunk | null>(null);
    useEffect(() => {
      let live = true;
      void load().then((c) => live && setChunk(() => c));
      return () => void (live = false);
    }, []);
    if (!chunk) return <>{children}</>;
    return (
      <chunk.MachineSettingsPlace machine={machine} name={name} reach={{ session: session(), own }} grants={grants}>
        {children}
      </chunk.MachineSettingsPlace>
    );
  };
}
