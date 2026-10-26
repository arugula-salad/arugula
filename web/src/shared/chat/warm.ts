// Warming the box (M262): while someone writes `@arugula` in a project's
// room, the composer's typing ping says `warm`, and control wakes the
// project's agents box so it's up by the time the message is sent. Control
// decides whether it does (who may ask, which box, once a minute); the page
// only says when, once per composing burst. Plain functions, for
// ../chat-store.ts and room.tsx, and scripts/chat-warm.test.mjs.

import type { ChatKind } from "../proto";

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
