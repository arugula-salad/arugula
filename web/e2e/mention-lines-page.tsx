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
  // A board's thread reached its lead with no @mention of it (#1290): the answer names the session.
  { machine: "mch_studio", machine_name: "studio", state: "answered", session: "launch lead", reply: 14 },
  { machine: "mch_air", machine_name: "air", state: "offline" },
  { machine: "mch_box", machine_name: "box", state: "mentions_off" },
  // Typed by its display name, in another case than the handle (#1167).
  { machine: "mch_mac", machine_name: "Aarons-Mac-mini.local", state: "no_such_session", asked: "v1-engineering-done-board", names: ["Keymaster", "deploy approvals workflow", "launch"] },
  // Routing put it there: nobody typed it, so no warning.
  { machine: "mch_old", machine_name: "oldbox", state: "no_such_session", asked: "gone", names: ["a", "b"] },
  { machine: "mch_off", machine_name: "offbox", state: "offline" },
];

const room = { id: "ch_aaaa0001" } as unknown as ChatRoom;
const text = "@air and @BOX and @aarons-mac-mini.local/v1-engineering-done-board please";
const m = { seq: 11, parent: null, deleted: null, text, mention_status: status } as unknown as ChatMessage;

render(
  <div style="padding: 16px; max-width: 520px">
    <MentionStatus room={room} m={m} insert={(t) => ((document.getElementById("composer") as HTMLElement).textContent += t)} />
    <p id="composer" data-composer />
  </div>,
  document.getElementById("root")!,
);
