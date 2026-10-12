// A block's service worker (M50; S27's web/sw.ts): control's code, on the
// block's own origin. It answers every request of the block's pages from
// the daemon, over the block's Noise channel; control only ever relays
// ciphertext.
//
// - The channel is opened on demand and again after it drops (the browser
//   stops an idle worker, which closes its socket; control closes one idle
//   for an hour).
// - A grant refused as `expired` asks the pages for a new one: they ask
//   the parent page, which signs it, and hand it back here.
// - HTML pages get the WebSocket shim first in their <head>.
// - Cookies: a response built here can't set them, and requests here
//   don't carry them, so the worker keeps the block's cookie jar itself.
// - Each request says `sec-fetch-mode` and `sec-fetch-dest` (from its
//   `mode` and `destination`), which the daemon's proxy reads.
// - Which way each channel went (direct, or relayed after a direct route
//   didn't connect) goes to the block's pages, whose shim tells the parent.
//
// Built by itself into dist/block/sw.js (vite.block.config.ts) and served
// by control at /.arugula/sw.js, with no CSP: hosting an app's own worker
// (below) runs its code with `new Function`.

import { BlockChannel, type Held, load, openChannel, Refused, save } from "./channel.ts";
import { TAG, VERSION } from "./page.ts";

interface Waiting {
  waitUntil(p: Promise<unknown>): void;
}
interface FetchEvent extends Waiting {
  request: Request;
  clientId: string;
  resultingClientId: string;
  respondWith(r: Response | Promise<Response>): void;
}
interface MessageEvent_ extends Waiting {
  data: unknown;
  ports: readonly MessagePort[];
  source: { postMessage(m: unknown): void } | null;
}
interface Worker {
  clients: {
    claim(): Promise<void>;
    matchAll(o?: { type?: string; includeUncontrolled?: boolean }): Promise<{ postMessage(m: unknown): void }[]>;
  };
  location: Location;
  skipWaiting(): Promise<void>;
  addEventListener(type: string, fn: (e: never) => void): void;
}

const sw = self as unknown as Worker;
const SHIM = `<script src="/.arugula/shim.js"></script>`;
const NULL_BODY = new Set([101, 103, 204, 205, 304]);

let held: Held | undefined;
let chan: Promise<BlockChannel> | null = null;
let jar: Record<string, string> | undefined;
let waiting: ((h: Held) => void)[] = [];

sw.addEventListener("install", () => void sw.skipWaiting());
sw.addEventListener("activate", (e: Waiting) => e.waitUntil(sw.clients.claim()));

sw.addEventListener("message", (e: MessageEvent_) => {
  const m = e.data as { arugula?: string; v?: number; held?: Held; ask?: string } | null;
  if (m?.arugula !== TAG || m.v !== VERSION) return;
  // A page of ours asks which way the channel went (it loaded after the
  // channel opened, so it missed `told`).
  if (m.ask === "route") {
    const to = e.source;
    if (to && chan) e.waitUntil(chan.then((ch) => to.postMessage({ arugula: TAG, v: VERSION, route: ch.route }), () => {}));
    return;
  }
  if (!m.held) return;
  held = m.held;
  chan = null;
  for (const w of waiting) w(m.held);
  waiting = [];
  e.waitUntil(save("held", m.held).then(() => e.ports[0]?.postMessage({ ok: true })));
});

async function getHeld(): Promise<Held | undefined> {
  held ??= await load<Held>("held");
  return held;
}

/** New credentials, from the parent page by way of a page of ours. */
async function renew(): Promise<Held> {
  const got = new Promise<Held>((res, rej) => {
    waiting.push(res);
    setTimeout(() => rej(new Error("no new grant came")), 10_000);
  });
  for (const c of await sw.clients.matchAll({ type: "window", includeUncontrolled: true })) c.postMessage({ arugula: TAG, v: VERSION, ask: "renew" });
  return got;
}

