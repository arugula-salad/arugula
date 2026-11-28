// What the Saved and Threads badges count (Arugula #1529): what their views
// show, the rooms `inView` takes in the page's scope. Rooms in, numbers out,
// so `scripts/chat-badges.test.mjs` runs it without a browser.

import type { ChatRoom, ChatSavedItem, ThreadsUnread } from "../proto.gen";
import { inView, type ChatScope } from "./sidebar.ts";

/** The saved items in the rooms `scope` lists (all, with none): the Saved
 * view's list and its badge's count. A room `room` doesn't know isn't in a scope. */
export const savedIn = (saved: ChatSavedItem[], room: (id: string) => ChatRoom | undefined, scope?: ChatScope): ChatSavedItem[] =>
  saved.filter((i) => inView(room(i.room), scope));

/** The Threads badge: unread replies and how many mention the reader, over
 * the rooms the Threads view shows. `byRoom` is null from a server that
 * sends only totals: those, whatever the scope. */
export function threadsIn(
  byRoom: ThreadsUnread[] | null,
  totals: { n: number; mentions: number },
  room: (id: string) => ChatRoom | undefined,
  scope?: ChatScope,
): { n: number; mentions: number } {
  if (!byRoom) return totals;
  let n = 0;
  let mentions = 0;
  for (const t of byRoom) {
    if (!inView(room(t.room), scope)) continue;
    n += t.unread;
    mentions += t.mentions;
  }
  return { n, mentions };
}
