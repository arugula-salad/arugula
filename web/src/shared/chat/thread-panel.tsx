// The panel that reads one thread (#106): its first message, the replies
// under it, a composer of its own with "Also send to #channel", and a
// follow switch. Beside the room on a wide screen; the whole screen on a
// phone. It is the same message and the same composer the room has
// (./room), so a reply has its reactions, edit, delete and mentions.

import { Fragment } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { chat, type ChatStore } from "../chat-store";
import type { ChatRoom } from "../proto";
import { useSubscribe } from "../ui/hooks";
import { RoomCompose, RoomMessage, roomLabel, threadComposeRef } from "./room";
import { usePlatformApi } from "../platform-api";
import { newFrom, repliesOf } from "./threads";
import { useStickToBottom } from "./stick";

/** How near the bottom still counts as at it, for following new replies. */
const NEAR_BOTTOM = 80;

export function ThreadPanel({ store, room, parent, phone, focus, close }: { store: ChatStore; room: ChatRoom; parent: number; phone: boolean; focus?: number; close: () => void }) {
  useSubscribe((fn) => store.subscribe(fn));
  const api = usePlatformApi();
  const t = store.threads.get(room.id, parent);
  const first = t?.parent ?? null;
  const replies = t?.replies ?? [];
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const stuck = useRef(true);
  const [atBottom, setAtBottom] = useState(true);
  // The red "New" line: the first reply after my mark when the thread
  // opened (not mine), kept while it stays open.
  const line = useRef<{ key: string; seq: number | null } | null>(null);
  const key = `${room.id}/${parent}`;
  if (first?.thread && line.current?.key !== key && t) {
    line.current = { key, seq: first.thread.following && first.thread.unread > 0 ? newFrom(replies, first.thread.read, (m) => chat.mine(m)) : null };
  }

  useEffect(() => {
    setError(null);
    setEditing(null);
    stuck.current = true;
    line.current = null;
    store.threads.load(room.id, parent).catch((e: Error) => setError(e.message));
    // On a phone the room isn't drawn behind the thread: this is what's open.
    if (phone) store.setOpen(room.id);
    store.setOpenThread(parent);
    return () => store.setOpenThread(null);
  }, [key]);

  // Follow new replies from the bottom (or my own); a link's `&msg=` is
  // scrolled to once.
  const last = replies.at(-1)?.seq;
  const focused = useRef<number | null>(null);
  useStickToBottom(list, () => stuck.current && focused.current === null);
  useLayoutEffect(() => {
    const el = list.current;
    if (!el || !t) return;
    if (focus !== undefined && focused.current !== focus) {
      const m = el.querySelector<HTMLElement>(`[data-msg="${focus}"]`);
      if (m) {
        focused.current = focus;
        m.scrollIntoView({ block: "center" });
        m.classList.add("flash");
        setTimeout(() => m.classList.remove("flash"), 1600);
        return;
      }
    }
    const lastMsg = replies.at(-1);
    if (focused.current === null && (stuck.current || (lastMsg && chat.mine(lastMsg)))) {
      el.scrollTop = el.scrollHeight;
      stuck.current = true;
      setAtBottom(true);
    }
  }, [last, !!t, focus]);
  const onScroll = () => {
    const el = list.current;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM;
    if (near !== stuck.current) {
      stuck.current = near;
      setAtBottom(near);
    }
  };
  // What's in view is read, by the thread's own mark.
  const mark = first?.thread?.read ?? 0;
  useEffect(() => {
    if (last !== undefined && atBottom && document.visibilityState === "visible" && first?.thread?.following && (first.thread.unread > 0 || last > mark)) {
      void store.threads.read(room.id, parent, last);
    }
  }, [key, last, atBottom, first?.thread?.unread, first?.thread?.following]);

  const following = !!first?.thread?.following;
  const lastOwn = replies.findLast((m) => m.deleted === null && !!m.author && m.author === chat.me && !m.agent);
  const label = room.kind === "channel" ? `#${room.name}` : roomLabel(room, api);
  const toNewest = () => list.current?.scrollTo({ top: list.current.scrollHeight });
  return (
    <aside
      class="chat-details chat-thread-panel"
      aria-label="Thread"
      data-thread-panel={parent}
      onKeyDown={(e) => {
        if (e.key !== "Escape" || e.defaultPrevented || e.isComposing) return;
        if (document.querySelector("[data-react-menu], [data-msg-menu], [data-emoji-picker], .mention-pick, .menu, .prompt")) return;
        if ((e.target as Element | null)?.closest(".msg-edit")) return;
        close();
      }}
    >
      <header class="chat-head">
        {phone && (
          <button class="chat-back" title={`Back to ${label}`} aria-label={`Back to ${label}`} data-thread-back onClick={close}>
            ‹
          </button>
        )}
        <div class="chat-title">
          <strong>Thread</strong>
          <span>{label}</span>
        </div>
        {first?.thread && (
          <button
            class={following ? "chat-info on thread-follow" : "chat-info thread-follow"}
            aria-pressed={following}
            title={following ? "You follow this thread: you hear its replies. Stop following" : "Follow this thread: hear its replies"}
            data-thread-follow
            onClick={() => store.threads.follow(room.id, parent, !following).catch((e: Error) => setError(e.message))}
          >
            {following ? "Following" : "Follow"}
          </button>
        )}
        {!phone && (
          <button class="thread-close" title="Close the thread" aria-label="Close the thread" data-thread-close onClick={close}>
            ✕
          </button>
        )}
      </header>
      <div class="thread-list thread-replies" ref={list} onScroll={onScroll}>
        {t === undefined && !error && <p class="thread-empty">Loading…</p>}
        {error && <p class="thread-error">{error}</p>}
        {t?.older && (
          <button class="chat-older" data-thread-older onClick={() => void store.threads.older(room.id, parent)}>
            Earlier replies
          </button>
        )}
        {first && (
          <RoomMessage room={room} m={first} reply={(q) => threadComposeRef.current?.reply(q)} setError={setError} editing={editing === first.seq} setEditing={(on) => setEditing(on ? first.seq : null)} inThread />
        )}
        {first && (
          <div class="msg-day thread-count" role="separator" data-thread-count>
            <span>{replies.length ? repliesOf(first.thread?.replies ?? replies.length) : "No replies yet"}</span>
          </div>
        )}
        {replies.map((m, i) => {
          // A reply follows on from the one before it, unless the New line is between.
          const joined = i > 0 && m.seq !== line.current?.seq ? replies[i - 1] : undefined;
          return (
            <Fragment key={m.seq}>
              {m.seq === line.current?.seq && (
                <div class="msg-new" role="separator" data-new-line>
                  <span>New</span>
                </div>
              )}
              <RoomMessage
                room={room}
                m={m}
                prev={joined}
                reply={(q) => threadComposeRef.current?.reply(q)}
                setError={setError}
                editing={editing === m.seq}
                setEditing={(on) => {
                  setEditing(on ? m.seq : null);
                  if (!on) threadComposeRef.current?.focus();
                }}
                inThread
              />
            </Fragment>
          );
        })}
        {!atBottom && (
          <div class="jump-newest">
            <button type="button" data-jump-newest onClick={toNewest}>
              ↓ Jump to newest
            </button>
          </div>
        )}
      </div>
      {room.writable && first ? (
        <RoomCompose room={room} phone={phone} setQuote={() => {}} setError={setError} name="Reply" thread={{ parent }} editLast={lastOwn ? () => setEditing(lastOwn.seq) : undefined} />
      ) : null}
    </aside>
  );
}