async function connect(): Promise<BlockChannel> {
  let h = await getHeld();
  if (!h) throw new Error("this block has no key");
  for (let attempt = 0; ; attempt++) {
    try {
      return await openChannel(h.creds, sw.location);
    } catch (e) {
      if (e instanceof Refused && e.code === "expired" && attempt === 0) {
        h = await renew();
        continue;
      }
      throw e;
    }
  }
}

/** Tell the block's pages which way a new channel went. */
async function told(ch: BlockChannel) {
  for (const c of await sw.clients.matchAll({ type: "window" })) c.postMessage({ arugula: TAG, v: VERSION, route: ch.route });
}

function channel(): Promise<BlockChannel> {
  if (!chan) {
    const p = connect().then((ch) => {
      ch.onclose = () => {
        if (chan === p) chan = null;
      };
      void told(ch);
      return ch;
    });
    p.catch(() => {
      if (chan === p) chan = null;
    });
    chan = p;
  }
  return chan;
}

async function cookies(): Promise<Record<string, string>> {
  jar ??= (await load<Record<string, string>>("jar")) ?? {};
  return jar;
}

function keep(setCookie: string[]) {
  if (!setCookie.length || !jar) return;
  for (const sc of setCookie) {
    const [pair, ...attrs] = sc.split(";");
    const i = pair.indexOf("=");
    if (i < 0) continue;
    const name = pair.slice(0, i).trim();
    const gone =
      attrs.some((a) => /^\s*max-age\s*=\s*(0|-)/i.test(a)) ||
      attrs.some((a) => /^\s*expires\s*=/i.test(a) && Date.parse(a.split("=")[1]) < Date.now());
    if (gone) delete jar[name];
    else jar[name] = pair.slice(i + 1).trim();
  }
  void save("jar", jar);
}

async function tunnel(req: Request, navigate: boolean): Promise<Response> {
  const url = new URL(req.url);
  const headers: [string, string][] = [];
  req.headers.forEach((v, k) => headers.push([k, v]));
  if (req.referrer && req.referrer !== "about:client") headers.push(["referer", req.referrer]);
  // A worker's requests show it no Sec-Fetch-*: say what this one is.
  headers.push(["sec-fetch-mode", req.mode]);
  if (req.destination) headers.push(["sec-fetch-dest", req.destination]);
  const cookie = Object.entries(await cookies())
    .map(([k, v]) => `${k}=${v}`)
    .join("; ");
  if (cookie) headers.push(["cookie", cookie]);
  const body = req.method === "GET" || req.method === "HEAD" ? new Uint8Array() : new Uint8Array(await req.arrayBuffer());
  let res;
  for (let attempt = 0; ; attempt++) {
    const ch = await channel();
    try {
      res = await ch.fetch(req.method, url.pathname + url.search, headers, body);
      break;
    } catch (e) {
      // A channel that died under us (a stopped daemon, a dropped relay):
      // once more on a new one.
      if (attempt > 0 || !ch.closed) throw e;
    }
  }
  const out = new Headers();
  const setCookie: string[] = [];
  let encoding = "";
  for (const [k, v] of res.head.headers) {
    if (k === "set-cookie") setCookie.push(v);
    else if (k === "content-encoding") encoding = v;
    else if (k !== "content-length") {
      try {
        out.append(k, v);
      } catch {}
    }
  }
  keep(setCookie);
  // The daemon asks for identity; a server that compresses anyway is
  // undone here, since a built response's body is taken as it is.
  let stream: ReadableStream<Uint8Array> = res.body;
  if (encoding === "gzip" || encoding === "deflate") stream = stream.pipeThrough(new DecompressionStream(encoding) as unknown as TransformStream<Uint8Array, Uint8Array>);
  const status = res.head.status;
  if (NULL_BODY.has(status) || req.method === "HEAD") {
    void stream.cancel().catch(() => {});
    return new Response(null, { status: status === 101 ? 502 : status, headers: out });
  }
  if (navigate && (out.get("content-type") ?? "").startsWith("text/html")) {
    const page = await new Response(stream).text();
    const at = page.search(/<head[\s>]/i);
    const end = at < 0 ? 0 : page.indexOf(">", at) + 1;
    return new Response(page.slice(0, end) + SHIM + page.slice(end), { status, headers: out });
  }
  return new Response(stream, { status, headers: out });
}

