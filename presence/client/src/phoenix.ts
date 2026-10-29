// arugula.video's relay as a transport (../../PROTOCOL.md): a Phoenix
// socket with a room token (or a publishable key, and a guest's name or a
// signed-in member's own token from the app's auth provider), the
// room's channel, binary frames both ways
// (the relay puts the sender's sub in front of each: `len:u8 sub frame`),
// Phoenix.Presence for who's there, waves and calls. The channel is
// joined while this page listens or publishes, and left when it does
// neither.

import { Presence, Socket, type Channel } from "phoenix";
import type { Look } from "@arugula/presence-core/avatar/rig";
import type { CallJoin, Member, Mode, State, Transport, TransportEvents, Message } from "./transport.ts";

/** Into the room with a room token: your server decided who's in it. */
export interface TokenAuth {
  /** A room token (PROTOCOL.md), or a function for a fresh one: called on
   * every (re)connect. */
  token: string | (() => Promise<string>);
  key?: never;
  guest?: never;
  auth?: never;
}

/** Into the room as a guest, with an app's publishable key (PROTOCOL.md,
 * "Publishable keys"): the page must be on one of the app's origins. */
export interface KeyAuth {
  /** `pk_live_…` or `pk_test_…`. */
  key: string;
  /** The guest's name; the relay makes their sub. */
  guest: string;
  /** A look to start with. */
  look?: Look;
  /** This browser's guest id, for the relay's usage metering only
   * (PROTOCOL.md, "Usage"): by default made once and kept in localStorage
   * (`guestId()`), so a guest counts once a month however often they
   * reconnect. Never identity: nobody in the room sees it. */
  gid?: string;
  token?: never;
  auth?: never;
}

/** Into the room as a member, signed in with the app's own auth provider
 * (PROTOCOL.md, "Members by sign-in"): a publishable key and the person's
 * token from Clerk, Auth0, Firebase, Supabase or any OIDC provider, which
 * the relay checks against the app's issuer. */
export interface MemberAuth {
  /** `pk_live_…` or `pk_test_…`. */
  key: string;
  /** Their provider token, or a function for a fresh one: called on
   * every (re)connect (Clerk's last a minute). */
  auth: string | (() => Promise<string>);
  /** What to call them if the token has no name. */
  name?: string;
  look?: Look;
  token?: never;
  guest?: never;
}

export type PhoenixTransportOptions = {
  /** The relay's socket, e.g. `wss://arugula.video/socket`. */
  url: string;
  /** The room, as the token names it. */
  room: string;
  /** Called with your sub whenever the relay lets you in (its join reply):
   * a guest's or a provider-token member's sub is the relay's to make, so
   * this is how a page learns it. */
  onJoin?: (you: string) => void;
} & (TokenAuth | KeyAuth | MemberAuth);

/** The socket's params: the token; or the key and room, with the guest's
 * name (and gid) or the member's provider token (`token` is the newest of
 * either). */
export function socketParams(o: PhoenixTransportOptions, token: string): Record<string, string> {
  if (o.key === undefined) return { token };
  const p: Record<string, string> = { key: o.key, room: o.room };
  if (o.auth !== undefined) {
    p.auth = token;
    if (o.name) p.name = o.name;
  } else {
    p.name = o.guest;
    if (o.gid) p.gid = o.gid;
  }
  if (o.look) p.look = `${o.look.character}:${o.look.colour}`;
  return p;
}

const GID_KEY = "arugula.gid";
let pageGid: string | undefined;

/** This browser's guest id: 22 random url-safe characters, made once and
 * kept in localStorage (or, where storage is off, for this page). The
 * relay counts a guest by it for usage, and for nothing else. */
export function guestId(): string {
  try {
    const kept = localStorage.getItem(GID_KEY);
    if (kept && /^[A-Za-z0-9_-]{8,64}$/.test(kept)) return kept;
    const made = randomId();
    localStorage.setItem(GID_KEY, made);
    return made;
  } catch {
    return (pageGid ??= randomId());
  }
}

