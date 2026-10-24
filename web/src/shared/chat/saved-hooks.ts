// Saved for later and mark as unread (#116) on a message's tools (#121's
// hooks): the save star on the hover toolbar, and mark unread and the
// reminders in its ⋯ menu and the phone's sheet. A failure shows in the
// store's error, as the store's other writes do.

import { chat } from "../chat-store";
import { messageHooks, type MessageCtx } from "./msg-actions";
import { HOUR, tomorrowMorning } from "./saved";

const saved = (c: MessageCtx) => chat.isSaved(c.room.id, c.m.seq);
const quietly = (p: Promise<unknown>) => void p.catch(() => {});
// Not on a deleted message, and only where it's a room's message to keep.
const keeps = (c: MessageCtx) => c.m.deleted === null;

messageHooks.save = {
  run: (c) => quietly(saved(c) ? chat.unsave(c.room.id, c.m.seq) : chat.save(c.room.id, c.m.seq)),
  label: (c) => (saved(c) ? "Remove from saved" : "Save for later"),
  on: saved,
  when: keeps,
};
messageHooks.unread = { run: (c) => quietly(chat.markUnread(c.room.id, c.m.seq)), when: keeps };
messageHooks.remindHour = { run: (c) => quietly(chat.save(c.room.id, c.m.seq, Date.now() + HOUR)), when: keeps };
messageHooks.remindTomorrow = { run: (c) => quietly(chat.save(c.room.id, c.m.seq, tomorrowMorning())), when: keeps };
