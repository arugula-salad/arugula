// arugula.video's relay as a transport (../../PROTOCOL.md): a Phoenix
// socket with a room token (or a publishable key and a guest's name), the
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
  token?: never;
}

export type PhoenixTransportOptions = {
  /** The relay's socket, e.g. `wss://arugula.video/socket`. */
  url: string;
  /** The room, as the token names it. */
  room: string;
} & (TokenAuth | KeyAuth);

/** The socket's params: the token, or the key, room and guest's name. */
export function socketParams(o: PhoenixTransportOptions, token: string): Record<string, string> {
  if (o.key === undefined) return { token };
  const p: Record<string, string> = { key: o.key, room: o.room, name: o.guest };
  if (o.look) p.look = `${o.look.character}:${o.look.colour}`;
  return p;
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
  let token = typeof o.token === "string" ? o.token : "";
  let socket: Socket | null = null;
  let channel: Channel | null = null;
  let on: TransportEvents | null = null;
  let listening = false;
  let publishing = false;
  let state: State = {};

  const join = async () => {
    if (channel) return;
    if (typeof o.token === "function") token = await o.token();
    if (channel || (!listening && !publishing)) return;
    socket = new Socket(o.url, { params: () => socketParams(o, token), binaryType: "arraybuffer" } as never);
    socket.connect();
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
      .receive("ok", (reply: { calls?: boolean }) => {
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
