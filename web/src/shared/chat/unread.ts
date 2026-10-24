// What counts as unread, and where it's counted. The Chat button counts
// only the rooms the chat page lists (a team's channels and DMs, the
// threads on panes and sessions, archives), so its number is one you can
// find and clear there. The threads on objects (M196: a native issue's, a
// run's, a mirrored PR's or issue's, a hosted workspace's) aren't listed
// in chat: they're on their objects' pages, so their unread is on the
// Issues and Runs lists and places, and in the inbox's "New in threads".
// Your own agents' posts count as unread everywhere (M197): control says
// so in `unread`, and nothing here second-guesses it.
//
// No page state: rooms in, counts out, so `scripts/chat-unread.test.mjs`
// runs it without a browser.

import type { ChatRoom, InboxEntry } from "../proto.gen";

/** What a room adds to a count: its unread, or only its mentions while
 * it's muted. */
export function counted(r: ChatRoom): number {
  return r.muted ? r.mentions : r.unread;
}

/** Whether the chat page's sidebar lists a room (`groupsOf` in
 * shared/chat/rooms.tsx): a team's channels and DMs, the threads on
 * panes and sessions, and archives. */
export function listedInChat(r: ChatRoom): boolean {
  switch (r.kind) {
    case "channel":
    case "dm":
      return !!r.team;
    case "pane":
    case "session":
    case "archive":
      return true;
    default:
      return false;
  }
}

/** The Chat button's count, and whether any of it mentions me: the rooms
 * chat lists, muted ones by their mentions. */
export function chatUnread(rooms: readonly ChatRoom[]): { n: number; mention: boolean } {
  let n = 0;
  let mention = false;
  for (const r of rooms) {
    if (!listedInChat(r)) continue;
    n += counted(r);
    mention ||= r.mentions > 0;
  }
  return { n, mention };
}

/** The thread on an object (`iss_…`, `run_…`), if someone's opened it. */
export function objectRoom(rooms: readonly ChatRoom[], target: string): ChatRoom | undefined {
  return rooms.find((r) => r.kind === "object" && r.target === target);
}

/** Unread in the thread on an object, for its row on a list. */
export function objectUnread(rooms: readonly ChatRoom[], target: string): { n: number; mention: boolean; room?: ChatRoom } {
  const room = objectRoom(rooms, target);
  return { n: room ? counted(room) : 0, mention: !!room?.mentions, room };
}

/** Unread in the threads on a project's objects whose ids start with
 * `prefix` (`iss_` for its Issues, `run_` for its Runs). */
export function projectUnread(rooms: readonly ChatRoom[], project: string, prefix: string): { n: number; mention: boolean } {
  let n = 0;
  let mention = false;
  for (const r of rooms) {
    if (r.kind !== "object" || r.project !== project || !r.target?.startsWith(prefix)) continue;
    n += counted(r);
    mention ||= r.mentions > 0;
  }
  return { n, mention };
}

/** A mention the inbox already has an entry for. */
const MENTIONS = new Set(["mentioned", "thread_mention"]);

/** The inbox's "New in threads": each object's thread with something
 * unread, unless an unseen mention in the inbox already points at it (a
 * native issue's `mentioned`, a mirrored item's `thread_mention`). Once
 * that's seen, what comes after it is new here. Mentions first, then the
 * most unread. */
export function threadNews(rooms: readonly ChatRoom[], entries: readonly InboxEntry[]): ChatRoom[] {
  const covered = new Set<string>();
  for (const e of entries) {
    if (!MENTIONS.has(e.reason) || e.seen_at || e.done_at) continue;
    covered.add(e.item);
    if (e.room) covered.add(e.room);
  }
  return rooms
    .filter((r) => r.kind === "object" && counted(r) > 0 && !covered.has(r.id) && !(r.target && covered.has(r.target)))
    .sort((a, b) => Number(b.mentions > 0) - Number(a.mentions > 0) || counted(b) - counted(a) || a.name.localeCompare(b.name));
}
