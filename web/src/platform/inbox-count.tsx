// The count on the bar's Inbox (UI audit #170), as control's bell counts
// what needs you: the inbox page's "unseen" (its entries not yet seen, and
// the threads with news chat doesn't list), and the deploys waiting for the
// reader's approval (#468), which the inbox shows first. The entries and
// the deploys are read when the bar comes up, when the reader comes back to
// the window, and whenever the inbox page reads them (it hands them here);
// the threads are chat's, live.

import { useEffect } from "preact/hooks";
import type { InboxEntry } from "../shared/proto.gen";
import { chat, chatStore } from "../shared/chat-store";
import { threadNews } from "../shared/chat/unread";
import { platformFetch } from "../shared/maintenance";
import { useSubscribe } from "../shared/ui/hooks";
import { session } from "./session";

let entries: InboxEntry[] | null = null;
/** Deploys waiting for the reader's approval (`/api/deploys/waiting`). */
let waiting = 0;
const listeners = new Set<() => void>();

/** Whether an entry counts as seen: marked so, or one that's done (a PR
 * ready, merged or closed, an issue closed), which tells the reader
 * something and asks nothing of them (UI audit #192). */
export const isSeen = (e: InboxEntry) => !!e.seen_at || e.reason === "done";

/** What the inbox page just read. */
export function noteInbox(list: InboxEntry[]) {
  entries = list.filter((e) => e.reason !== "draft");
  listeners.forEach((fn) => fn());
}

/** How many deploys wait for the reader, as the inbox page just read them. */
export function noteWaitingDeploys(n: number) {
  waiting = n;
  listeners.forEach((fn) => fn());
}

async function load() {
  const read = async <T,>(path: string): Promise<T | null> => {
    try {
      const r = await platformFetch(path, { credentials: "same-origin" });
      return r.ok ? ((await r.json()) as T) : null;
    } catch {
      // The count waits for the next read; the page says what went wrong.
      return null;
    }
  };
  const [inbox, deploys] = await Promise.all([read<{ entries: InboxEntry[] }>("/api/inbox"), read<{ deploys: unknown[] }>("/api/deploys/waiting")]);
  if (deploys) noteWaitingDeploys(deploys.deploys.length);
  if (inbox) noteInbox(inbox.entries);
}

/** Deploys waiting for the reader's approval, or 0 before it's known. */
export const deploysWaiting = () => waiting;

/** The inbox page's "unseen", or 0 before it's known. */
export function inboxUnseen(): number {
  if (entries === null) return 0;
  const threads = chatStore() ? threadNews(chat.rooms, entries).length : 0;
  return entries.filter((e) => !isSeen(e)).length + threads;
}

/** The count, beside the bar's Inbox: nothing while nothing's unseen and
 * no deploy waits. */
export function InboxCount() {
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  });
  useSubscribe((fn) => chat.subscribe(fn));
  // Once signed in: the bar is drawn while the page signs in, and a call
  // before then would race the sign-in.
  const ready = session.state === "ready";
  useEffect(() => {
    if (!ready) return;
    void load();
    const back = () => document.visibilityState === "visible" && void load();
    document.addEventListener("visibilitychange", back);
    return () => document.removeEventListener("visibilitychange", back);
  }, [ready]);
  const unseen = inboxUnseen();
  const n = unseen + waiting;
  if (!n) return null;
  const said = [waiting ? `${waiting} ${waiting === 1 ? "deploy waits" : "deploys wait"} for your approval` : "", unseen ? `${unseen} unseen in your inbox` : ""];
  const title = said.filter(Boolean).join("; ");
  return (
    <span class={waiting ? "thread-unread place-unread mention" : "thread-unread place-unread"} title={title} data-inbox-count={n} data-inbox-deploys={waiting || undefined}>
      <span class="thread-unread-dot" aria-hidden="true" />
      <span aria-hidden="true">{n > 99 ? "99+" : n}</span>
      <span class="place-unread-said">{title}</span>
    </span>
  );
}
