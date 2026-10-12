// A block's Noise channel to its daemon (M50, #150; ported from S27's
// spikes/s27-blocks/web/chan.ts), and the streams inside it. Used by the
// block's worker (HTTP) and by the WebSocket shim in its pages, on the
// block's own origin, `b-<site key>.<block domain>`, which control serves
// (crates/control/src/blocks.rs).
//
// The handshake is the product's (Noise IK, ../e2e/noise.ts, the frozen
// prologue `illogical/1\n<daemon id>\n`) with the block's one-off key as
// the static key, and the grant in message 1's payload (`{"grant": …}`,
// crates/e2e/src/block.rs). The daemon answers in message 2's payload:
// `{"ok":true}`, or `{"ok":false,"refused":"<code>"}` and a close. Then
// every transport message is the product's framing (../e2e/channel.ts: a
// continuation byte, then a binary `Msg`), each carrying one frame of
// `arugula_e2e::block::wire`: `kind ‖ u32 stream ‖ payload`.

import { cat, Initiator, type Cipher } from "../e2e/noise.ts";
import { unhex } from "../e2e/cert.ts";
import { decodeMsg, encodeMsg } from "../e2e/channel.ts";

const CHUNK = 16 * 1024;
const MAX_MSG = 64 << 20;
const enc = new TextEncoder();
const dec = new TextDecoder();

/** `arugula_e2e::block::Grant`, as the parent page signed it. */
export interface Grant {
  daemon: string;
  block: string;
  key: string;
  /** Ms since the epoch, by the daemon's clock. */
  expires: number;
  by: string;
  sig: string;
}

/** What the parent page hands a block (the postMessage contract in
 * docs/control-e2e.md, "The frame's contract with control's page"). */
export interface Creds {
  /** The daemon: its device id and its Noise key (hex), from a certificate
   * the parent checked. */
  daemon: { id: string; noise: string };
  /** The one-off X25519 block key: its PKCS#8 private half and its public
   * half (hex, the grant's `key`). */
  key: { pkcs8: ArrayBuffer; pub: string };
  grant: Grant;
  /** "relay" (control's relay on this origin), or a `wss://…/e2e` URL
   * straight to the daemon. */
  route: string;
}

/** What the block keeps: the credentials and the parent page's origin,
 * where renewals are asked for. */
export interface Held {
  creds: Creds;
  parent: string;
}

export interface ResHead {
  status: number;
  headers: [string, string][];
}

export interface WsHandlers {
  onOpen(protocol: string): void;
  onMessage(text: boolean, data: Uint8Array): void;
  onClose(code: number, reason: string): void;
}

/** The daemon refused the grant: `code` is `arugula_e2e::block::Refused`'s
 * (`expired`: ask for a new one). */
export class Refused extends Error {
  constructor(readonly code: string) {
    super(`the machine refused this block (${code})`);
  }
}

export const NativeWebSocket: typeof WebSocket = globalThis.WebSocket;

function frame(kind: string, sid: number, payload: Uint8Array = new Uint8Array()): Uint8Array {
  const f = new Uint8Array(5 + payload.length);
  f[0] = kind.charCodeAt(0);
  new DataView(f.buffer).setUint32(1, sid);
  f.set(payload, 5);
  return f;
}

/** Where the channel goes: control's relay on this block's own origin, or
 * the daemon directly. */
export function channelUrl(creds: Creds, location: { protocol: string; host: string }): string {
  if (creds.route !== "relay") return creds.route;
  const ws = location.protocol === "http:" ? "ws:" : "wss:";
  return `${ws}//${location.host}/.arugula/relay?d=${encodeURIComponent(creds.daemon.id)}`;
}

interface Handler {
  frame(kind: string, payload: Uint8Array): void;
  fail(why: string): void;
}

export class BlockChannel {
  onclose: () => void = () => {};
  closed = false;
  private sendQ: Promise<void> = Promise.resolve();
  private recvQ: Promise<void> = Promise.resolve();
  private partial: Uint8Array[] = [];
  private partialLen = 0;
  private handlers = new Map<number, Handler>();
  private next = 1;
  private ws!: WebSocket;
  private tx!: Cipher;
  private rx!: Cipher;

