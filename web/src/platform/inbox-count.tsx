// The count on the bar's Inbox (UI audit #170), as control's bell counts
// what needs you: the inbox page's "unseen" (its entries not yet seen, and
// the threads with news chat doesn't list). The entries are read when the
// bar comes up, when the reader comes back to the window, and whenever the
// inbox page reads them (it hands them here); the threads are chat's,
// live.

import { useEffect } from "preact/hooks";
import type { InboxEntry } from "../shared/proto.gen";
import { chat, chatStore } from "../shared/chat-store";
import { threadNews } from "../shared/chat/unread";
import { platformFetch } from "../shared/maintenance";
import { useSubscribe } from "../shared/ui/hooks";

let entries: InboxEntry[] | null = null;
const listeners = new Set<() => void>();

/** What the inbox page just read. */
export function noteInbox(list: InboxEntry[]) {
  entries = list.filter((e) => e.reason !== "draft");
  listeners.forEach((fn) => fn());
}

async function load() {
  try {
    const r = await platformFetch("/api/inbox", { credentials: "same-origin" });
    if (r.ok) noteInbox(((await r.json()) as { entries: InboxEntry[] }).entries);
  } catch {
    // The count waits for the next read; the page says what went wrong.
  }
}

/** The inbox page's "unseen", or 0 before it's known. */
export function inboxUnseen(): number {
  if (entries === null) return 0;
  const threads = chatStore() ? threadNews(chat.rooms, entries).length : 0;
  return entries.filter((e) => !e.seen_at).length + threads;
}

/** The count, beside the bar's Inbox: nothing while nothing's unseen. */
export function InboxCount() {
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  });
  useSubscribe((fn) => chat.subscribe(fn));
  useEffect(() => {
    void load();
    const back = () => document.visibilityState === "visible" && void load();
    document.addEventListener("visibilitychange", back);
    return () => document.removeEventListener("visibilitychange", back);
  }, []);
  const n = inboxUnseen();
  if (!n) return null;
  const title = `${n} unseen in your inbox`;
  return (
    <span class="thread-unread place-unread" title={title} data-inbox-count={n}>
      <span class="thread-unread-dot" aria-hidden="true" />
      <span aria-hidden="true">{n > 99 ? "99+" : n}</span>
      <span class="place-unread-said">{title}</span>
    </span>
  );
}
