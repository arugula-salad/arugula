// Chrome's Local Network Access and a block's worker (#849).

/** Why this browser won't let a block's service worker go straight to the
 * daemon, or null when it might. Chrome (every Chromium) checks a service
 * worker's local network requests against the worker's own origin's Local
 * Network Access permission (the LNA spec's note: Chromium "applies the
 * LNA permission for service worker-initiated fetches based on the
 * worker's script origin"), and a worker can't ask for it. A block's
 * origin (`b-<key>.<block domain>`) never has it: a frame's own requests
 * are asked on behalf of control's page (the iframe's `allow`), a worker's
 * aren't. WebSockets are checked since Chrome 147, and a tailnet's
 * 100.64.0.0/10 is "local". So the worker doesn't try, and goes through
 * the relay; the pages' own channel (their WebSockets, ./shim.ts) can
 * still go direct. */
export function workerDirectBlocked(ua: string): string | null {
  const m = /Chrom(?:e|ium)\/(\d+)/.exec(ua);
  if (m && Number(m[1]) >= 147) return "Chrome lets a block's worker reach the local network only with its own origin's Local Network Access permission, which a block can't ask for";
  return null;
}
