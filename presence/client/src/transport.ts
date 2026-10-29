// How a presence client reaches the people it shows (arugula.video's
// PROTOCOL.md, ../../PROTOCOL.md): it publishes this page's frames and
// state, and hears everyone else's. Frames are opaque bytes here and on
// the wire; who sees whom is the transport's and its server's business.

import type { Look } from "@arugula/presence-core/avatar/rig";

/** What the camera sends: outline lines, a rig for an avatar, or nothing. */
export type Mode = "outline" | "avatar" | "none";

/** Someone in the room, as the relay's presence says (a transport without
 * presence, Arugula control's, has no members). */
export interface Member {
  sub: string;
  name: string;
  look: Look | null;
  mode: Mode;
  away: boolean;
  watching: boolean;
}

/** This page's part in its members' meta. */
export interface State {
  mode?: Mode;
  look?: Look;
  away?: boolean;
  watching?: boolean;
}

/** A kept text message (PROTOCOL.md `said`): `id` only goes up in a room,
 * `at` is Unix ms. */
export interface Message {
  id: number;
  sub: string;
  name: string;
  text: string;
  at: number;
}

export interface TransportEvents {
  /** `sub`'s newest frame. */
  frame(sub: string, bytes: Uint8Array): void;
  /** `sub`'s camera went off, or their frame went stale. */
  off(sub: string): void;
  /** Hearing others: connecting, on, or refused (it won't try again). */
  live(state: "connecting" | "on" | "refused"): void;
  /** Everyone in the room, whenever that changes. */
  members?(members: Member[]): void;
  /** Someone asks to talk. */
  wave?(from: string): void;
  /** Someone's reaction: an emoji for the room. */
  react?(from: string, emoji: string): void;
  answer?(from: string, yes: boolean): void;
  /** Someone's message (not this connection's: that's `say`'s answer). */
  said?(m: Message): void;
  /** The room's last messages, on (re)joining. */
  history?(messages: Message[]): void;
}

export interface Transport {
  /** Start hearing others (a page shows them). */
  listen(on: TransportEvents): void;
  /** Stop hearing others. */
  unlisten(): void;
  /** This page's newest frame. */
  publish(bytes: Uint8Array): void;
  /** This page's camera went off. */
  off(): void;
  /** The page is closing: say so if it can (a beacon). */
  leaving?(): void;
  state?(s: State): void;
  wave?(to: string): void;
  react?(emoji: string): void;
  answer?(to: string, yes: boolean): void;
  /** Send a message; resolves with it as kept, rejects with the reason. */
  say?(text: string): Promise<Message>;
  /** The messages before `before`, oldest first. */
  history?(before: number): Promise<Message[]>;
}
