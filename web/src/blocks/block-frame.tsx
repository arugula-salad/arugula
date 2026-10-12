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
// channel actually connected), or "own site" once it fell back.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../proto";
import type { Route } from "../block/channel.ts";
import { routeTitle, setBlockRoute, type BlockRoute } from "./block-route";
import { blockOrigin, blockSrc, credsFor, frameBlock } from "./through-control";

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
  const [went, setWent] = useState<Route | null>(null);
  const ref = useRef<HTMLIFrameElement>(null);
  const via = failed === null ? origin : null;
  // Before paint, so the frame is answered before it can load and ask
  // (the frame asks again until it is, too).
  useLayoutEffect(() => {
    const frame = ref.current;
    if (!via || !frame) return;
    const block = String(p.id);
    setWent(null);
    return frameBlock({ frame, origin: via, creds: () => credsFor(p.client, block), failed: (why) => setFailed(why), routed: setWent });
  }, [via, p.id, p.src, p.reloads]);

  const own = ownSite(p.src);
  // Only a block opened through control says which way: one on the
  // daemon's own page is always on its own site.
  const route: BlockRoute | null = via ? went : failed !== null && own ? "own site" : null;
  useEffect(() => {
    setBlockRoute(p.id, route);
    return () => setBlockRoute(p.id, null);
  }, [p.id, route]);
  const pill = route && (
    <span class={`block-route host-path ${route === "relayed" ? "relayed" : ""}`} data-block-route={route} title={routeTitle(route)}>
      {route}
    </span>
  );
  const frame = (src: string) => (
    <iframe
      ref={ref}
      key={`${src}#${p.reloads}`}
      class={p.class}
      src={src}
      title={p.title}
      sandbox={p.sandbox}
      allow={p.allow}
      referrerpolicy="no-referrer"
      data-block-origin={via ?? undefined}
    />
  );
  if (via)
    return (
      <>
        {frame(blockSrc(p.src, via))}
        {pill}
      </>
    );
  if (own)
    return (
      <>
        {failed !== null && (
          <div class="block-note" data-block-fallback>
            Via the machine's own block site: this browser couldn't open it through Arugula control ({failed}).
          </div>
        )}
        {frame(own)}
        {pill}
      </>
    );
  return (
    <div class="browser-card error" data-block-nowhere>
      {failed !== null ? (
        <>
          <p>This browser can't show this block through Arugula control</p>
          <p class="dim">{failed}</p>
          <p class="dim">This machine has no block site of its own to fall back to. Try Chrome, or another browser.</p>
        </>
      ) : (
        <>
          <p>This block shows in Arugula control's page</p>
          <p class="dim">This machine has no block site of its own (--block-listen), so its ports and editors open from control.</p>
        </>
      )}
    </div>
  );
}
