// Warming the box (M262): while someone writes `@arugula` in a project's
// room, the composer's typing ping says `warm`, and control wakes the
// project's agents box so it's up by the time the message is sent. Control
// decides whether it does (who may ask, which box, once a minute); the page
// only says when, once per composing burst. Plain functions, for
// ../chat-store.ts and room.tsx, and scripts/chat-warm.test.mjs.

import type { ChatKind, ChatTyping } from "../proto";

/** Whether a composer's text asks `@arugula`: it starts with it (even
 * before the word is done), or has `@arugula ` anywhere, after the start
 * or anything but a letter or digit, as control reads a mention. */
export function asksArugula(text: string): boolean {
  return /^\s*@arugula/i.test(text) || /(?:^|[^\p{L}\p{N}])@arugula\s/iu.test(text);
}

/** Rooms `@arugula` may work in: a channel, an issue's or PR's thread.
 * Never a DM, a pane's or a session's room. */
export function warmsIn(kind: ChatKind): boolean {
  return kind === "channel" || kind === "object";
}

/** Where a composer's burst is kept: the room's own composer by the
 * room, a reply thread's by the room and its parent. */
export function burstOf(room: string, thread?: number): string {
  return thread === undefined ? room : `${room} ${thread}`;
}

/** A typing ping's body: none for a plain one, `warm` for the burst's
 * first that asks. A reply thread's composer shows no typing, so its
 * ping is only ever a warm one (`show` false), and none otherwise. */
export function typingBody(warm: boolean, thread: boolean): ChatTyping | undefined | null {
  if (thread) return warm ? { warm: true, show: false } : null;
  return warm ? { warm: true, show: true } : undefined;
}

/** Which rooms' bursts have warmed: a ping says `warm` once per burst,
 * which ends when the composer is emptied (sent, or cleared). */
export class Warming {
  private warmed = new Set<string>();

  /** Whether this ping, in `room`, says `warm`: the burst's first one
   * whose text asks. */
  ping(room: string, asks: boolean): boolean {
    if (!asks || this.warmed.has(room)) return false;
    this.warmed.add(room);
    return true;
  }

  /** The composer in `room` was emptied: its next `@arugula` warms again. */
  ended(room: string) {
    this.warmed.delete(room);
  }
}
