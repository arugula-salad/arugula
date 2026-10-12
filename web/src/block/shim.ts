// The WebSocket shim (M50; S27's web/shim.ts), first in every page of a
// block (the worker puts it there). A worker can't carry WebSockets, so a
// socket to the block's own host goes over a Noise channel this page opens
// itself; any other socket is a real one. The page's channel, not the
// worker's: the browser stops an idle worker after about 30 s, which would
// cut a hot-reload socket that sits quiet for minutes.
//
// It also relays renewals: when the worker's grant has expired it asks its
// pages, and a page asks the parent. And it tells the parent which way the
// worker's channel went (`route`), which the block shows.
//
// Built by itself into dist/block/shim.js (vite.block.config.ts) and served
// by control at /.arugula/shim.js.

import { BlockChannel, type Held, load, NativeWebSocket, openChannel, Refused, save } from "./channel.ts";
import { askParent, giveWorker, tellRoute, TAG, VERSION } from "./page.ts";

let chan: Promise<BlockChannel> | null = null;

async function renew(parent: string): Promise<Held> {
  const h = await askParent([parent], "renew");
  await save("held", h);
  await giveWorker(h);
  return h;
}

navigator.serviceWorker?.addEventListener("message", async (e) => {
  const m = e.data as { arugula?: string; v?: number; ask?: string; route?: string } | null;
  if (m?.arugula !== TAG || m.v !== VERSION) return;
  const h = await load<Held>("held");
  if (!h) return;
  if (m.route === "direct" || m.route === "relayed") tellRoute(h.parent, m.route);
  else if (m.ask === "renew") await renew(h.parent).catch(() => {});
});
// The worker opened its channel for this page's own request, before this
// page could hear it say so: ask.
navigator.serviceWorker?.controller?.postMessage({ arugula: TAG, v: VERSION, ask: "route" });

async function connect(): Promise<BlockChannel> {
  let h = await load<Held>("held");
  if (!h) throw new Error("this block has no key");
  try {
    return await openChannel(h.creds, location);
  } catch (e) {
    if (!(e instanceof Refused && e.code === "expired")) throw e;
    h = await renew(h.parent);
    return openChannel(h.creds, location);
  }
}

function channel(): Promise<BlockChannel> {
  if (!chan) {
    const p = connect().then((ch) => {
      ch.onclose = () => {
        if (chan === p) chan = null;
      };
      return ch;
    });
    p.catch(() => {
      if (chan === p) chan = null;
    });
    chan = p;
  }
  return chan;
}

const enc = new TextEncoder();
const dec = new TextDecoder();

class ShimSocket extends EventTarget {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  readonly CONNECTING = 0;
  readonly OPEN = 1;
  readonly CLOSING = 2;
  readonly CLOSED = 3;
  readyState = 0;
  protocol = "";
  extensions = "";
  bufferedAmount = 0;
  binaryType: BinaryType = "blob";
  readonly url: string;
  onopen: ((e: Event) => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  onclose: ((e: CloseEvent) => void) | null = null;
  onerror: ((e: Event) => void) | null = null;
  private h: ReturnType<BlockChannel["websocket"]> | null = null;
  private sendQ: Promise<void> = Promise.resolve();

  constructor(url: URL, protocols: string[]) {
    super();
    this.url = url.href;
    channel()
      .then((ch) => {
        if (this.readyState === 3) return;
        this.h = ch.websocket(url.pathname + url.search, protocols, {
          onOpen: (p) => {
            this.protocol = p;
            this.readyState = 1;
            this.fire(new Event("open"));
          },
          onMessage: (text, data) => {
            const d = text ? dec.decode(data) : this.binaryType === "arraybuffer" ? data.buffer : new Blob([data as Uint8Array<ArrayBuffer>]);
            this.fire(new MessageEvent("message", { data: d, origin: location.origin }));
          },
          onClose: (code, reason) => this.closed(code, reason),
        });
      })
      .catch(() => this.closed(1006, ""));
  }

  private fire(e: Event) {
    this.dispatchEvent(e);
    const f = (this as unknown as Record<string, ((e: Event) => void) | null>)[`on${e.type}`];
    f?.call(this, e);
  }

  private closed(code: number, reason: string) {
    if (this.readyState === 3) return;
    const was = this.readyState;
    this.readyState = 3;
    if (code === 1006 || was === 0) this.fire(new Event("error"));
    this.fire(new CloseEvent("close", { code, reason, wasClean: code !== 1006 }));
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
    if (this.readyState === 0) throw new DOMException("Still in CONNECTING state.", "InvalidStateError");
    if (this.readyState !== 1 || !this.h) return;
    const h = this.h;
    this.sendQ = this.sendQ.then(async () => {
      if (typeof data === "string") h.send(true, enc.encode(data));
      else if (data instanceof Blob) h.send(false, new Uint8Array(await data.arrayBuffer()));
      else if (ArrayBuffer.isView(data)) h.send(false, new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
      else h.send(false, new Uint8Array(data as ArrayBuffer));
    });
  }

  close(code = 1000, reason = "") {
    if (this.readyState >= 2) return;
    this.readyState = 2;
    if (this.h) this.h.close(code, reason);
    else this.closed(code, reason);
  }
}

window.WebSocket = new Proxy(NativeWebSocket, {
  construct(target, args: [string | URL, (string | string[])?]) {
    const u = new URL(args[0], location.href);
    if ((u.protocol === "ws:" || u.protocol === "wss:") && u.host === location.host) {
      const p = args[1];
      return new ShimSocket(u, p === undefined ? [] : typeof p === "string" ? [p] : p) as unknown as WebSocket;
    }
    return new target(...(args as [string, string[]]));
  },
});
