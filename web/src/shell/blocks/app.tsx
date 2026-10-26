// App blocks (M35): an app's box in a frame, on its own origin.
//
// Getting in is the box's: it signs whoever looks with their Arugula
// account, through the hub, so nothing is minted or kept here and a
// teammate who views the block sees the app as themselves (native #206
// retired the studio login that minted the owner's way in). ↻ draws the
// frame again here; the block's `reload` does it for every client.
//
// The frame shows once a page has loaded in it. A box that takes the
// connection and never answers (asleep, or hung) leaves a frame loading
// forever, which draws as a blank white page: after `STUCK_AFTER` the card
// says so instead, and the frame comes in if the box answers late.

import { render } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { registerBlock, type BlockView } from "./view";
import { BlockError, BlockLoading, OpenOutside } from "./block-states";

export interface AppState {
  box_url: string;
  title: string | null;
  /** Its name where it came from, if it was given one. */
  app: string | null;
  /** Bumped by the block's `reload`: every client draws the frame again. */
  reloads: number;
}

/** How long the box may take before the card says it isn't answering. */
const STUCK_AFTER = 15_000;

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

const nameOf = (s: AppState) => s.title ?? s.app ?? hostOf(s.box_url);

function AppBlock({ s }: { s: AppState | null }) {
  // ↻ here: a frame of its own, keyed with the block's reloads.
  const [mine, setMine] = useState(0);
  // opening: no page yet; in: one loaded; stuck: none came in time.
  const [phase, setPhase] = useState<"opening" | "in" | "stuck">("opening");
  const stuck = useRef<number | null>(null);
  const key = `${s?.reloads ?? 0}:${mine}`;
  useEffect(() => {
    if (!s) return;
    setPhase((p) => (p === "in" ? p : "opening"));
    stuck.current = window.setTimeout(() => {
      stuck.current = null;
      setPhase((p) => (p === "in" ? p : "stuck"));
    }, STUCK_AFTER);
    return () => {
      if (stuck.current !== null) clearTimeout(stuck.current);
      stuck.current = null;
    };
  }, [!!s, key]);

  if (!s) return <BlockLoading what="the app" />;
  const name = nameOf(s);
  const again = () => {
    setPhase("opening");
    setMine((n) => n + 1);
  };
  return (
    <div class="browser app-block" data-app={s.app ?? hostOf(s.box_url)}>
      <div class="browser-bar">
        <button title="Reload" aria-label="Reload" onClick={again}>
          ↻
        </button>
        <span class="app-name" title={s.box_url}>
          {name}
        </span>
        {name !== hostOf(s.box_url) && <span class="dim app-host">{hostOf(s.box_url)}</span>}
        <OpenOutside href={s.box_url} />
      </div>
      {phase === "opening" && <BlockLoading what={name} />}
      {phase === "stuck" && (
        <BlockError
          data-app-stuck
          title={`${name} isn't answering`}
          detail={`${hostOf(s.box_url)} hasn't sent a page in ${STUCK_AFTER / 1000} seconds. It shows here if it answers.`}
          retry={again}
        />
      )}
      {/* The box's own origin: it signs the person in itself. */}
      <iframe
        key={key}
        class="browser-frame"
        style={phase === "in" ? undefined : { display: "none" }}
        src={s.box_url + "/"}
        title={name}
        sandbox="allow-scripts allow-forms allow-same-origin allow-popups"
        referrerpolicy="no-referrer"
        onLoad={() => {
          if (stuck.current !== null) clearTimeout(stuck.current);
          stuck.current = null;
          setPhase("in");
        }}
      />
    </div>
  );
}

registerBlock("app", (_client: Client, _id: PaneId): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-browser block-app";
  let state: AppState | null = null;
  const draw = () => render(<AppBlock s={state} />, host);
  draw();
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as AppState;
      draw();
    },
    title: () => (state ? nameOf(state) : "app"),
    text: () => (state ? `${nameOf(state)}\n${state.box_url}` : ""),
    focus: () => host.querySelector<HTMLElement>("iframe")?.focus(),
    dispose: () => {
      render(null, host);
      host.remove();
    },
  };
});
