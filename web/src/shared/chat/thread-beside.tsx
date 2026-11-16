// A thread open beside a page that draws a room in it, not chat's own
// (an issue's comments, M196; a run's, M215): the same Thread panel a
// channel opens (./thread-panel), with its agent strip, over the page's
// right edge, or the whole screen on a phone. The route says which
// thread, as a channel's does; closing it goes back to the room's page.
// It's drawn in the body: a page (`.sw-page`) is a size container, which
// holds a fixed panel inside it.

import { createPortal } from "preact/compat";
import { chat } from "../chat-store";
import type { ChatRoom } from "../proto";
import { usePlatformApi } from "../platform-api";
import { ThreadPanel } from "./thread-panel";

export function ThreadBeside({ room, parent, focus, phone }: { room: ChatRoom; parent: number; focus?: number; phone: boolean }) {
  const { nav } = usePlatformApi();
  return createPortal(
    <div class={`chat thread-beside${phone ? " phone" : ""}`} data-thread-beside={parent}>
      <ThreadPanel key={parent} store={chat} room={room} parent={parent} phone={phone} focus={focus} close={() => nav.openRoom(room.id)} />
    </div>,
    document.body,
  );
}
