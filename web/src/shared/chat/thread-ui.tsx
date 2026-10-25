// Reply threads on a message (#106), the parts a room's messages carry: the
// footer under a message that has replies ("3 replies · last 5m ago", with
// the repliers' pictures), the tag on a reply sent to the channel, and the
// "Reply in thread" hook of the message tools (./msg-actions). The panel
// that reads a thread is ./thread-panel, the Threads view ./threads-view.

import "./threads.css";
import type { ChatMessage, ChatRoom } from "../proto";
import { usePlatformApi, type ChatNav } from "../platform-api";
import { MsgAvatar } from "./avatar";
import { messageHooks } from "./msg-actions";
import { ago, footer, rootOf } from "./threads";

// The hook can't ask for the page's nav: the room it's shown in registers
// it (`RoomBody`), and the hook opens the thread there.
let nav: ChatNav | null = null;
export function setThreadNav(n: ChatNav | null) {
  nav = n;
}

/** Whether a room's messages take replies in a thread: those a team's
 * people talk in. An issue's or a run's comments are one flat list, as
 * they are on GitHub; an archive is read only. */
export function threads(room: ChatRoom): boolean {
  return room.kind === "channel" || room.kind === "dm" || room.kind === "pane" || room.kind === "session";
}

messageHooks.thread = {
  run: ({ room, m }) => nav?.openRoom(room.id, undefined, rootOf(m)),
  when: ({ room, m }) => threads(room) && room.writable && m.deleted === null && m.imported === null,
};

/** Under a message that has replies: how many, the newest's age, who
 * replied, and what's new to me if I follow it. Opens the thread. */
export function ThreadFooter({ room, m }: { room: ChatRoom; m: ChatMessage }) {
  const { nav } = usePlatformApi();
  const t = m.thread;
  if (!t || !threads(room)) return null;
  const unread = t.following ? t.unread : 0;
  return (
    <button
      type="button"
      class={`thread-footer${unread ? " unread" : ""}`}
      data-thread-footer={m.seq}
      title={`Open the thread. The newest reply was ${new Date(t.last).toLocaleString()}`}
      onClick={() => nav.openRoom(room.id, undefined, m.seq)}
    >
      <span class="thread-footer-faces">
        {[...t.repliers].reverse().map((p) => (
          <MsgAvatar key={p.account} who={p.account} name={p.name || "someone"} account={p.account} />
        ))}
      </span>
      <span class="thread-footer-count">{footer(t, Date.now())}</span>
      {unread > 0 && (
        <span class={t.mentions ? "chat-count mention" : "chat-count"} data-thread-unread>
          {t.mentions ? `@${unread}` : `${unread} new`}
        </span>
      )}
      <span class="thread-footer-go" aria-hidden="true">
        ›
      </span>
    </button>
  );
}

/** On a reply that was sent to the channel too: that it is one, and a way
 * into its thread. In the thread itself it only says it was also sent. */
export function ReplyTag({ room, m, inThread }: { room: ChatRoom; m: ChatMessage; inThread: boolean }) {
  const { nav } = usePlatformApi();
  if (m.parent === undefined) return null;
  if (inThread) {
    return m.also ? (
      <span class="thread-tag reply-also" data-reply-also>
        Also sent to the channel
      </span>
    ) : null;
  }
  return (
    <button type="button" class="reply-tag" data-reply-tag onClick={() => nav.openRoom(room.id, m.seq, m.parent)}>
      ↩ Replied in a thread
    </button>
  );
}

/** "last 5m ago", for the Threads view's rows. */
export function lastAgo(at: number): string {
  return ago(at, Date.now());
}
