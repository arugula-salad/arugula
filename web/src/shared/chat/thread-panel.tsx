// The panel that reads one thread (#106): its first message, the replies
// under it, a composer of its own with "Also send to #channel", and a
// follow switch. Beside the room on a wide screen; the whole screen on a
// phone. It is the same message and the same composer the room has
// (./room), so a reply has its reactions, edit, delete and mentions.

import { BlockLoading } from "../ui/block-states";
import { Fragment } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { chat, type ChatStore } from "../chat-store";
import type { ChatRoom } from "../proto";
import { useSubscribe } from "../ui/hooks";
import { RoomCompose, RoomMessage, roomLabel, threadComposeRef, useRoomTokens } from "./room";
import { usePlatformApi } from "../platform-api";
import { newFrom, repliesOf } from "./threads";
import { Bar, BarTitle, BarWhere } from "../ui/bar";
import { savedAnchor, useAnchor } from "./use-anchor";
import { SeenMarks, ThreadAgents, useThreadAgents } from "./thread-agents";
import { ArrowIcon, ChevronIcon, CloseIcon, IconButton } from "../ui/icons";

/** The head's width under which Follow goes into ⋯ (Close stays). Measured:
 * the title (52px), Follow (75), Close (25), the gaps and padding fit in
 * 200px, and 300 leaves about 100px for the room's name beside them. */
const THREAD_FOLD = 300;

export function ThreadPanel({ store, room, parent, phone, focus, close }: { store: ChatStore; room: ChatRoom; parent: number; phone: boolean; focus?: number; close: () => void }) {
  useSubscribe((fn) => store.subscribe(fn));
  const api = usePlatformApi();
  const tokens = useRoomTokens(room.id);
  const t = store.threads.get(room.id, parent);
  const first = t?.parent ?? null;
  const replies = t?.replies ?? [];
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const list = useRef<HTMLDivElement>(null);
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
    line.current = null;
    store.threads.load(room.id, parent).catch((e: Error) => setError(e.message));
    // On a phone the room isn't drawn behind the thread: this is what's open.
    if (phone) store.setOpen(room.id);
    store.setOpenThread(parent);
    return () => store.setOpenThread(null);
  }, [key]);

  // The view keeps its place (#965), as the room's does (use-anchor); a
  // link's `&msg=` is scrolled to once.
  const last = replies.at(-1)?.seq;
  const focused = useRef<number | null>(null);
  const storeKey = `chat.scroll.${room.id}.${parent}`;
  const anchor = useAnchor(list, storeKey, setAtBottom);
  const prevLast = useRef<{ key: string; seq?: number }>({ key });
  useLayoutEffect(() => {
    const el = list.current;
    if (!el || !t) return;
    if (focus !== undefined && focused.current !== focus) {
      const m = el.querySelector<HTMLElement>(`[data-msg="${focus}"]`);
      if (m) {
        focused.current = focus;
        anchor.open(null);
        m.scrollIntoView({ block: "center" });
        anchor.capture();
        m.classList.add("flash");
        setTimeout(() => m.classList.remove("flash"), 1600);
        return;
      }
    }
    const lastMsg = replies.at(-1);
    if (!anchor.ready()) {
      // A thread with replies I haven't read opens at the bottom, where its
      // "New" line is; else where it was left.
      const kept = first?.thread && first.thread.following && first.thread.unread > 0 ? null : savedAnchor(storeKey);
      const gone = kept && "seq" in kept && kept.seq !== first?.seq && !replies.some((m) => m.seq === kept.seq);
      if (gone && t.older && kept.seq < (replies[0]?.seq ?? Infinity)) return void store.threads.older(room.id, parent);
      anchor.open(gone ? null : kept);
    } else if (prevLast.current.key === key && prevLast.current.seq !== undefined && last !== undefined && last > prevLast.current.seq && lastMsg && chat.mine(lastMsg)) {
      anchor.toBottom();
    }
    prevLast.current = { key, seq: last };
  }, [last, replies[0]?.seq, !!t, focus, key]);
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
  const newest = last ?? first?.seq;
  const agents = useThreadAgents(room.id, parent, newest);
  const follow = () => store.threads.follow(room.id, parent, !following).catch((e: Error) => setError(e.message));
  const toNewest = () => anchor.toBottom();
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
      <Bar
        class="chat-head"
        lead={
          phone ? (
            <IconButton label={`Back to ${label}`} class="chat-back" data-thread-back onClick={close}>
              <ChevronIcon dir="left" size="md" />
            </IconButton>
          ) : undefined
        }
        title={<BarTitle class="chat-title">Thread</BarTitle>}
        where={<BarWhere>{label}</BarWhere>}
        foldBelow={THREAD_FOLD}
        fold={
          first?.thread
            ? [
                {
                  key: "follow",
                  node: (
                    <button
                      class={following ? "chat-info bar-btn on thread-follow" : "chat-info bar-btn thread-follow"}
                      aria-pressed={following}
                      title={following ? "You follow this thread: you hear its replies. Stop following" : "Follow this thread: hear its replies"}
                      data-thread-follow
                      onClick={follow}
                    >
                      {following ? "Following" : "Follow"}
                    </button>
                  ),
                  item: { label: following ? "Stop following" : "Follow", run: follow },
                },
              ]
            : []
        }
        actions={
          !phone ? (
            <IconButton label="Close the thread" class="thread-close" data-thread-close onClick={close}>
              <CloseIcon size="sm" />
            </IconButton>
          ) : undefined
        }
      />
      <ThreadAgents agents={agents} newest={newest} phone={phone} />
      <div class="thread-list thread-replies" ref={list} onScroll={() => anchor.onScroll()}>
        {t === undefined && !error && <BlockLoading inline what="the thread" />}
        {error && <p class="thread-error">{error}</p>}
        {t?.older && (
          <button class="chat-older" data-thread-older onClick={() => {
              anchor.capture();
              void store.threads.older(room.id, parent);
            }}>
            Earlier replies
          </button>
        )}
        {first && (
          <RoomMessage room={room} m={first} reply={(q) => threadComposeRef.current?.reply(q)} setError={setError} editing={editing === first.seq} setEditing={(on) => setEditing(on ? first.seq : null)} inThread tokens={tokens} />
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
                tokens={tokens}
                reply={(q) => threadComposeRef.current?.reply(q)}
                setError={setError}
                editing={editing === m.seq}
                setEditing={(on) => {
                  setEditing(on ? m.seq : null);
                  if (!on) threadComposeRef.current?.focus();
                }}
                inThread
              />
              <SeenMarks agents={agents} m={m} />
            </Fragment>
          );
        })}
        {!atBottom && (
          <div class="jump-newest">
            <button type="button" data-jump-newest onClick={toNewest}>
              <ArrowIcon dir="down" size="xs" /> Jump to newest
            </button>
          </div>
        )}
      </div>
      {room.writable && first ? (
        <RoomCompose room={room} phone={phone} setQuote={() => {}} setError={setError} name="Reply" thread={{ parent }} threadAgents={agents} editLast={lastOwn ? () => setEditing(lastOwn.seq) : undefined} />
      ) : null}
    </aside>
  );
}
