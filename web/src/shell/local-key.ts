// This daemon's page key (crates/daemon/src/localauth.rs). A browser on the
// daemon's own machine proves it may drive it with the page key, which the
// sign-in link (`arugula web`, the desktop app) stores in this origin's
// localStorage. Origins are per port, so no other server on 127.0.0.1 can
// read it, as it could a cookie: browsers send cookies to every port.
//
// Imported first: from then on the page's own requests carry it (fetch,
// XMLHttpRequest and sendBeacon as `X-Arugula-Key`, the WebSocket in its URL,
// where a browser can't set a header), and a refusal that says to sign in
// sends the page to the sign-in link's page, which signs a browser in from
// an older link's cookie or says how to. Only on loopback pages: through
// serve, on the tailnet or on control, identity is theirs.

/** Where the sign-in link's page puts the key. */
const STORE = "arugula.key";
/** The header the daemon reads it from (access.rs `PAGE_KEY_HEADER`). */
const HEADER = "x-arugula-key";
/** The WebSocket's parameter for it (server.rs `page_key_from_query`). */
const PARAM = "arugula_key";
/** What the daemon's sign-in refusal carries (server.rs `guard`). */
const SIGN_IN = "x-arugula-sign-in";
/** Where the service worker finds it (it has no localStorage). */
export const KEY_CACHE = "arugula-key";

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

/** A WebSocket URL with the key in it, when it is our daemon's. */
export function keyedSocketUrl(url: string | URL, page: { href: string; host: string }, key: string | null): string | URL {
  const u = key ? ownUrl(url, page) : null;
  if (!u || !key) return url;
  u.searchParams.set(PARAM, key);
  return u.toString();
}

/** To the sign-in link's page, at most every half minute (no loop when it
 * can't sign us in: it says how instead). */
function signIn() {
  try {
    const at = Number(sessionStorage.getItem("arugula.signin") ?? 0);
    if (Date.now() - at < 30_000) return;
    sessionStorage.setItem("arugula.signin", String(Date.now()));
    localStorage.removeItem(STORE);
  } catch {
    /* storage refused: go anyway */
  }
  location.replace(`/auth?next=${encodeURIComponent(location.pathname + location.search + location.hash)}`);
}

function install() {
  const page = location;
  const plainFetch = window.fetch.bind(window);
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const own = ownUrl(input instanceof Request ? input.url : input, page);
    const key = own ? pageKey() : null;
    let req: RequestInfo | URL = input;
    let opts = init;
    if (key) {
      const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
      headers.set(HEADER, key);
      if (input instanceof Request) {
        req = new Request(input, { ...init, headers });
        opts = undefined;
      } else {
        opts = { ...init, headers };
      }
    }
    const res = await plainFetch(req, opts);
    if (own && res.status === 401 && res.headers.get(SIGN_IN)) signIn();
    return res;
  };

  type Xhr = XMLHttpRequest & { arugulaOwn?: boolean };
  const open = XMLHttpRequest.prototype.open as (this: XMLHttpRequest, ...a: unknown[]) => void;
  XMLHttpRequest.prototype.open = function (this: Xhr, ...a: unknown[]) {
    this.arugulaOwn = !!ownUrl(a[1] as string | URL, page);
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
      if (!ownUrl(url, page) || !pageKey()) return beacon(url, data);
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
  } else {
    // Not signed in here (or signed in by an older link's cookie): ask once,
    // so a refusal sends us to sign in before the app waits on a socket.
    void window.fetch("/api/host").catch(() => {});
  }
}

if (typeof window !== "undefined" && onLoopback(location)) install();
