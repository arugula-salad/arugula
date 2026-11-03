// Attention that belongs to no pane (#551): a card for what the machine
// wants its owner for and no pane can show. Today that is a teammate's
// mention that no session took. The drawer, the bell and the phone's sheet
// draw them from the `State` (and `Delta`s) each machine sends its owner.

import type { Notice } from "./proto";

/** A card's title: what it says. A mention counts up ("2 mentions in
 * #agents"); one alone says who, where and the first line. */
export function noticeTitle(n: Notice): string {
  if (n.kind === "mention" && n.count > 1) return `${n.count} mentions in #${(n.room_name ?? n.room ?? "a room").replace(/^#/, "")}`;
  return n.headline;
}

/** What the card's primary button says. */
export const noticeOpenLabel = (n: Notice): string => (n.kind === "mention" ? "Open chat" : "Open");

/** The room a card opens, if it opens one. */
export const noticeRoom = (n: Notice): string | null => (n.kind === "mention" ? (n.room ?? null) : null);

/** Oldest first, a key to break a tie: the order they arrive in, so a new
 * card goes to the end and the ones above it don't jump. */
export function sortNotices<T extends { n: Notice }>(items: readonly T[]): T[] {
  return [...items].sort((a, b) => a.n.since_ms - b.n.since_ms || (a.n.key < b.n.key ? -1 : a.n.key > b.n.key ? 1 : 0));
}
