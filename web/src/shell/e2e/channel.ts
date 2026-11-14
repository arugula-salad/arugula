// An end-to-end channel to a daemon over a WebSocket (direct, or through
// control's relay): the browser's copy of `arugula_e2e::channel`.
//
// Inside it travel the WebSocket protocol's text and binary messages and
// HTTP requests, so a page reached through the relay works the same as
// one on the tailnet. WebCrypto is async, so sealing and opening each run
// in a strict queue: nonces must go out (and be read) in order.

import { cat, deviceKey, Initiator, type Cipher } from "./noise.ts";
import type { DeviceKeys } from "./keys.ts";
import { tokenHello, TOKEN_REFUSED, type MachineTokens } from "./machine-token.ts";
import { unhex } from "./cert.ts";
import { fullReason, RelayMux, type SocketLike, type WakeSaid } from "./relaymux.ts";

const CHUNK = 16 * 1024;
const MAX_MSG = 64 << 20;
const enc = new TextEncoder();
const dec = new TextDecoder();

export interface RequestHead {
  method: string;
  path: string;
  content_type?: string;
  /** The answer may come in parts (channel.rs); the page never asks. */
  stream?: boolean;
}
export interface ResponseHead {
  status: number;
  content_type?: string;
  /** More parts follow: only for a request with `stream`. */
  more?: boolean;
}

export type Msg =
  | { kind: "text"; text: string }
  | { kind: "binary"; data: Uint8Array }
  | { kind: "request"; id: number; head: RequestHead; body: Uint8Array }
  | { kind: "response"; id: number; head: ResponseHead; body: Uint8Array };

export function encodeMsg(m: Msg): Uint8Array {
  const headed = (k: string, id: number, head: unknown, body: Uint8Array) => {
    const h = enc.encode(JSON.stringify(head));
    const pre = new Uint8Array(9);
    pre[0] = k.charCodeAt(0);
    const dv = new DataView(pre.buffer);
    dv.setUint32(1, id);
    dv.setUint32(5, h.length);
    return cat(pre, h, body);
  };
  switch (m.kind) {
    case "text":
      return cat(enc.encode("T"), enc.encode(m.text));
    case "binary":
      return cat(enc.encode("B"), m.data);
    case "request":
      return headed("Q", m.id, m.head, m.body);
    case "response":
      return headed("R", m.id, m.head, m.body);
  }
}

export function decodeMsg(b: Uint8Array): Msg {
  const kind = String.fromCharCode(b[0]);
  const rest = b.subarray(1);
  if (kind === "T") return { kind: "text", text: dec.decode(rest) };
  if (kind === "B") return { kind: "binary", data: rest };
  if (kind === "Q" || kind === "R") {
    const dv = new DataView(rest.buffer, rest.byteOffset, rest.byteLength);
    const id = dv.getUint32(0);
    const n = dv.getUint32(4);
    const head = JSON.parse(dec.decode(rest.subarray(8, 8 + n)));
    const body = rest.subarray(8 + n);
    return kind === "Q" ? { kind: "request", id, head, body } : { kind: "response", id, head, body };
  }
  throw new Error(`unknown message kind ${kind}`);
}

export interface DaemonRef {
  /** Its device id (in the prologue). */
  id: string;
  /** Its Noise public key, hex: from a certificate this browser checked,
   * or (with machine tokens, aru-141) as control's directory gives it. */
  noise: string;
}

/** What a channel opens with: this browser's device keys, or machine
 * tokens from control (aru-141), a new token and a key of its own for each
 * try. */
export type Credential = DeviceKeys | MachineTokens;

export const usesTokens = (c: Credential): c is MachineTokens => typeof (c as MachineTokens).token === "function";

/** Whether a device should reach the machine with its key after trying
 * tokens first: control gives no tokens at all (one without an issuer key
 * answers 404, as a dev or test control does), or the machine's daemon is
 * from before tokens. Anything else (signed out, not let in, a refused
 * token) the key wouldn't fix. */
export function keyAfterTokens(e: unknown): boolean {
  const err = e as { noToken?: boolean; status?: number; byDaemon?: boolean; code?: string };
  return (err.noToken === true && err.status === 404) || (err.byDaemon === true && err.code === "malformed");
}