// ---- an app's own worker ------------------------------------------------
//
// A page in the block may register a worker of its own (VS Code's webviews
// do). That script's fetch never reaches a worker: it goes to control,
// which doesn't have it. So control answers with this worker and the app's
// path in ARUGULA_APP_SW, and this worker fetches the app's script through
// the channel and runs it inside, handing it the events: its fetch handlers
// go first, and what they leave goes through the channel as usual.

const APP = (self as unknown as { ARUGULA_APP_SW?: string }).ARUGULA_APP_SW;
type Handler = (e: unknown) => void;
const appHandlers: Record<string, Handler[]> = {};
let app: Promise<void> | null = null;

function loadApp(): Promise<void> {
  app ??= (async () => {
    const res = await tunnel(new Request(new URL(APP!, sw.location.href)), false);
    if (!res.ok) throw new Error(`the app's worker: ${res.status}`);
    const code = await res.text();
    const add = (type: string, fn: Handler) => (appHandlers[type] ??= []).push(fn);
    const scope = new Proxy(self, {
      get(t, k) {
        if (k === "addEventListener") return add;
        const v = Reflect.get(t, k);
        return typeof v === "function" ? v.bind(t) : v;
      },
    });
    new Function("self", "addEventListener", code)(scope, add);
  })();
  return app;
}

function dispatchApp(type: string, e: unknown) {
  for (const h of appHandlers[type] ?? []) h(e);
}

if (APP) {
  for (const type of ["install", "activate"] as const) {
    sw.addEventListener(type, (e: Waiting) => {
      const waits: Promise<unknown>[] = [];
      e.waitUntil(
        loadApp()
          .then(() => dispatchApp(type, { waitUntil: (p: Promise<unknown>) => waits.push(p) }))
          .then(() => Promise.all(waits)),
      );
    });
  }
  sw.addEventListener("message", (e: MessageEvent_) => {
    if ((e.data as { arugula?: string } | null)?.arugula === TAG) return;
    e.waitUntil(loadApp().then(() => dispatchApp("message", e)));
  });
}

async function viaApp(e: FetchEvent): Promise<Response | undefined> {
  await loadApp();
  let answer: Promise<Response> | undefined;
  dispatchApp("fetch", {
    request: e.request,
    clientId: e.clientId,
    resultingClientId: e.resultingClientId,
    respondWith: (r: Response | Promise<Response>) => (answer = Promise.resolve(r)),
    waitUntil: (p: Promise<unknown>) => e.waitUntil(p),
  });
  return answer;
}

const failed = (err: unknown) => new Response(`Arugula: ${String(err)}`, { status: 502, headers: { "content-type": "text/plain" } });

sw.addEventListener("fetch", (e: FetchEvent) => {
  const url = new URL(e.request.url);
  const ours = url.pathname.startsWith("/.arugula/");
  if (APP && !ours) {
    e.respondWith(
      (async () => {
        const r = await viaApp(e).catch(() => undefined);
        if (r) return r;
        if (url.origin !== sw.location.origin) return fetch(e.request);
        try {
          return await tunnel(e.request, e.request.mode === "navigate");
        } catch (err) {
          return failed(err);
        }
      })(),
    );
    return;
  }
  if (url.origin !== sw.location.origin || ours) return;
  const navigate = e.request.mode === "navigate";
  e.respondWith(
    (async () => {
      if (!(await getHeld())) {
        // No key here (storage cleared): control's bootstrap page gets one.
        return navigate ? fetch(e.request) : new Response("Arugula: this block has no key", { status: 503 });
      }
      try {
        return await tunnel(e.request, navigate);
      } catch (err) {
        return failed(err);
      }
    })(),
  );
});
