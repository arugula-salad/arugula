// Many daemons' channels over one WebSocket to control's relay (M25).
//
// Browsers space out WebSocket connections to one address past about eight
// (S16), and through the relay every daemon is the same address. So a page
// that reaches many daemons opens `/api/relay/m` once, and each daemon's
// Noise channel is a numbered channel inside it. Each binary message is
// `kind (1) ‖ channel (4, BE) ‖ payload` (see control's relay.rs). A
// channel looks enough like a WebSocket for E2ESocket to use it as one.

const OPEN = 1;
const DATA = 2;
const CLOSE = 3;
const OPENED = 4;
/** M182: as OPEN, and a hosted machine that's asleep is woken. */
const OPEN_WAKE = 5;
/** M182: what's become of a hosted machine's box, as JSON (WakeSaid). */
const WAKE = 6;

/** How long an open waits once control says it's waking the machine's
 * box: control gives up on it first (crates/control/src/wake.rs). */
export const WAKE_MS = 50_000;

/** What control says of a hosted machine's box (M182): `waking` as it
 * starts to wake it, then `failed` (`code`, `why`) or `asleep` (nobody
 * asked to wake it) before the channel's close. */
export interface WakeSaid {
  state: "waking" | "asleep" | "failed";
  /** `gone`, `refused`, `unavailable`, `starting`, `failed` or `silent`. */
  code?: string;
  why?: string;
}

/** The close code (try again later) control's relay closes a browser's
 * socket with when it's at its ceiling (#344), saying why. */
export const FULL = 1013;

/** Why control's relay is full, if this closed a socket for that. */
export function fullReason(e?: { code: number; reason: string }): string | undefined {
  return e?.code === FULL ? e.reason || "control is full: try again shortly" : undefined;
}

/** What E2ESocket needs of a socket. */
export interface SocketLike {
  binaryType: string;
  readonly url: string;
  readonly readyState: number;
  onmessage: ((e: { data: ArrayBuffer }) => void) | null;
  onclose: ((e?: { code: number; reason: string }) => void) | null;
  send(data: Uint8Array): void;
  close(): void;
}

function frame(kind: number, chan: number, payload: Uint8Array = new Uint8Array()): Uint8Array<ArrayBuffer> {
  const f = new Uint8Array(5 + payload.length);
  f[0] = kind;
  new DataView(f.buffer).setUint32(1, chan);
  f.set(payload, 5);
  return f;
}

class MuxChannel implements SocketLike {
  binaryType = "arraybuffer";
  readyState: number = WebSocket.CONNECTING;
  onmessage: ((e: { data: ArrayBuffer }) => void) | null = null;
  onclose: (() => void) | null = null;
  /** Why control closed it, if it did. */
  reason = "";
  /** Set when control's relay was full (#344). */
  full = false;
  /** M182: what control last said of a hosted machine's box. */
  wake: WakeSaid | undefined;
  private mux: RelayMux;
  readonly chan: number;
  readonly url: string;
  constructor(mux: RelayMux, chan: number, url: string) {
    this.mux = mux;
    this.chan = chan;
    this.url = url;
  }
  /** #369: the shared socket's state, which every relayed channel waits on. */
  health(): string {
    return this.mux.health();
  }
  send(data: Uint8Array) {
    if (this.readyState === WebSocket.OPEN) this.mux.write(frame(DATA, this.chan, data));
  }
  close() {
    if (this.readyState === WebSocket.CLOSED) return;
    if (this.readyState === WebSocket.OPEN) this.mux.write(frame(CLOSE, this.chan));
    this.mux.forget(this.chan);
    this.ended();
  }
  ended() {
    if (this.readyState === WebSocket.CLOSED) return;
    this.readyState = WebSocket.CLOSED;
    this.onclose?.();
  }
}

export class RelayMux {
  private static all = new Map<string, RelayMux>();
  /** The one for this relay URL (`wss://control…/api/relay/m`). */
  static for(url: string): RelayMux {
    let m = RelayMux.all.get(url);
    if (!m) RelayMux.all.set(url, (m = new RelayMux(url)));
    return m;
  }

  /** Drop every shared socket (a simulated sleep, for tests). */
  static closeAll() {
    for (const m of RelayMux.all.values()) m.ws?.close();
  }

  /** WebSockets this has opened (for tests: one, however many daemons). */
  sockets = 0;
  private ws: WebSocket | null = null;
  private ready: Promise<WebSocket> | null = null;
  private next = 1;
  private chans = new Map<number, { ch: MuxChannel; opened: (ok: boolean) => void; said?: (w: WakeSaid) => void }>();
  /** #369: when the shared socket last brought anything, for any channel. */
  private heardAt = 0;