/** Open a channel with `tokens` when given, and with this device's `keys`
 * if tokens can't reach it ([`keyAfterTokens`]) or there are none. */
export async function connectWith(urls: Route[], daemon: DaemonRef, tokens: MachineTokens | undefined, keys: DeviceKeys | undefined): Promise<E2ESocket> {
  if (tokens) {
    try {
      return await E2ESocket.connect(urls, daemon, tokens);
    } catch (e) {
      if (keys && keyAfterTokens(e)) return E2ESocket.connect(urls, daemon, keys);
      throw e;
    }
  }
  if (keys) return E2ESocket.connect(urls, daemon, keys);
  throw new Error("no device key and no token to reach it with");
}

export interface Response {
  status: number;
  ok: boolean;
  contentType?: string;
  body: Uint8Array;
  json<T = unknown>(): T;
  text(): string;
}

/** A way to a daemon: a WebSocket URL, or (`mux`) a channel inside the
 * page's one socket to control's relay (M25). */
export interface Route {
  url: string;
  timeoutMs: number;
  /** `wss://control…/api/relay/m`: reach the daemon through it, as a
   * channel, instead of a socket of its own at `url`. */
  mux?: string;
  /** Only if a `mux` route before it couldn't reach control at all (an
   * older control without the shared socket). */
  ifNoMux?: boolean;
  /** M182, on a `mux` route: a hosted machine that's asleep is woken, and
   * `onWake` hears what control says of its box. */
  wake?: boolean;
  onWake?: (w: WakeSaid) => void;
}

function openRoute(r: Route, id: string): Promise<SocketLike> {
  if (r.mux) return RelayMux.for(r.mux).open(id, r.timeoutMs, r.wake, r.onWake);
  return openSocket(r.url, r.timeoutMs) as Promise<unknown> as Promise<SocketLike>;
}

function openSocket(url: string, timeoutMs: number): Promise<WebSocket> {
  return new Promise((res, rej) => {
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    const t = setTimeout(() => {
      ws.close();
      rej(new Error(`timed out: ${url}`));
    }, timeoutMs);
    ws.onopen = () => {
      clearTimeout(t);
      res(ws);
    };
    ws.onerror = () => {
      clearTimeout(t);
      rej(new Error(`couldn't connect: ${url}`));
    };
  });
}

/** The daemon's clock, from message 2's payload (`{"now": ms}`, M50):
 * what block grants' expiry is minted from. */
export interface DaemonClock {
  /** Ms since the epoch, by the daemon's clock, when it answered. */
  now: number;
  /** `performance.now()` here when that answer was read. */
  at: number;
}

/** The daemon's now, estimated from `c`: its clock when it answered, plus
 * how long ago that was here (a monotonic clock, so this device's own
 * wall clock never comes into it). */
export function daemonNow(c: DaemonClock, perfNow = performance.now()): number {
  return Math.round(c.now + Math.max(0, perfNow - c.at));
}

/** What message 2's payload says: a clock (a device's channel), or a
 * refusal (`{"ok":false,"refused":…}`: its message, and its code, such as
 * `machine-token`). Anything else is nothing. */
export function readWelcome(payload: Uint8Array, at: number): { clock?: DaemonClock; refused?: string; code?: string } {
  if (!payload.length) return {};
  try {
    const j = JSON.parse(dec.decode(payload)) as { now?: unknown; ok?: unknown; refused?: unknown; message?: unknown };
    if (j.ok === false) {
      const code = typeof j.refused === "string" ? j.refused : undefined;
      return { refused: typeof j.message === "string" ? j.message : String(j.refused ?? "refused"), ...(code ? { code } : {}) };
    }
    if (typeof j.now === "number" && Number.isFinite(j.now) && j.now > 0) return { clock: { now: j.now, at } };
  } catch {
    // an older or odder daemon: no clock
  }
  return {};
}

/** Tokens asked for but never sent (message 1 didn't go: an offline
 * machine, a relay that couldn't reach it), by machine id, with when to stop
 * offering them (30 s before they expire). A reconnect takes this before
 * asking control for another: each try asked anew, and pages retrying
 * offline machines spent the account's tokens an hour (2026-11-14). */
