// The fixture page for an issue's comment that agents answered in its
// thread (Arugula #1071): its mention line and its replies link, in the
// issue's room (an object room). issue-thread-links.spec.ts builds this
// with Vite and serves it, so no daemon is involved. Where a click goes is
// kept in `window.opened`.

import { render } from "preact";
import "../src/shared/style.css";
import { MentionStatus } from "../src/shared/chat/mention-status";
import { ThreadFooter } from "../src/shared/chat/thread-ui";
import { PlatformApiContext, type PlatformApi } from "../src/shared/platform-api";
import type { ChatMessage, ChatRoom } from "../src/shared/proto";

const room = { id: "ob_aaaa0002", kind: "object" } as unknown as ChatRoom;
const comment = {
  seq: 31,
  parent: null,
  deleted: null,
  mention_status: [{ machine: "mch_box", machine_name: "box", for_session: "check-b", state: "answered", session: "check-b", reply: 32 }],
  thread: { replies: 2, last: Date.now() - 60_000, repliers: [{ account: "acc_1", name: "Ada" }], following: false, unread: 0, mentions: 0, read: 0 },
} as unknown as ChatMessage;

const opened: unknown[][] = [];
(window as unknown as { opened: unknown[][] }).opened = opened;
const api = { nav: { openRoom: (...a: unknown[]) => opened.push(a.map((x) => x ?? null)) } } as unknown as PlatformApi;

render(
  <PlatformApiContext.Provider value={api}>
    <div style="padding: 16px; max-width: 520px">
      <MentionStatus room={room} m={comment} />
      <ThreadFooter room={room} m={comment} />
    </div>
  </PlatformApiContext.Provider>,
  document.getElementById("root")!,
);
