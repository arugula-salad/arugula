// Arugula control's relay as a transport (S43): frames are POSTed to
// `/api/people/outline` (and `/outline/off`), and a page hears the people
// it shares a team with over Server-Sent Events from `/api/people/outlines`,
// whose `frame` events carry `{account, at, data}` (data in base64). Who
// sees whom is control's (team rosters); there are no members or waves.

import type { Transport, TransportEvents } from "./transport.ts";

export interface ArugulaTransportOptions {
  /** fetch, or one that waits out control's maintenance (Arugula's
   * platformFetch). */
  fetch?: (input: string, init?: RequestInit) => Promise<Response>;
  /** Where control's API is: the page's own origin by default. */
  base?: string;
}

export function arugulaTransport(o: ArugulaTransportOptions = {}): Transport {
  const base = o.base ?? "";
  const f = o.fetch ?? ((input: string, init?: RequestInit) => fetch(input, init));
  let es: EventSource | null = null;
  return {
    listen(on: TransportEvents) {
      if (es || typeof EventSource === "undefined") return;
      on.live("connecting");
      const s = new EventSource(`${base}/api/people/outlines`);
      es = s;
      s.addEventListener("open", () => on.live("on"));
      s.addEventListener("frame", (e) => {
        try {
          const m = JSON.parse((e as MessageEvent).data) as { account: string; data: string };
          on.frame(m.account, Uint8Array.from(atob(m.data), (c) => c.charCodeAt(0)));
        } catch {
          // Not a frame event this page reads: skipped.
        }
      });
      s.addEventListener("off", (e) => on.off((JSON.parse((e as MessageEvent).data) as { account: string }).account));
      s.addEventListener("error", () => {
        // EventSource tries again by itself; a refusal (chat off) closes it.
        if (es === s) on.live(s.readyState === EventSource.CLOSED ? "refused" : "connecting");
      });
    },
    unlisten() {
      es?.close();
      es = null;
    },
    publish(bytes) {
      f(`${base}/api/people/outline`, {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: bytes as BodyInit,
        credentials: "same-origin",
      }).catch(() => {
        // A dropped frame: the next one says the same.
      });
    },
    off() {
      f(`${base}/api/people/outline/off`, { method: "POST", credentials: "same-origin" }).catch(() => {});
    },
    leaving() {
      navigator.sendBeacon?.(`${base}/api/people/outline/off`);
    },
  };
}
