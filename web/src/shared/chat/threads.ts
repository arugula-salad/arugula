// Reply threads (#106), the plain parts: how a parent says it has replies,
// where the "New" line goes in a thread, a message with its thread read or
// followed before the server has answered, and two runs of replies as one.
// Plain functions, for ./thread-state.ts, ./thread-ui.tsx and their tests
// (scripts/chat-threads.test.mjs).

import type { ChatMessage, ChatThread } from "../proto";

/** "just now", "5m ago", "3h ago", "2d ago", then the date: how long ago
 * `at` was, as the parent's footer says its newest reply. */
export function ago(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.round(h / 24);
  if (d < 30) return `${d}d ago`;
  return new Date(at).toLocaleDateString([], { month: "short", day: "numeric", year: new Date(at).getFullYear() === new Date(now).getFullYear() ? undefined : "numeric" });
}

/** "1 reply", "12 replies". */
export function repliesOf(n: number): string {
  return `${n} ${n === 1 ? "reply" : "replies"}`;
}

/** The footer under a parent: "3 replies · last reply 5m ago" (UI audit
 * #169: "last 5m ago" said nothing of what). */
export function footer(t: ChatThread, now: number): string {
  return `${repliesOf(t.replies)} · last reply ${ago(t.last, now)}`;
}

/** The thread a message belongs to: its own number if it starts one, else
 * the one it replies in. */
export function rootOf(m: ChatMessage): number {
  return m.parent ?? m.seq;
}

/** The first reply after the read mark that isn't mine: where the red
 * "New" line goes when a thread opens (null: nothing new). */
export function newFrom(replies: readonly ChatMessage[], read: number, mine: (m: ChatMessage) => boolean): number | null {
  return replies.find((m) => m.seq > read && !mine(m) && m.deleted === null)?.seq ?? null;
}

/** `m`'s thread read up to `seq`, as the server will have it: the mark
 * moves forward only, and what's after it is unread again. */
export function readTo(m: ChatMessage, seq: number, replies: readonly ChatMessage[], mine: (m: ChatMessage) => boolean): ChatMessage {
  const t = m.thread;
  if (!t || seq <= t.read) return m;
  const unread = t.following ? replies.filter((r) => r.seq > seq && !mine(r) && r.deleted === null).length : 0;
  return { ...m, thread: { ...t, read: seq, unread: Math.min(t.unread, unread), mentions: unread ? t.mentions : 0 } };
}

/** `m` followed or not, as the server will answer: following begins from
 * now (what's there is read), and stopping leaves no unread. */
export function followed(m: ChatMessage, on: boolean, newest: number): ChatMessage {
  const t = m.thread;
  if (!t || t.following === on) return m;
  return { ...m, thread: on ? { ...t, following: true, read: Math.max(t.read, newest), unread: 0, mentions: 0 } : { ...t, following: false, unread: 0, mentions: 0 } };
}

/** Two runs of replies as one, by number, the later copy winning. */
export function mergeReplies(a: readonly ChatMessage[], b: readonly ChatMessage[]): ChatMessage[] {
  const by = new Map<number, ChatMessage>();
  for (const m of a) by.set(m.seq, m);
  for (const m of b) by.set(m.seq, m);
  return [...by.values()].sort((x, y) => x.seq - y.seq);
}

/** The text of the Threads view's badge: unread replies, `@n` when some
 * mention me. Empty for none. */
export function badge(unread: number, mentions: number): string {
  if (unread <= 0) return "";
  return mentions > 0 ? `@${unread}` : String(unread);
}
