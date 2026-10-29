// connect(): a room on arugula.stream (or any presence relay) for a page
// to show (../../sdk/API.md). A Room is a PresenceClient over the Phoenix
// transport, its changes as events (members, knock, answer, live), and
// mount/avatar to draw people into an element. Who's in which room is the
// token's business: your server signs it (or, to prototype, the relay's
// demo tokens).

import { PresenceClient, phoenixTransport, type Member, type Transport } from "@arugula/presence";
import { setAssetBase } from "@arugula/presence-core/assets";
import type { Look } from "@arugula/presence-core/avatar/rig";
import { mountAvatar, mountPeople, type AvatarOptions, type PeopleOptions } from "./view.ts";
import { CSS } from "./styles.ts";

export const RELAY = "wss://arugula.stream/socket";

export interface ConnectOptions {
  /** The room's id in your app. */
  room: string;
  /** A room token your server signed, or a function for a fresh one
   * (called on every (re)connect). */
  token: string | (() => Promise<string>);
  /** The relay's socket. Default: arugula.stream's. */
  relay?: string;
  /** Starting choices where this browser has none kept. */
  mode?: "avatar" | "outline";
  look?: Look;
  /** Where this browser keeps the person's choices (localStorage keys). */
  storage?: string;
  /** Where the trackers' models and Wasm are. Default: the relay's origin,
   * which serves them. */
  assets?: string;
  /** Tests: a transport of their own. */
  transport?: Transport;
}

export interface RoomEvents {
  members: (members: Member[]) => void;
  knock: (from: string) => void;
  answer: (from: string, yes: boolean) => void;
  live: (state: "connecting" | "on" | "refused" | "off") => void;
}

/** The relay's HTTP origin, from its socket's URL. */
export function httpOrigin(relay: string): string {
  const u = new URL(relay);
  u.protocol = u.protocol === "wss:" ? "https:" : u.protocol === "ws:" ? "http:" : u.protocol;
  return u.origin;
}

/** The `sub` claim of a room token (unverified: the relay verifies it). */
export function subOf(token: string): string {
  try {
    const part = token.split(".")[1] ?? "";
    const json = atob(part.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - (part.length % 4)) % 4));
    return String((JSON.parse(json) as { sub?: unknown }).sub ?? "");
  } catch {
    return "";
  }
}

export class Room {
  readonly client: PresenceClient;
  readonly room: string;
  /** Your own sub, once a token has been read. */
  me = "";
  private handlers: { [K in keyof RoomEvents]: Set<RoomEvents[K]> } = { members: new Set(), knock: new Set(), answer: new Set(), live: new Set() };
  private stop: () => void;
  private unsub: () => void;
  private seen = { members: [] as Member[], knocks: 0, answers: 0, live: "off" as string };

  constructor(o: ConnectOptions) {
    this.room = o.room;
    const relay = o.relay ?? RELAY;
    setAssetBase(o.assets ?? `${httpOrigin(relay)}/`);
    const token =
      typeof o.token === "string"
        ? ((this.me = subOf(o.token)), o.token)
        : async () => {
            const t = await (o.token as () => Promise<string>)();
            this.me = subOf(t);
            return t;
          };
    const storage = o.storage ?? "arugula.stream.";
    this.client = new PresenceClient({ transport: o.transport ?? phoenixTransport({ url: relay, token, room: o.room }), storage });
    const kept = (k: string) => {
      try {
        return localStorage.getItem(`${storage}${k}`) !== null;
      } catch {
        return false;
      }
    };
    if (!kept("mode")) this.client.setMode(o.mode ?? "avatar");
    if (o.look && !kept("look")) this.client.setLook(o.look);
    const client = this.client;
    this.camera = {
      async start(c = {}) {
        if (c.mode && c.mode !== client.mode) client.setMode(c.mode);
        await client.startCamera();
      },
      stop: () => client.stopCamera(),
      get on() {
        return client.cameraOn;
      },
    };
    this.unsub = this.client.subscribe(() => this.changed());
    this.stop = this.client.watch();
  }

  get members(): Member[] {
    return this.client.members;
  }

  on<K extends keyof RoomEvents>(event: K, fn: RoomEvents[K]): () => void {
    this.handlers[event].add(fn);
    return () => void this.handlers[event].delete(fn);
  }

  /** Your camera: avatar (a rig, a few dozen bytes a frame) or outline. */
  readonly camera: { start(o?: { mode?: "avatar" | "outline" }): Promise<void>; stop(): void; readonly on: boolean };

  setLook(look: Look) {
    this.client.setLook(look);
  }

  knock(sub: string) {
    this.client.knock(sub);
  }

  answer(sub: string, yes: boolean) {
    this.client.answer(sub, yes);
  }

  /** Draw everyone in the room into `el` (in a shadow root of its own,
   * with the elements' styles); returns a function to stop. */
  mount(el: Element, o: PeopleOptions = {}): () => void {
    return styled(el, (root) => mountPeople(this, root, o));
  }

  /** Draw one person into `el`, likewise. */
  avatar(el: Element, sub: string, o: AvatarOptions = {}): () => void {
    return styled(el, (root) => mountAvatar(this, root, sub, o), true);
  }

  leave() {
    this.client.stopCamera();
    this.unsub();
    this.stop();
  }

  private emit<K extends keyof RoomEvents>(event: K, ...args: Parameters<RoomEvents[K]>) {
    for (const fn of [...this.handlers[event]]) (fn as (...a: unknown[]) => void)(...args);
  }

  private changed() {
    const c = this.client;
    if (c.members !== this.seen.members) {
      this.seen.members = c.members;
      this.emit("members", c.members);
    }
    if (c.knocks.length > this.seen.knocks) for (const k of c.knocks.slice(this.seen.knocks)) this.emit("knock", k.from);
    this.seen.knocks = c.knocks.length;
    if (c.answers.length > this.seen.answers) for (const a of c.answers.slice(this.seen.answers)) this.emit("answer", a.from, a.yes);
    this.seen.answers = c.answers.length;
    if (c.live !== this.seen.live) {
      this.seen.live = c.live;
      this.emit("live", c.live);
    }
  }
}

/** A host in `el` with a shadow root carrying the stylesheet, for `draw`
 * to fill; `fill` makes the host take the element's whole box. */
export function styled(el: Element, draw: (root: ShadowRoot) => () => void, fill = false): () => void {
  const host = document.createElement("div");
  if (fill) host.style.cssText = "width:100%;height:100%";
  el.append(host);
  const root = host.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = CSS;
  root.append(style);
  const stop = draw(root);
  return () => {
    stop();
    host.remove();
  };
}

/** Join `o.room`: resolves once the relay has let you in (or after a few
 * seconds of trying, still connecting); rejects if it refuses. */
export function connect(o: ConnectOptions): Promise<Room> {
  const room = new Room(o);
  return new Promise((ok, no) => {
    if (room.client.live === "on") return ok(room);
    const t = setTimeout(() => (off(), ok(room)), 8_000);
    const off = room.on("live", (s) => {
      if (s === "on") {
        clearTimeout(t);
        off();
        ok(room);
      } else if (s === "refused") {
        clearTimeout(t);
        off();
        room.leave();
        no(new Error("the relay refused the room token"));
      }
    });
  });
}