const unspentTokens = new Map<string, { token: string; until: number }>();

/** A JWT's `exp`, in ms; none if it can't be read. */
function tokenExpiry(token: string): number | undefined {
  try {
    const b = token.split(".")[1].replace(/-/g, "+").replace(/_/g, "/");
    const exp = (JSON.parse(atob(b.padEnd(b.length + ((4 - (b.length % 4)) % 4), "="))) as { exp?: unknown }).exp;
    return typeof exp === "number" && Number.isFinite(exp) ? exp * 1000 : undefined;
  } catch {
    return undefined;
  }
}

/** Keep `token` for machine `id`'s next try. One that can't be read is
 * kept a minute. */
export function keepUnspent(id: string, token: string, now = Date.now()) {
  const exp = tokenExpiry(token);
  unspentTokens.set(id, { token, until: exp === undefined ? now + 60_000 : exp - 30_000 });
}

/** Machine `id`'s kept token, taken (so no other try sends it too). */
export function takeUnspent(id: string, now = Date.now()): string | null {
  const kept = unspentTokens.get(id);
  unspentTokens.delete(id);
  return kept && now < kept.until ? kept.token : null;
}

export class E2ESocket {
  onText: (t: string) => void = () => {};
  onBinary: (b: Uint8Array) => void = () => {};
  onClose: () => void = () => {};
  /** Which way it went: the first URL tried, or a later one (the relay). */
  readonly url: string;
  /** #369: why it closed, once it has. */
  why = "";
  /** #369: a piece of a message arrived (any piece: a big message comes
   * in many, and only the last one delivers it). */
  onWire: () => void = () => {};
  /** The daemon's clock as it answered the handshake (M50); none from a
   * daemon from before it said. */
  clock: DaemonClock | undefined;

  private sendQ: Promise<void> = Promise.resolve();
  private recvQ: Promise<void> = Promise.resolve();
  private partial: Uint8Array[] = [];
  private partialLen = 0;
  private nextId = 1;
  private pending = new Map<number, { res: (r: Response) => void; rej: (e: Error) => void }>();
  private closed = false;

  private ws: SocketLike;
  private send_: Cipher;
  private recv: Cipher;

  private constructor(ws: SocketLike, send: Cipher, recv: Cipher, early: Uint8Array[]) {
    this.ws = ws;
    this.send_ = send;
    this.recv = recv;
    this.url = ws.url;
    const take = (wire: Uint8Array) => {
      this.recvQ = this.recvQ.then(() => this.take(wire)).catch((e: Error) => this.close(`couldn't read: ${e.message}`));
    };
    // What the daemon sent right after its handshake message (its hello)
    // arrived while we were still finishing ours.
    for (const w of early) take(w);
    ws.onmessage = (e) => {
      this.onWire();
      take(new Uint8Array(e.data as ArrayBuffer));
    };
    // A relay channel says why control closed it (`reason`).
    ws.onclose = () => this.close((ws as { reason?: string }).reason ? `relay closed it: ${(ws as { reason?: string }).reason}` : "socket closed");
  }

