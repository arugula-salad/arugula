// A workspace block's graph (#620): behold's estate graph, run by the block
// on its host and framed here through the block's own site. A pick in
// behold (a member's box or a card) opens that member's Shell or Changes in
// the block; the block's env and its settled reads drive behold.

import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../proto";
import type { WorkspaceState } from "./workspace";

export type GraphStatus =
  | { is: "off" }
  | { is: "starting" }
  | { is: "running"; src: string; run: number }
  | { is: "failed"; error: string };

/** What a click in the graph opens on its member. */
export type Clicks = "shell" | "changes" | "none";

/** behold at `root` (its site), framed in this page (`host`, an origin), on
 * `member` (or the whole estate) and `env` ("" is the source graph), its
 * gate strip reading `gates` (the block's env, whatever the graph shows). */
function beholdSrc(root: string, member: string | null, env: string, gates: string, host: string): string {
  const q = new URLSearchParams();
  if (member) q.set("member", member);
  q.set("env", env);
  q.set("gates", gates);
  q.set("embed", "1");
  q.set("host", host);
  q.set("theme", "dark");
  return `${root.replace(/\/$/, "")}/?${q}`;
}

/** A `behold:select` from the frame at `origin`: the member picked, or null
 * for any other message. */
function beholdPick(data: unknown, from: string, origin: string): string | null {
  if (from !== origin || !data || typeof data !== "object") return null;
  const d = data as { type?: unknown; member?: unknown };
  return d.type === "behold:select" && typeof d.member === "string" && d.member ? d.member : null;
}

/** behold's estate graph in the block (#620), on `member`. It opens on the
 * source graph; a live read of the block's env is a choice, since it needs
 * this host's credentials. A click on a member or a card in it opens that
 * member's Shell or Changes here. */
export function Graph({ client, id, s, member, close, picked }: {
  client: Client; id: PaneId; s: WorkspaceState; member: string | null; close: () => void; picked: (member: string, clicks: Clicks) => void;
}) {
  const g = s.graph ?? { is: "off" };
  const frame = useRef<HTMLIFrameElement>(null);
  const [clicks, setClicks] = useState<Clicks>("shell");
  const [live, setLive] = useState(false);
  // The address a run of behold loads with; later moves are messages.
  // behold reads `gates` once, as it loads: a new block env loads it again.
  const loaded = useRef<{ run: number; gates: string; src: string } | null>(null);
  if (g.is === "running" && (loaded.current?.run !== g.run || loaded.current.gates !== s.env)) {
    loaded.current = { run: g.run, gates: s.env, src: beholdSrc(g.src, member, live ? s.env : "", s.env, location.origin) };
  }
  const origin = g.is === "running" ? new URL(g.src).origin : "";
  const view = (v: Record<string, unknown>) => {
    if (origin) frame.current?.contentWindow?.postMessage({ type: "behold:view", ...v }, origin);
  };
  useEffect(() => view({ member }), [member, origin]);
  // The live box moves the graph between the source and the block's env.
  useEffect(() => view({ env: live ? s.env : "" }), [live, origin]);
  // Read when a pick comes, so one right after a change in the bar (before
  // effects run again) goes by the bar as it is now.
  const now = useRef({ clicks, picked });
  now.current = { clicks, picked };
  useEffect(() => {
    const on = (e: MessageEvent) => {
      if (e.source !== frame.current?.contentWindow) return;
      const m = beholdPick(e.data, e.origin, origin);
      const { clicks, picked } = now.current;
      if (m && clicks !== "none") picked(m, clicks);
    };
    window.addEventListener("message", on);
    return () => window.removeEventListener("message", on);
  }, [origin]);
  return (
    <section class="ws-graph" data-ws-graph>
      <div class="ws-graph-bar">
        <b>Graph</b>
        <span class="dim">{member ?? "whole estate"}</span>
        <label class="dim" title={`Reads ${s.env} live through behold, with this host's credentials`}>
          <input type="checkbox" data-ws-graph-live checked={live} onChange={(e) => setLive(e.currentTarget.checked)} /> live {s.env}
        </label>
        <label class="dim">
          a click opens{" "}
          <select class="ws-env" data-ws-graph-clicks value={clicks} onChange={(e) => setClicks(e.currentTarget.value as Clicks)}>
            <option value="shell">Shell</option>
            <option value="changes">Changes</option>
            <option value="none">nothing</option>
          </select>
        </label>
        <button title="Close the graph" onClick={close}>
          ✕
        </button>
      </div>
      {g.is === "running" && loaded.current ? (
        // Its own origin, the block's site, never the app's.
        <iframe
          key={`${loaded.current.run}:${loaded.current.gates}`}
          ref={frame}
          class="browser-frame"
          src={loaded.current.src}
          title={`behold: ${s.name ?? "workspace"}`}
          sandbox="allow-scripts allow-forms allow-same-origin"
          referrerpolicy="no-referrer"
        />
      ) : g.is === "failed" ? (
        <div class="browser-card" data-ws-graph-error>
          <p>behold didn't start</p>
          <p class="dim">{g.error}</p>
          <button onClick={() => void client.api(`/api/blocks/${id}/call/graph`, {}, "couldn't start behold")}>Try again</button>
        </div>
      ) : (
        <div class="browser-card dim">Starting behold…</div>
      )}
    </section>
  );
}