  static async connect(url: string, creds: Creds, timeoutMs = 10_000): Promise<BlockChannel> {
    const c = new BlockChannel();
    const priv = await crypto.subtle.importKey("pkcs8", creds.key.pkcs8, { name: "X25519" }, false, ["deriveBits"]);
    const pub = await crypto.subtle.importKey("raw", unhex(creds.key.pub), { name: "X25519" }, true, []);
    const ik = new Initiator({ privateKey: priv, publicKey: pub }, unhex(creds.daemon.noise));
    const ws = new NativeWebSocket(url);
    ws.binaryType = "arraybuffer";
    c.ws = ws;
    await new Promise<void>((res, rej) => {
      const t = setTimeout(() => {
        ws.close();
        rej(new Error(`timed out: ${url}`));
      }, timeoutMs);
      let first = true;
      ws.onopen = async () => {
        try {
          // Frozen (#504): the prologue both ends agree on.
          ws.send(await ik.write(enc.encode(JSON.stringify({ grant: creds.grant })), enc.encode(`illogical/1\n${creds.daemon.id}\n`)));
        } catch (e) {
          rej(e);
        }
      };
      ws.onerror = () => {
        clearTimeout(t);
        rej(new Error(`couldn't connect: ${url}`));
      };
      // A refusal can still be in the receive queue when the close comes
      // (S27 saw it in WebKit): read it first.
      ws.onclose = () =>
        void c.recvQ.then(() => {
          clearTimeout(t);
          rej(new Error("closed during the handshake"));
        });
      ws.onmessage = (e) => {
        const wire = new Uint8Array(e.data as ArrayBuffer);
        if (!first) return c.take(wire);
        first = false;
        c.recvQ = c.recvQ.then(async () => {
          const { payload, channel } = await ik.read(wire);
          c.tx = channel.send;
          c.rx = channel.recv;
          clearTimeout(t);
          const a = JSON.parse(dec.decode(payload)) as { ok?: boolean; refused?: string };
          if (a.ok) res();
          else rej(new Refused(a.refused ?? "malformed"));
        });
        c.recvQ.catch(rej);
      };
    }).catch((e) => {
      ws.close();
      throw e;
    });
    ws.onclose = () => c.close();
    ws.onerror = () => c.close();
    return c;
  }

  private take(wire: Uint8Array) {
    this.recvQ = this.recvQ
      .then(async () => {
        const plain = await this.rx.open(wire);
        this.partial.push(plain.subarray(1));
        this.partialLen += plain.length - 1;
        if (this.partialLen > MAX_MSG) throw new Error("message too large");
        if (plain[0] !== 0) return;
        const whole = this.partial.length === 1 ? this.partial[0] : cat(...this.partial);
        this.partial = [];
        this.partialLen = 0;
        const m = decodeMsg(whole);
        if (m.kind !== "binary" || m.data.length < 5) return;
        const f = m.data;
        const sid = new DataView(f.buffer, f.byteOffset).getUint32(1);
        this.handlers.get(sid)?.frame(String.fromCharCode(f[0]), f.subarray(5));
      })
      .catch(() => this.close());
  }

  private send(f: Uint8Array) {
    const plain = encodeMsg({ kind: "binary", data: f });
    this.sendQ = this.sendQ
      .then(async () => {
        for (let o = 0; ; o += CHUNK) {
          const last = o + CHUNK >= plain.length;
          const wire = await this.tx.seal(cat(Uint8Array.of(last ? 0 : 1), plain.subarray(o, o + CHUNK)));
          if (this.ws.readyState === NativeWebSocket.OPEN) this.ws.send(wire);
          if (last) break;
        }
      })
      .catch(() => this.close());
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    try {
      this.ws.close();
    } catch {}
    for (const h of this.handlers.values()) h.fail("the channel closed");
    this.handlers.clear();
    this.onclose();
  }