function randomId(): string {
  const b = crypto.getRandomValues(new Uint8Array(16));
  return btoa(String.fromCharCode(...b)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** A relayed frame's sender and the frame itself. */
export function unprefix(b: Uint8Array): { sub: string; frame: Uint8Array } {
  const n = b[0] ?? 0;
  if (b.length < 1 + n) throw new Error("relayed frame: cut short");
  return { sub: new TextDecoder().decode(b.subarray(1, 1 + n)), frame: b.subarray(1 + n) };
}

interface Meta {
  name?: string;
  look?: Look | null;
  mode?: Mode;
  away?: boolean;
  watching?: boolean;
  call?: string | null;
}

/** A member from their connections' metas: on camera if any page is, away
 * only if every page is, watching if any is. */
export function memberOf(sub: string, metas: Meta[]): Member {
  const first = metas[0] ?? {};
  const mode = metas.find((m) => m.mode && m.mode !== "none")?.mode ?? "none";
  return {
    sub,
    name: first.name ?? sub,
    look: metas.find((m) => m.look)?.look ?? null,
    mode,
    away: metas.length > 0 && metas.every((m) => m.away),
    watching: metas.some((m) => m.watching),
    call: metas.find((m) => m.call)?.call ?? null,
  };
}

export function phoenixTransport(o: PhoenixTransportOptions): Transport {
  // A guest's id for metering, unless the page gave one.
  if (o.key !== undefined && o.auth === undefined && o.gid === undefined) o = { ...o, gid: guestId() };
  // A room token or a member's provider token: as given, or fresh from a
  // function on each join, and again whenever the socket drops, so
  // Phoenix's own reconnects carry an unexpired one.
  const cred = o.key === undefined ? o.token : o.auth;
  const fresh = typeof cred === "function" ? cred : null;
  let token = typeof cred === "string" ? cred : "";
  let refreshing = false;
  const refresh = () => {
    if (!fresh || refreshing) return;
    refreshing = true;
    fresh()
      .then((t) => void (token = t))
      .catch(() => {})
      .finally(() => void (refreshing = false));
  };
  let socket: Socket | null = null;
  let channel: Channel | null = null;
  let on: TransportEvents | null = null;
  let listening = false;
  let publishing = false;
  let state: State = {};

  const join = async () => {
    if (channel) return;
    if (fresh) token = await fresh();
    if (channel || (!listening && !publishing)) return;
    const s = new Socket(o.url, { params: () => socketParams(o, token), binaryType: "arraybuffer" } as never);
    socket = s;
    s.onClose(() => {
      if (socket === s) refresh();
    });
    s.connect();
    const ch = socket.channel(`room:${o.room}`, {});
    channel = ch;
    on?.live("connecting");
    ch.on("frame", (payload: unknown) => {
      if (!(payload instanceof ArrayBuffer) || !listening) return;
      try {
        const { sub, frame } = unprefix(new Uint8Array(payload));
        on?.frame(sub, frame);
      } catch {
        // Not a frame this page reads: skipped.
      }
    });
    ch.on("off", ({ sub }: { sub: string }) => {
      if (listening) on?.off(sub);
    });
    ch.on("wave", ({ from }: { from: string }) => on?.wave?.(from));
    ch.on("react", ({ from, emoji }: { from: string; emoji: string }) => on?.react?.(from, emoji));
    ch.on("answer", ({ from, yes }: { from: string; yes: boolean }) => on?.answer?.(from, yes));
    ch.on("said", (m: Message) => on?.said?.(m));
    ch.on("history", ({ messages }: { messages: Message[] }) => on?.history?.(messages));
    ch.on("call", ({ from, id }: { from: string; id: string }) => on?.call?.(from, id));
    ch.on("call_cancel", ({ id }: { id: string }) => on?.callCancel?.(id));
    ch.on("call_answer", (a: { id: string; from: string; yes: boolean; reason?: string }) => on?.callAnswer?.(a.id, a.from, a.yes, a.reason));
    ch.on("call_join", (j: CallJoin) => on?.callJoin?.(j));
    const presence = new Presence(ch);
    presence.onSync(() => on?.members?.(presence.list((sub, { metas }: { metas: Meta[] }) => memberOf(sub, metas))));
    ch.join()
      .receive("ok", (reply: { you?: string; calls?: boolean } | undefined) => {
        if (reply?.you) o.onJoin?.(reply.you);
        on?.calls?.(!!reply?.calls);
        on?.live("on");
        ch.push("state", { ...state, watching: listening });
      })
      .receive("error", () => {
        on?.live("refused");
        leave();
      });
  };

  const leave = () => {
    channel?.leave();
    channel = null;
    socket?.disconnect();
    socket = null;
  };

  const settle = () => {
    if (listening || publishing) void join();
    else leave();
  };

  const push = (s: State) => {
    state = { ...state, ...s };
    channel?.push("state", s);
  };

  return {
    listen(events) {
      on = events;
      listening = true;
      settle();
      push({ watching: true });
    },
    unlisten() {
      listening = false;
      push({ watching: false });
      settle();
    },
    publish(bytes) {
      if (!publishing) {
        publishing = true;
        settle();
      }
      // Its own buffer: Phoenix sends an ArrayBuffer whole.
      channel?.push("frame", bytes.slice().buffer as never);
    },
    off() {
      publishing = false;
      push({ mode: "none" });
      settle();
    },
    state: push,
    react(emoji) {
      channel?.push("react", { emoji });
    },
    wave(to) {
      channel?.push("wave", { to });
    },
    answer(to, yes) {
      channel?.push("answer", { to, yes });
    },
    say(text) {
      return ask<Message>("say", { text });
    },
    history(before) {
      return ask<{ messages: Message[] }>("history", { before }).then((r) => r.messages);
    },
    call(to) {
      return ask<{ id: string }>("call", { to });
    },
    callAnswer(id, yes) {
      return ask<unknown>("call_answer", { id, yes }).then(() => undefined);
    },
    callLeave(id) {
      channel?.push("call_leave", { id });
    },
    callToken(id) {
      return ask<CallJoin>("call_token", { id });
    },
  };

  /** A push the relay answers: its `ok` payload, or its `error` reason. */
  function ask<T>(event: string, payload: object): Promise<T> {
    return new Promise((resolve, reject) => {
      if (!channel) return reject(new Error("not in the room yet"));
      channel
        .push(event, payload)
        .receive("ok", (r: T) => resolve(r))
        .receive("error", (e: { reason?: string }) => reject(new Error(e?.reason ?? "refused")))
        .receive("timeout", () => reject(new Error("the relay didn't answer")));
    });
  }
}