  readonly url: string;
  private constructor(url: string) {
    this.url = url;
  }

  get channels(): number {
    return this.chans.size;
  }

  private socket(timeoutMs: number): Promise<WebSocket> {
    if (this.ws?.readyState === WebSocket.OPEN) return Promise.resolve(this.ws);
    if (this.ready) return this.ready;
    this.ready = new Promise<WebSocket>((res, rej) => {
      const ws = new WebSocket(this.url);
      this.sockets++;
      ws.binaryType = "arraybuffer";
      const t = setTimeout(() => {
        ws.close();
        rej(new Error(`timed out: ${this.url}`));
      }, timeoutMs);
      ws.onopen = () => {
        clearTimeout(t);
        this.ws = ws;
        res(ws);
      };
      ws.onerror = () => {
        clearTimeout(t);
        rej(new Error(`couldn't connect: ${this.url}`));
      };
      ws.onmessage = (e) => {
        this.heardAt = Date.now();
        this.take(new Uint8Array(e.data as ArrayBuffer));
      };
      ws.onclose = (e) => {
        if (this.ws === ws) this.ws = null;
        const full = fullReason(e);
        // Every channel inside it is gone too.
        for (const { ch, opened } of [...this.chans.values()]) {
          if (full) Object.assign(ch, { reason: full, full: true });
          opened(false);
          ch.ended();
        }
        this.chans.clear();
      };
    }).finally(() => {
      this.ready = null;
    });
    return this.ready;
  }

  private take(b: Uint8Array) {
    if (b.length < 5) return;
    const chan = new DataView(b.buffer, b.byteOffset).getUint32(1);
    const c = this.chans.get(chan);
    if (!c) return;
    const payload = b.subarray(5);
    if (b[0] === OPENED) {
      c.ch.readyState = WebSocket.OPEN;
      c.opened(true);
    } else if (b[0] === DATA) {
      c.ch.onmessage?.({ data: payload.slice().buffer });
    } else if (b[0] === WAKE) {
      let w: WakeSaid;
      try {
        w = JSON.parse(new TextDecoder().decode(payload)) as WakeSaid;
      } catch {
        return;
      }
      c.ch.wake = w;
      c.said?.(w);
    } else if (b[0] === CLOSE) {
      c.ch.reason = new TextDecoder().decode(payload);
      this.chans.delete(chan);
      c.opened(false);
      c.ch.ended();
    }
  }

  /** #369: the shared socket, for a dropped link's console line. */
  health(): string {
    const ws = this.ws;
    if (!ws) return "relay socket gone";
    const quiet = this.heardAt ? `${Date.now() - this.heardAt}ms` : "never";
    return `relay socket state ${ws.readyState}, quiet ${quiet}, ${ws.bufferedAmount} B unsent, ${this.chans.size} channels`;
  }

  write(f: Uint8Array<ArrayBuffer>) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(f);
  }

  forget(chan: number) {
    this.chans.delete(chan);
  }

  /** A channel to daemon `id`, once control has its stream up. `wake`
   * (M182): a hosted machine that's asleep is woken, and `said` hears
   * what control says of its box; the open waits longer while it wakes. */
  async open(id: string, timeoutMs: number, wake = false, said?: (w: WakeSaid) => void): Promise<SocketLike> {
    const ws = await this.socket(timeoutMs);
    const chan = this.next++;
    const ch = new MuxChannel(this, chan, `${this.url}#${id}`);
    return new Promise((res, rej) => {
      const giveUp = () => {
        ch.close();
        rej(new Error(`timed out: ${id} through ${this.url}`));
      };
      let t = setTimeout(giveUp, timeoutMs);
      this.chans.set(chan, {
        ch,
        opened: (ok) => {
          clearTimeout(t);
          if (ok) res(ch);
          // Control answered, but no channel: the shared socket works, so
          // a socket of its own wouldn't do better.
          else rej(Object.assign(new Error(ch.reason || `couldn't reach ${id}`), { refused: true, full: ch.full, wake: ch.wake }));
        },
        said: (w) => {
          if (w.state === "waking") {
            clearTimeout(t);
            t = setTimeout(giveUp, WAKE_MS);
          }
          said?.(w);
        },
      });
      ws.send(frame(wake ? OPEN_WAKE : OPEN, chan, new TextEncoder().encode(id)));
    });
  }
}
