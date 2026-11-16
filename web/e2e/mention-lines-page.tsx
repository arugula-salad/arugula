// The fixture page for a message's mention lines (#502): the real lines
// under a post that names two sessions of one machine, one session of
// another, a third machine itself, and a fourth whose owner turned
// agent mentions off. mention-lines.spec.ts builds this
// with Vite and serves it, so no daemon is involved.

import { render } from "preact";
import "../src/shared/style.css";
import { MentionStatus } from "../src/shared/chat/mention-status";
import type { ChatMentionStatus, ChatMessage, ChatRoom } from "../src/shared/proto";

const status: ChatMentionStatus[] = [
  { machine: "mch_geek", machine_name: "geek", for_session: "chat", state: "working", session: "chat" },
  { machine: "mch_geek", machine_name: "geek", for_session: "review", state: "sending" },
  { machine: "mch_mini", machine_name: "mini", for_session: "build", state: "answered", session: "build", reply: 12 },
  { machine: "mch_air", machine_name: "air", state: "offline" },
  { machine: "mch_box", machine_name: "box", state: "mentions_off" },
];

const room = { id: "ch_aaaa0001" } as unknown as ChatRoom;
const m = { seq: 11, parent: null, deleted: null, mention_status: status } as unknown as ChatMessage;

render(
  <div style="padding: 16px; max-width: 520px">
    <MentionStatus room={room} m={m} />
  </div>,
  document.getElementById("root")!,
);
