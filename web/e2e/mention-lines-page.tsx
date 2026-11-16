// The fixture page for a message's mention lines (#502): the real lines
// under a post that names two sessions of one machine, one session of
// another and a third machine itself; and an issue's comment with replies
// in its thread (Arugula #1071). mention-lines.spec.ts builds this with
// Vite and serves it, so no daemon is involved. Where a click goes is kept
// in `window.opened`.

import { render } from "preact";
import "../src/shared/style.css";
import { MentionStatus } from "../src/shared/chat/mention-status";
import { ThreadFooter } from "../src/shared/chat/thread-ui";
import { PlatformApiContext, type PlatformApi } from "../src/shared/platform-api";
import type { ChatMentionStatus, ChatMessage, ChatRoom } from "../src/shared/proto";

const status: ChatMentionStatus[] = [
  { machine: "mch_geek", machine_name: "geek", for_session: "chat", state: "working", session: "chat" },
  { machine: "mch_geek", machine_name: "geek", for_session: "review", state: "sending" },
  { machine: "mch_mini", machine_name: "mini", for_session: "build", state: "answered", session: "build", reply: 12 },
  { machine: "mch_air", machine_name: "air", state: "offline" },
];

const room = { id: "ch_aaaa0001" } as unknown as ChatRoom;
const m = { seq: 11, parent: null, deleted: null, mention_status: status } as unknown as ChatMessage;

// An issue's comment (its object room) that two agents answered in its thread.
const issueRoom = { id: "ob_aaaa0002", kind: "object" } as unknown as ChatRoom;
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
      <MentionStatus room={room} m={m} />
      <div data-issue-comment>
        <MentionStatus room={issueRoom} m={comment} />
        <ThreadFooter room={issueRoom} m={comment} />
      </div>
    </div>
  </PlatformApiContext.Provider>,
  document.getElementById("root")!,
);
