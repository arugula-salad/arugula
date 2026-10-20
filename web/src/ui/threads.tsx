// Threads on panes and sessions: the people working on something talk
// beside it. Since M123 a pane's or a session's thread is its room in
// control's chat (./room), and since M125 the daemon keeps none. This is the
// badge on a pane, the menu items that open its room in a drawer (the whole
// screen on a phone), and the layer that shows it.

import type { Client } from "../client";
import type { PaneId, SessionId } from "../proto";
import { colorOf } from "./people";
import type { MenuItem } from "./menu";
import { chat, chatStore } from "../chat-store";
import { useSubscribe } from "./hooks";
import { RoomDrawer, closeRoomDrawer, openRoomDrawer } from "./room";

export interface Quote {
  pane: PaneId;
  text: string;
}

/** What a thread is about: a pane or a session. */
export type ThreadTarget = { pane: PaneId } | { session: SessionId };

/** Show a thread (its room in control's chat), with output to quote in the
 * next message. */
export function openThread(client: Client, target: ThreadTarget, quote?: Quote) {
  const machine = controlChat(client);
  if (!machine) return;
  const pane = "pane" in target;
  const id = pane ? target.pane : target.session;
  void openRoomDrawer(machine, pane ? "pane" : "session", id, quote && { machine, pane: quote.pane, text: quote.text }, (s) => client.toast(s));
}

export function closeThread() {
  closeRoomDrawer();
}

/** The machine's id in control, when chat is control's (M123) and this
 * client reaches the machine through it. */
function controlChat(client: Client): string | null {
  return chatStore() ? (client.e2e?.daemon.id ?? null) : null;
}

/** A pane's or session's unread in its room in control, as "3 new". */
function roomUnreadLabel(machine: string, kind: "pane" | "session", id: number): string {
  const r = chatStore()?.roomOf(machine, kind, id);
  if (!r?.unread) return "";
  return `${r.mentions ? "@ " : ""}${r.unread} new`;
}

/** A session's thread's unread messages, and whether one mentions you. */
export function sessionUnread(client: Client, session: SessionId): { unread: number; mention: boolean } {
  const machine = controlChat(client);
  const r = machine ? chatStore()?.roomOf(machine, "session", session) : undefined;
  return { unread: r?.unread ?? 0, mention: !!r?.mentions };
}

/** The pane menu's thread items. */
export function threadItems(client: Client, pane: PaneId): MenuItem[] {
  const machine = controlChat(client);
  if (!machine) return [];
  const n = roomUnreadLabel(machine, "pane", pane);
  const selected = client.panes.get(pane)?.view.selection().trim() ?? "";
  return [
    { label: n ? `Thread (${n})` : "Thread", run: () => openThread(client, { pane }) },
    { label: "Quote selection in thread", disabled: !selected, run: () => openThread(client, { pane }, { pane, text: selected }) },
  ];
}

/** The session menu's thread item. */
export function sessionThreadItems(client: Client, session: number): MenuItem[] {
  const machine = controlChat(client);
  if (!machine) return [];
  const n = roomUnreadLabel(machine, "session", session);
  return [{ label: n ? `Session thread (${n})` : "Session thread", run: () => openThread(client, { session }) }];
}

/** On a pane: its unread messages, or that it has a thread. */
export function ThreadBadge({ client, pane }: { client: Client; pane: PaneId }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const machine = controlChat(client);
  if (!machine) return null;
  const u = chatStore()!.paneUnread(machine, pane);
  if (!u.room) return null;
  return (
    <button
      class={`thread-badge${u.unread ? " unread" : ""}${u.mention ? " mention" : ""}`}
      title={u.unread ? `${u.unread} new in this pane's thread` : "This pane's thread"}
      data-room-badge={u.room.id}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={() => openThread(client, { pane })}
    >
      <Bubble />
      {u.unread ? <span>{u.mention ? `@ ${u.unread}` : u.unread}</span> : null}
    </button>
  );
}

function Bubble() {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path
        d="M3 2.5h10a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5H7l-3 2.5V11.5H3A1.5 1.5 0 0 1 1.5 10V4A1.5 1.5 0 0 1 3 2.5z"
        fill="currentColor"
      />
    </svg>
  );
}

/** A pane's or session's room, open beside the panes. */
export function ThreadLayer({ phone, client }: { phone: boolean; client?: Client }) {
  return <RoomDrawer phone={phone} toast={(s) => client?.toast(s)} />;
}

/** A message's picture: theirs, or their initials on their color. */
export function MsgAvatar({ who, name, pic }: { who: string; name: string; pic?: string }) {
  const initials = name
    .split(/[\s@]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
  return (
    <span class="msg-avatar" style={{ background: colorOf(who) }} aria-hidden="true">
      {pic ? <img src={pic} alt="" referrerpolicy="no-referrer" /> : initials}
    </span>
  );
}