  /** An HTTP request; resolves at the response's head, its body streams. */
  fetch(method: string, path: string, headers: [string, string][], body: Uint8Array): Promise<{ head: ResHead; body: ReadableStream<Uint8Array> }> {
    const sid = this.next++;
    return new Promise((res, rej) => {
      if (this.closed) return rej(new Error("the channel closed"));
      let ctl: ReadableStreamDefaultController<Uint8Array>;
      const stream = new ReadableStream<Uint8Array>({
        start: (c) => {
          ctl = c;
        },
        cancel: () => {
          this.handlers.delete(sid);
          this.send(frame("K", sid));
        },
      });
      let headed = false;
      this.handlers.set(sid, {
        frame: (k, p) => {
          if (k === "R") {
            headed = true;
            res({ head: JSON.parse(dec.decode(p)), body: stream });
          } else if (k === "D") ctl.enqueue(p.slice());
          else if (k === "E") {
            this.handlers.delete(sid);
            ctl.close();
          } else if (k === "X") {
            this.handlers.delete(sid);
            const why = (JSON.parse(dec.decode(p)) as { message: string }).message;
            if (headed) ctl.error(new Error(why));
            else rej(new Error(why));
          }
        },
        fail: (why) => {
          if (headed) ctl.error(new Error(why));
          else rej(new Error(why));
        },
      });
      this.send(frame("H", sid, enc.encode(JSON.stringify({ method, path, headers }))));
      for (let o = 0; o < body.length; o += 60 * 1024) this.send(frame("D", sid, body.subarray(o, o + 60 * 1024)));
      this.send(frame("E", sid));
    });
  }

  /** A WebSocket to the block's port. */
  websocket(path: string, protocols: string[], h: WsHandlers): { send(text: boolean, data: Uint8Array): void; close(code: number, reason: string): void } {
    const sid = this.next++;
    let done = false;
    const finish = (code: number, reason: string) => {
      if (done) return;
      done = true;
      this.handlers.delete(sid);
      h.onClose(code, reason);
    };
    this.handlers.set(sid, {
      frame: (k, p) => {
        if (k === "A") h.onOpen((JSON.parse(dec.decode(p)) as { protocol?: string }).protocol ?? "");
        else if (k === "M") h.onMessage(p[0] === 0, p.subarray(1).slice());
        else if (k === "C") {
          const c = JSON.parse(dec.decode(p)) as { code?: number; reason?: string };
          finish(c.code || 1005, c.reason || "");
        } else if (k === "X") finish(1006, (JSON.parse(dec.decode(p)) as { message: string }).message);
      },
      fail: (why) => finish(1006, why),
    });
    this.send(frame("O", sid, enc.encode(JSON.stringify({ path, protocols }))));
    return {
      send: (text, data) => this.send(frame("M", sid, cat(Uint8Array.of(text ? 0 : 1), data))),
      close: (code, reason) => {
        this.send(frame("C", sid, enc.encode(JSON.stringify({ code, reason }))));
        finish(code, reason);
      },
    };
  }
}

// ---- where the worker and the pages keep the block's credentials -------

let opened: Promise<IDBDatabase> | null = null;

/** One connection, let go when the database is being deleted (storage
 * cleared), so the deletion isn't held up by us. */
function db(): Promise<IDBDatabase> {
  opened ??= new Promise((res, rej) => {
    const r = indexedDB.open("arugula-block", 1);
    r.onupgradeneeded = () => r.result.createObjectStore("kv");
    r.onsuccess = () => {
      r.result.onversionchange = () => {
        r.result.close();
        opened = null;
      };
      res(r.result);
    };
    r.onerror = () => {
      opened = null;
      rej(r.error);
    };
  });
  return opened;
}

export async function load<T>(key: string): Promise<T | undefined> {
  const d = await db();
  return new Promise((res, rej) => {
    const r = d.transaction("kv").objectStore("kv").get(key);
    r.onsuccess = () => res(r.result as T | undefined);
    r.onerror = () => rej(r.error);
  });
}

export async function save(key: string, v: unknown): Promise<void> {
  const d = await db();
  return new Promise((res, rej) => {
    const t = d.transaction("kv", "readwrite");
    t.objectStore("kv").put(v, key);
    t.oncomplete = () => res();
    t.onerror = () => rej(t.error);
  });
}
