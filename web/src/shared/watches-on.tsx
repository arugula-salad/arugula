// Agents' watches waiting on a PR or a thread (Arugula #1036): whose agent
// waits, for what, and until when, for the people of its project who can
// read what it waits on. A session is named only as its owner's
// `agent_sessions` setting shows it (aru-133); else it's their agent. The
// reader's own watch has Cancel, which tells its session. Read from control
// as the signed-in person, so on a machine's own page (no control session)
// the read fails and nothing shows.

import { useEffect, useState } from "preact/hooks";
import { agentOf } from "./agent-label";
import { call } from "./chat-store";
import { sessionLabel } from "./chief-of-staff";
import { usePlatformApi } from "./platform-api";
import type { WatchShown, WatchesOn as Watches } from "./proto";
import { Time } from "./ui/time";
import "./watches-on.css";

/** How often an open block asks again. */
const EVERY_MS = 30_000;

/** The watches at `path` (none: nothing to read), read while it's shown. */
export function useWatchesOn(path: string | null): [WatchShown[], () => void] {
  const [watches, setWatches] = useState<WatchShown[]>([]);
  const [asked, setAsked] = useState(0);
  useEffect(() => {
    setWatches([]);
    if (!path) return;
    let live = true;
    const read = () => {
      if (document.visibilityState !== "visible") return;
      call<Watches>("GET", path)
        .then((r) => live && setWatches(r.watches))
        .catch(() => {});
    };
    read();
    const every = setInterval(read, EVERY_MS);
    return () => {
      live = false;
      clearInterval(every);
    };
  }, [path, asked]);
  return [watches, () => setAsked((n) => n + 1)];
}

/** Whose agent it is, as the reader may know it: a chief of staff by its
 * project's name, with `projectName` (Arugula #1457). */
export function watcher(w: WatchShown, projectName?: (id: string) => string | undefined): string {
  return w.session ? `${agentOf(w.owner_name)} ${sessionLabel(w.session, projectName)}` : agentOf(w.owner_name);
}

/** The watches on a PR or a thread; nothing when there are none. */
export function WatchesOn({ path, compact }: { path: string | null; compact?: boolean }) {
  const [watches, again] = useWatchesOn(path);
  const [failed, setFailed] = useState<string | null>(null);
  const { projectName } = usePlatformApi();
  if (!watches.length) return null;
  const cancel = (w: WatchShown) => {
    setFailed(null);
    call("POST", `/api/watches/${encodeURIComponent(w.id)}/cancel`)
      .then(again)
      .catch((e: Error) => setFailed(e.message));
  };
  return (
    <section class={compact ? "watches-on compact" : "watches-on"} aria-label="Agents waiting on this" data-watches-on>
      <h4>Watched by</h4>
      {watches.map((w) => (
        <div key={w.id} class="watch-on" data-watch-on={w.id}>
          <span class="watch-on-text">
            <span class="watch-on-who">{watcher(w, projectName)}</span>
            <span class="watch-on-what">waits for {w.waits}</span>
            <span class="watch-on-until dim">
              until <Time at={w.expires_ms} />
            </span>
          </span>
          {w.yours && (
            <button type="button" class="secondary small" data-watch-cancel={w.id} onClick={() => cancel(w)}>
              Cancel
            </button>
          )}
        </div>
      ))}
      {failed && <div class="watch-on-error">{failed}</div>}
    </section>
  );
}
