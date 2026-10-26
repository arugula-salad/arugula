// The Threads view (#106), next to Activity: the threads I follow, the
// most recently active first, each with its unread count. A row opens the
// thread beside its room; the corner stops following it.

import "./threads.css";
import { useEffect } from "preact/hooks";
import type { ChatStore } from "../chat-store";
import type { ChatThreadItem } from "../proto";
import { useSubscribe } from "../ui/hooks";
import { Markup } from "../ui/markup";
import { usePlatformApi } from "../platform-api";
import { authorLabel } from "./agents";
import { AuthorAvatar, roomLabel, roomSigil } from "./room";
import { footer, repliesOf } from "./threads";

function Row({ store, it }: { store: ChatStore; it: ChatThreadItem }) {
  const api = usePlatformApi();
  const room = store.room(it.room);
  const t = it.parent.thread;
  if (!t) return null;
  const m = it.parent;
  const unread = t.unread;
  return (
    <li class="thread-row-item">
      <button
        class={unread ? "chat-hit thread-row unread" : "chat-hit thread-row"}
        data-thread-row={m.seq}
        data-hit-room={it.room}
        onClick={() => api.nav.openRoom(it.room, undefined, m.seq)}
      >
        <span class="chat-hit-where">
          <span>{room ? `${roomSigil(room)} ${roomLabel(room, api)}` : "a room"}</span>
          <time>{footer(t, Date.now())}</time>
        </span>
        <span class="chat-hit-body">
          <AuthorAvatar m={m} />
          <span class={m.agent ? "chat-hit-text by-agent" : "chat-hit-text"}>
            <b>{authorLabel(m)}</b>
            {m.deleted !== null ? <p class="thread-text dim">This message was deleted.</p> : m.text ? <Markup text={m.text} /> : <p class="thread-text">{m.quote}</p>}
          </span>
        </span>
        {it.latest && (
          <span class="thread-row-latest">
            <b>{authorLabel(it.latest)}</b> {it.latest.text || "…"}
          </span>
        )}
        {unread > 0 && (
          <span class={t.mentions ? "chat-count mention thread-row-count" : "chat-count thread-row-count"} data-thread-row-unread>
            {t.mentions ? `@${unread}` : unread}
          </span>
        )}
      </button>
      <button
        class="thread-unfollow"
        title="Stop following this thread"
        aria-label="Stop following this thread"
        data-thread-unfollow={m.seq}
        onClick={() => void store.threads.follow(it.room, m.seq, false).catch((e: Error) => api.toast(e.message))}
      >
        Unfollow
      </button>
    </li>
  );
}

/** Followed threads over every room I read (`GET /api/chat/threads`). */
export function ThreadsView({ store, phone }: { store: ChatStore; phone: boolean }) {
  const { nav } = usePlatformApi();
  useSubscribe((fn) => store.subscribe(fn));
  useEffect(() => store.threads.watch(), []);
  const list = store.threads.list;
  // A thread I just stopped following goes at once.
  const shown = (list?.threads ?? []).filter((it) => it.parent.thread?.following !== false);
  const n = shown.length;
  return (
    <section class="chat-thread chat-view" aria-label="Threads" data-chat-threads-view>
      <header class="chat-head chat-view-head">
        {phone && (
          <button class="chat-back" title="Every room" onClick={() => nav.openChat()}>
            ‹
          </button>
        )}
        <div class="chat-title">
          <strong>Threads</strong>
          <span>{list ? (n ? `${repliesOf(shown.reduce((k, it) => k + (it.parent.thread?.replies ?? 0), 0))} in ${n} ${n === 1 ? "thread" : "threads"} you follow` : "Threads you follow") : "Loading…"}</span>
        </div>
      </header>
      <div class="thread-list">
        {list && n === 0 && <p class="thread-empty">Nothing yet. When you reply in a thread, or someone replies to your message, it is here, with what's new in it.</p>}
        <ul class="chat-hits">
          {shown.map((it) => (
            <Row key={`${it.room}/${it.parent.seq}`} store={store} it={it} />
          ))}
        </ul>
      </div>
    </section>
  );
}