  /** Try each URL in order (direct ones first, the relay last). With
   * machine tokens (aru-141), a try that sends message 1 spends its token
   * (the daemon takes each once), so the next try asks control for
   * another; one that never got that far leaves its token for the next,
   * and for the next connect to that machine (`keepUnspent`). A
   * token the daemon refused (expired, already used) is replaced and tried
   * once more on the same way. */
  static async connect(urls: Route[], daemon: DaemonRef, cred: Credential): Promise<E2ESocket> {
    let last: unknown = new Error("no way to reach it");
    let refused = false;
    const tokens = usesTokens(cred) ? cred : null;
    let unspent: string | null = tokens ? takeUnspent(daemon.id) : null;
    try {
      for (const route of urls) {
        if (route.ifNoMux && refused) continue;
        for (let tries = 1; ; tries++) {
          let sent = false;
          try {
            // Asked for before the way is opened: without one, no way would
            // do. A failure says `noToken`.
            const token = tokens ? (unspent ??= await tokens.token(daemon.id)) : null;
            const ws = await openRoute(route, daemon.id);
            try {
              // The daemon ignores a token channel's static key: one made for
              // it, so nothing ties this channel to another.
              const s = token === null ? (cred as DeviceKeys).noise : await deviceKey();
              const payload = token === null ? new Uint8Array() : tokenHello(token);
              return await E2ESocket.handshake(ws, daemon, s, payload, route.timeoutMs, () => {
                sent = true;
                unspent = null;
              });
            } catch (e) {
              ws.close();
              throw e;
            }
          } catch (e) {
            last = e;
            const err = e as { byDaemon?: boolean; code?: string; refused?: unknown; noToken?: boolean };
            // Control gave no token (signed out, or not let in to it).
            if (err.noToken) throw e;
            // The daemon refused the token: a new one, once, the same way.
            if (tokens && sent && err.byDaemon && err.code === TOKEN_REFUSED && tries < 2) continue;
            // The daemon itself said no (#913: it doesn't trust this device
            // yet, say). Every way reaches the same daemon, so another one
            // would only hear the same, or, a moment later, be let in on a
            // worse way than the first and stay there: try again later, from
            // the first.
            if (err.byDaemon) throw e;
            if (route.mux && err.refused) refused = true;
            break;
          }
        }
      }
      throw last;
    } finally {
      // Never sent: the next try (this page's next reconnect) sends it.
      if (unspent) keepUnspent(daemon.id, unspent);
    }
  }

  /** Noise IK with static key `s`, `payload` in message 1 (a token's
   * hello, or nothing for a device); `sent` hears that message 1 went. */
  private static handshake(ws: SocketLike, daemon: DaemonRef, s: CryptoKeyPair, payload: Uint8Array, timeoutMs: number, sent: () => void): Promise<E2ESocket> {
    const token = payload.length > 0;
    return new Promise((res, rej) => {
      const ik = new Initiator(s, unhex(daemon.noise));
      const t = setTimeout(() => rej(new Error("handshake timed out")), Math.max(timeoutMs, 5000));
      // Every message from the first on is kept: the daemon's first
      // transport message can arrive before we've finished reading its
      // handshake reply, and dropping it would skip a nonce.
      const early: Uint8Array[] = [];
      let first = true;
      // Message 2 is read before a close counts: a refusal can come with
      // the close (M50), and the read is async.
      let reading: Promise<void> = Promise.resolve();
      ws.onmessage = (e) => {
        const wire = new Uint8Array(e.data as ArrayBuffer);
        if (!first) return void early.push(wire);
        first = false;
        reading = (async () => {
          try {
            const { payload, channel } = await ik.read(wire);
            clearTimeout(t);
            const w = readWelcome(payload, performance.now());
            // A daemon from before tokens takes the hello for a block's
            // grant, and finds it malformed.
            const why = token && w.code === "malformed" ? "its Arugula is too old to take a token from Arugula cloud: update it" : w.refused;
            if (w.refused) return rej(Object.assign(new Error(`the machine refused: ${why}`), { refused: w.refused, code: w.code, byDaemon: true }));
            const sock = new E2ESocket(ws, channel.send, channel.recv, early);
            sock.clock = w.clock;
            res(sock);
          } catch (err) {
            clearTimeout(t);
            rej(err);
          }
        })();
      };
      ws.onclose = (e) => {
        const full = fullReason(e);
        void reading.then(() => {
          clearTimeout(t);
          if (full) rej(Object.assign(new Error(full), { full: true }));
          else rej(new Error(token ? "closed during the handshake" : "closed during the handshake (not an approved device?)"));
        });
      };
      void ik
        // Frozen (#504): the prologue both ends agree on.
        .write(payload, enc.encode(`illogical/1\n${daemon.id}\n`))
        .then((m) => {
          ws.send(m);
          sent();
        })
        .catch(rej);
    });
  }

  get open(): boolean {
    return !this.closed && this.ws.readyState === WebSocket.OPEN;
  }

