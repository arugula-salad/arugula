// This daemon's page key (crates/daemon/src/localauth.rs). A browser on the
// daemon's own machine proves it may drive it with the page key, which the
// sign-in link (`arugula web`, the desktop app) stores in this origin's
// localStorage. Origins are per port, so no other server on 127.0.0.1 can
// read it, as it could a cookie: browsers send cookies to every port.
//
// Imported first: from then on the page's requests to its own daemon's API
// carry it (fetch, XMLHttpRequest and sendBeacon as `X-Arugula-Key`, the
// WebSocket in its URL, where a browser can't set a header). Nothing else
// does, not even another path on the same origin, and a keyed fetch doesn't
// follow redirects: a browser keeps a custom header across a redirect to
// another site. A page with no key, or one refused with the cue to sign in,
// goes to the sign-in link's page (the app held back meanwhile), which gives
// a key to a browser it can (an older link's cookie, for a while after an
// upgrade) and says how to sign in to one it can't. Only on loopback pages:
// through serve, on the tailnet or on control, identity is theirs.

import { KEY_CACHE } from "./local-key-cache.ts";

/** Where the sign-in link's page puts the key. */
const STORE = "arugula.key";
/** The header the daemon reads it from (access.rs `PAGE_KEY_HEADER`). */
const HEADER = "x-arugula-key";
/** The WebSocket's parameter for it (server.rs `page_key_from_query`). */
const PARAM = "arugula_key";
/** What the daemon's sign-in refusal carries (server.rs `guard`). */
const SIGN_IN = "x-arugula-sign-in";

const LOOPBACK = ["127.0.0.1", "localhost", "[::1]"];

/** A page of this machine's daemon, on loopback (or `just dev`'s Vite). */
export const onLoopback = (loc: { protocol: string; hostname: string }) => loc.protocol === "http:" && LOOPBACK.includes(loc.hostname);

export function pageKey(): string | null {
  try {
    return localStorage.getItem(STORE);
  } catch {
    return null;
  }
}

/** `url` if it is this page's own daemon's (http or ws, this host and port). */
export function ownUrl(url: string | URL, page: { href: string; host: string }): URL | null {
  try {
    const u = new URL(url, page.href);
    return (u.protocol === "http:" || u.protocol === "ws:") && u.host === page.host ? u : null;
  } catch {
    return null;
  }
}

/** `url` if it is our daemon's API (`/api/…`, `/ws`), or its ways through
 * to other hosts (`/h/<name>/…`, `/tunnel/<name>/…`; the daemon strips the
 * key before passing a request on, and other sites' redirects from the
 * answer): what the key goes to. */
export function ownApiUrl(url: string | URL, page: { href: string; host: string }): URL | null {
  const u = ownUrl(url, page);
  if (!u) return null;
  const p = u.pathname;
  return /^\/(api\/|ws$)/.test(p) || /^\/(h|tunnel)\/[^/]+\/(api\/|ws$)/.test(p) ? u : null;
}

/** A WebSocket URL with the key in it, when it is our daemon's. */
export function keyedSocketUrl(url: string | URL, page: { href: string; host: string }, key: string | null): string | URL {
  const u = key ? ownApiUrl(url, page) : null;
  if (!u || !key) return url;
  u.searchParams.set(PARAM, key);
  return u.toString();
}

/** A fetch's arguments with the key on, for a request to our daemon's API
 * (`ownApiUrl`; none without a key). The daemon's API never redirects, so a
 * keyed request doesn't follow one: a browser would take the header along. */
export function keyedRequest(input: RequestInfo | URL, init: RequestInit | undefined, key: string | null): [RequestInfo | URL, RequestInit | undefined] {
  if (!key) return [input, init];
  const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
  headers.set(HEADER, key);
  if (input instanceof Request) return [new Request(input, { ...init, headers, redirect: "manual" }), undefined];
  return [input, { ...init, headers, redirect: "manual" }];
}

/** On its way to the sign-in link's page. */
let leaving = false;

/** To the sign-in link's page, at most every half minute (no loop when it
 * can't sign us in: it says how instead). Whether it went. */
function signIn(): boolean {
  try {
    const at = Number(sessionStorage.getItem("arugula.signin") ?? 0);
    if (Date.now() - at < 30_000) return false;
    sessionStorage.setItem("arugula.signin", String(Date.now()));
    localStorage.removeItem(STORE);
  } catch {
    /* storage refused: go anyway */
  }
  location.replace(`/auth?next=${encodeURIComponent(location.pathname + location.search + location.hash)}`);
  return true;
}

/** Whether the page has no key yet. */
function install(): boolean {
  const page = location;
  const plainFetch = window.fetch.bind(window);
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const own = ownApiUrl(input instanceof Request ? input.url : input, page);
    const [req, opts] = keyedRequest(input, init, own ? pageKey() : null);
    const res = await plainFetch(req, opts);
    if (own && res.status === 401 && res.headers.get(SIGN_IN)) leaving ||= signIn();
    return res;
  };

  type Xhr = XMLHttpRequest & { arugulaOwn?: boolean };
  const open = XMLHttpRequest.prototype.open as (this: XMLHttpRequest, ...a: unknown[]) => void;
  XMLHttpRequest.prototype.open = function (this: Xhr, ...a: unknown[]) {
    // XMLHttpRequest always follows redirects: only the daemon's API, whose
    // answers never redirect to another site.
    this.arugulaOwn = !!ownApiUrl(a[1] as string | URL, page);
    return open.apply(this, a);
  } as typeof XMLHttpRequest.prototype.open;
  const send = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function (this: Xhr, body?: Document | XMLHttpRequestBodyInit | null) {
    const key = this.arugulaOwn ? pageKey() : null;
    if (key) this.setRequestHeader(HEADER, key);
    return send.call(this, body);
  };

  if (navigator.sendBeacon) {
    const beacon = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = (url: string | URL, data?: BodyInit | null) => {
      if (!ownApiUrl(url, page) || !pageKey()) return beacon(url, data);
      void window.fetch(url, { method: "POST", body: data ?? null, keepalive: true }).catch(() => {});
      return true;
    };
  }

  const Plain = window.WebSocket;
  class KeyedWebSocket extends Plain {
    constructor(url: string | URL, protocols?: string | string[]) {
      super(keyedSocketUrl(url, page, pageKey()), protocols);
    }
  }
  window.WebSocket = KeyedWebSocket;

  const key = pageKey();
  if (key) {
    // For the service worker's own requests (a notification's answer).
    void globalThis.caches
      ?.open(KEY_CACHE)
      .then((c) => c.put("/key", new Response(key)))
      .catch(() => {});
    return false;
  }
  return true;
}

/** No key here: ask the daemon once whether this page may go on (it may be
 * let in another way: control's own page, or serve's identity), and if it
 * says to sign in, go to the sign-in link's page, which gives a key to a
 * browser it can (an older link's cookie, for a while after an upgrade) and
 * says how to sign in to one it can't. Whether the page is leaving. */
async function keyless(): Promise<boolean> {
  await window.fetch("/api/host", { signal: AbortSignal.timeout(3000) }).catch(() => null);
  return leaving;
}

// The app waits for that answer, and doesn't start on its way out: its own
// changes to the address (a hash, replaceState) would cancel the navigation.
if (typeof window !== "undefined" && onLoopback(location) && install() && (await keyless())) {
  await new Promise(() => {});
}
