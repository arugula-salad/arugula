// A board card's live session in the app (Arugula
// iss_c62e00eee3b4b953ff054d2951827de8): `PlatformApi.LivePane`,
// `LiveAsk` and `CardWhy` (Arugula #1019), and `LiveWork`, the Overview's
// Work level's panes (Arugula #1289), over the app's own clients to its machines. The terminals are
// in a chunk of their own (./live-pane), loaded when a drawer first shows
// a session's pane or question.

import type { ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { Reach } from "./machine-client";
import type { AppSession } from "./session";

type Chunk = typeof import("./live-pane");
type Live<P = {}> = ComponentType<{ machine: string; pane: string } & P>;

let loading: Promise<Chunk> | null = null;
const load = () => (loading ??= import("./live-pane"));

/** The live pane and question over `session`: the hub's machines, reached
 * with machine tokens. `session` null is a daemon's own page, which
 * reaches a machine at the origin `own` gives for its id. */
export function appLive(session: () => AppSession | null, own?: (machine: string) => string | undefined): { LivePane: Live; LiveAsk: Live; CardWhy: Live; LiveWork: Live<{ watch?: boolean }> } {
  const lazy = <P,>(pick: (c: Chunk) => ComponentType<{ machine: string; pane: string; reach: Reach } & P>): Live<P> =>
    function Live(props) {
      const [chunk, setChunk] = useState<Chunk | null>(null);
      useEffect(() => {
        let live = true;
        void load().then((c) => live && setChunk(() => c));
        return () => void (live = false);
      }, []);
      if (!chunk) return null;
      const C = pick(chunk);
      return <C {...props} reach={{ session: session(), own }} />;
    };
  return { LivePane: lazy((c) => c.LivePane), LiveAsk: lazy((c) => c.LiveAsk), CardWhy: lazy((c) => c.CardWhy), LiveWork: lazy<{ watch?: boolean }>((c) => c.WorkSlot) };
}