  /** #369: what the way to the daemon looks like, for a link that's
   * dropped: how much of a message has arrived, and the socket's state. */
  health(): string {
    const ws = this.ws as SocketLike & { bufferedAmount?: number; health?(): string };
    const parts = [`${this.partialLen} B of a message in`];
    if (ws.health) parts.push(ws.health());
    else parts.push(`socket state ${ws.readyState}, ${ws.bufferedAmount ?? 0} B unsent`);
    return parts.join(", ");
  }

  private async take(wire: Uint8Array) {
    const plain = await this.recv.open(wire);
    const more = plain[0] !== 0;
    this.partial.push(plain.subarray(1));
    this.partialLen += plain.length - 1;
    if (this.partialLen > MAX_MSG) throw new Error("message too large");
    if (more) return;
    const whole = this.partial.length === 1 ? this.partial[0] : cat(...this.partial);
    this.partial = [];
    this.partialLen = 0;
    const m = decodeMsg(whole);
    if (m.kind === "text" || m.kind === "binary") {
      if (this.held) this.held.push(m);
      else this.deliver(m);
    } else if (m.kind === "response") {
      const done = this.pending.get(m.id);
      this.pending.delete(m.id);
      const body = m.body;
      done?.res({
        status: m.head.status,
        ok: m.head.status >= 200 && m.head.status < 300,
        contentType: m.head.content_type,
        body,
        json: () => JSON.parse(dec.decode(body)),
        text: () => dec.decode(body),
      });
    }
  }

  /** Protocol messages that came before `start`. */
  private held: Msg[] | null = [];

  private deliver(m: Msg) {
    if (m.kind === "text") this.onText(m.text);
    else if (m.kind === "binary") this.onBinary(m.data);
  }

  /** Set `onText` and `onBinary`, then call this: messages that arrived
   * before (the daemon's hello) are delivered now, in order. Requests work
   * without it. */
  start() {
    const held = this.held ?? [];
    this.held = null;
    for (const m of held) this.deliver(m);
  }

  private put(m: Msg) {
    if (!this.open) return;
    const plain = encodeMsg(m);
    this.sendQ = this.sendQ
      .then(async () => {
        for (let o = 0; o < plain.length || o === 0; o += CHUNK) {
          const last = o + CHUNK >= plain.length;
          const chunk = cat(Uint8Array.of(last ? 0 : 1), plain.subarray(o, o + CHUNK));
          const wire = await this.send_.seal(chunk);
          if (this.ws.readyState === WebSocket.OPEN) this.ws.send(wire);
          if (last) break;
        }
      })
      .catch((e: Error) => this.close(`couldn't send: ${e.message}`));
  }

  sendText(text: string) {
    this.put({ kind: "text", text });
  }

  sendBinary(data: Uint8Array) {
    this.put({ kind: "binary", data });
  }

  /** An HTTP request to the daemon's API, through the channel. */
  request(method: string, path: string, body?: unknown, timeoutMs = 30_000): Promise<Response> {
    const id = this.nextId++;
    // Raw bytes (an upload's chunk, M70) go as they are.
    const raw = body instanceof Uint8Array ? body : undefined;
    const json = body === undefined || raw ? undefined : JSON.stringify(body);
    return new Promise((res, rej) => {
      if (!this.open) return rej(new Error("not connected"));
      const t = setTimeout(() => {
        this.pending.delete(id);
        rej(new Error("request timed out"));
      }, timeoutMs);
      this.pending.set(id, {
        res: (r) => {
          clearTimeout(t);
          res(r);
        },
        rej: (e) => {
          clearTimeout(t);
          rej(e);
        },
      });
      this.put({
        kind: "request",
        id,
        head: {
          method,
          path,
          ...(raw ? { content_type: "application/octet-stream" } : json === undefined ? {} : { content_type: "application/json" }),
        },
        body: raw ?? (json === undefined ? new Uint8Array() : enc.encode(json)),
      });
    });
  }

  close(why = "closed here") {
    if (this.closed) return;
    this.closed = true;
    this.why = why;
    this.ws.close();
    for (const p of this.pending.values()) p.rej(new Error("connection closed"));
    this.pending.clear();
    this.onClose();
  }
}
