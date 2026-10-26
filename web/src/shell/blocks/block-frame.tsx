// The frame of a port or editor block (M50): on control's block origin when
// this page reaches the daemon through a control that serves them, else on
// the daemon's own block site (`--block-listen`), as before.
//
// When the block can't load through control in this browser (its frame
// says why: no service worker in a cross-site frame, say), it falls back
// to the daemon's own block site where there is one, and says so where
// there's none.
//
// A block opened through control shows which way it went, in the host
// menu's words: "direct" or "relayed" (the frame says which way its
// channel actually connected), or "own site" once it fell back: a pill in
// its bar (RoutePill), not over the page (UI audit #177).

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { useSubscribe } from "../../shared/ui/hooks";
import { blockRoute, onBlockRoutes, routeTitle, setBlockRoute, type BlockRoute } from "./block-route";
import { BlockEmpty, BlockError } from "./block-states";
import { blockOrigin, blockSrc, credsFor, frameAllow, frameBlock, type Routing } from "./through-control";

export interface FrameProps {
  client: Client;
  id: PaneId;
  /** The block's origin key on control's block domain (from its state). */
  siteKey: string | null | undefined;
  /** The daemon's address for it: on its own block site, or a path. */
  src: string;
  reloads: number;
  class: string;
  title: string;
  sandbox: string;
  allow?: string;
}

/** Where this page frames a block through control, if it does. */
export function throughControl(client: Client, siteKey: string | null | undefined): string | null {
  const domain = client.blockDomain();
  if (!domain || !siteKey || client.features?.block_channels === false) return null;
  return blockOrigin(siteKey, domain, location);
}

/** The daemon's own block site for it, when it has one. */
const ownSite = (src: string) => (/^https?:\/\//.test(src) ? src : null);

export function BlockFrame(p: FrameProps) {
  const origin = throughControl(p.client, p.siteKey);
  const [failed, setFailed] = useState<string | null>(null);
  // Which way the worker's channel went (the block's requests), and the
  // pages' own (their WebSockets), by the frame's word.
  const [went, setWent] = useState<Routing | null>(null);
  const [wsWent, setWsWent] = useState<Routing | null>(null);
  const ref = useRef<HTMLIFrameElement>(null);
  const via = failed === null ? origin : null;
  // Before paint, so the frame is answered before it can load and ask
  // (the frame asks again until it is, too).
  useLayoutEffect(() => {
    const frame = ref.current;
    if (!via || !frame) return;
    const block = String(p.id);
    setWent(null);
    setWsWent(null);
    const routed = (r: Routing) => {
      // For a live check (#849): the frame's worker logs in its own console.
      console.info(`arugula block ${block}: its ${r.ws ? "pages' WebSockets" : "requests"} went ${r.route}${r.why ? `: ${r.why}` : ""}`);
      (r.ws ? setWsWent : setWent)(r);
    };
    return frameBlock({ frame, origin: via, creds: () => credsFor(p.client, block), failed: (why) => setFailed(why), routed });
  }, [via, p.id, p.src, p.reloads]);

  const own = ownSite(p.src);
  // Only a block opened through control says which way: one on the
  // daemon's own page is always on its own site.
  const route: BlockRoute | null = via ? (went?.route ?? null) : failed !== null && own ? "own site" : null;
  const title = route && [routeTitle(route), went?.why && `Not direct: ${went.why}`, wsWent && `Its WebSockets: ${wsWent.route}${wsWent.why ? ` (${wsWent.why})` : ""}`].filter(Boolean).join("\n");
  useEffect(() => {
    setBlockRoute(p.id, route && { route, title: title || "", why: went?.why, ws: wsWent?.route });
    return () => setBlockRoute(p.id, null);
  }, [p.id, route, title]);
  const frame = (src: string) => (
    <iframe
      ref={ref}
      key={`${src}#${p.reloads}`}
      class={p.class}
      src={src}
      title={p.title}
      sandbox={p.sandbox}
      allow={via ? frameAllow(p.allow) : p.allow}
      referrerpolicy="no-referrer"
      data-block-origin={via ?? undefined}
    />
  );
  if (via) return frame(blockSrc(p.src, via));
  if (own)
    return (
      <>
        {failed !== null && (
          <div class="block-note" data-block-fallback>
            Via the machine's own block site: this browser couldn't open it through Arugula control ({failed}).
          </div>
        )}
        {frame(own)}
      </>
    );
  return failed !== null ? (
    <BlockError data-block-nowhere title="This browser can't show this block through Arugula control" detail={failed}>
      <p class="block-error-detail">This machine has no block site of its own to fall back to. Try Chrome, or another browser.</p>
    </BlockError>
  ) : (
    <BlockEmpty data-block-nowhere>
      <p>This block shows in Arugula control's page</p>
      <p>This machine has no block site of its own (--block-listen), so its ports and editors open from control.</p>
    </BlockEmpty>
  );
}

/** Which way block `id` went, in its bar: "direct", "relayed" or "own
 * site"; nothing when it isn't shown through control. */
export function RoutePill({ id }: { id: PaneId }) {
  useSubscribe(onBlockRoutes);
  const r = blockRoute(id);
  if (!r) return null;
  return (
    <span class={`block-route host-path ${r.route === "relayed" ? "relayed" : ""}`} data-block-route={r.route} data-block-route-why={r.why} data-block-ws-route={r.ws} title={r.title || undefined}>
      {r.route}
    </span>
  );
}
