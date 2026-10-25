// Pinning (#119) in a message's ⋯ menu and the phone's sheet (#121's hook).

import { chat } from "../chat-store";
import { messageHooks } from "./msg-actions";
import { mayPin } from "./pins";

messageHooks.pin = {
  run: (c) => void chat.pin(c.room.id, c.m.seq, !c.m.pinned).catch(() => {}),
  label: (c) => (c.m.pinned ? "Unpin" : "Pin to the room"),
  on: (c) => c.m.pinned,
  when: (c) => mayPin(c.room, c.m),
};
