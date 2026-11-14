// A board card's live session in the app (Arugula
// iss_c62e00eee3b4b953ff054d2951827de8): `PlatformApi.LivePane` and
// `LiveAsk`, over the app's own clients to its machines. The terminals are
// in a chunk of their own (./live-pane), loaded when a drawer first shows
// a session's pane or question.

import type { ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { Reach } from "./live-pane";
import type { AppSession } from "./session";

type Chunk = typeof import("./live-pane");
type Live = ComponentType<{ machine: string; pane: string }>;

let loading: Promise<Chunk> | null = null;
const load = () => (loading ??= import("./live-pane"));

/** The live pane and question over `session`: the hub's machines, reached
 * with machine tokens. `session` null is a daemon's own page, which
 * reaches a machine at the origin `own` gives for its id. */
export function appLive(session: () => AppSession | null, own?: (machine: string) => string | undefined): { LivePane: Live; LiveAsk: Live } {
  const lazy = (pick: (c: Chunk) => ComponentType<{ machine: string; pane: string; reach: Reach }>): Live =>
    function Live({ machine, pane }) {
      const [chunk, setChunk] = useState<Chunk | null>(null);
      useEffect(() => {
        let live = true;
        void load().then((c) => live && setChunk(() => c));
        return () => void (live = false);
      }, []);
      if (!chunk) return null;
      const C = pick(chunk);
      return <C machine={machine} pane={pane} reach={{ session: session(), own }} />;
    };
  return { LivePane: lazy((c) => c.LivePane), LiveAsk: lazy((c) => c.LiveAsk) };
}
