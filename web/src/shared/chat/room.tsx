// A room of control's chat (M123): its messages, the box to write in, and
// a message's pane chip. The chat page shows it (./chat-rooms), and so does
// the drawer beside a pane when its thread is opened from the pane. The
// room is control's, so it reads and posts with every machine asleep; only
// a pane's chip, when it opens, needs its machine. What it knows of
// machines and panes comes from the page (../platform-api): the shell's
// fleet, or nothing on the platform page.
//
// A quote is the lines someone chose from a pane, copied into the message
// (S41 decision 1): the page reads the pane end to end and the person
// trims it. Whether a chip opens is control's to say (`pane.opens`: the
// reader may reach that machine); one that doesn't is inert, and the quote
// is all they see.

import { Fragment, type ComponentChildren } from "preact";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { chat, type PaneQuote } from "../chat-store";
import type { ChatMessage, ChatRoom } from "../proto";
import { AgentsChip } from "./agents-chip";
import { authorLabel, isSystemLine } from "./agents";
import { ChatMarkdown } from "./chat-markdown";
import { mentionToken } from "../ui/markup";
import { MsgAvatar } from "./avatar";
import { TypingLine } from "./typing";
import { asksArugula, warmsIn } from "./warm";
import { BROADCAST_CONFIRM_OVER, broadcastOptions, usesBroadcast } from "./alerts";
// Saved items (#116) fill the toolbar's save, mark-unread and reminder hooks.
import "./saved-hooks";
import { EmojiPicker } from "./emoji-picker";
import { MessagePreviews } from "./previews";
import { hookFor, itemsOf, MsgItems, MsgMenu } from "./msg-actions";
import { QUICK } from "./reactions";
import { AttachButton, AttachChips, dragsFiles, filesOf, MessageFiles, useAttachments } from "./files-ui";
import { PinnedMark, SystemLine } from "./pins-ui";
// Pins (#119) fill the ⋯ menu's pin hook.
import "./pins-hooks";
import { longPress, mayReact, ReactionChips, ReactMenu } from "./reactions-ui";
import { findEmoji, rememberEmoji, useEmoji } from "./emoji";
import { useSubscribe } from "../ui/hooks";
import { useChatShown } from "./shown";
import { usePlatformApi, type PlatformApi } from "../platform-api";
import { ReplyTag, setThreadNav, ThreadFooter } from "./thread-ui";
import { AskMessage, askOfMessage } from "./ask-card";
import { answerVia, SaidAvatar, SaidHead, saidOf, SessionCardMessage, sessionOf } from "./session-card";
import { dmLabel } from "./members";
import { useStickToBottom } from "./stick";
import { BookmarkIcon } from "./icons";
import { Time } from "../ui/time";

// ---- machines, by control's id for them

/** A machine's name here, from control's id for it. */
export function machineName(api: PlatformApi, id: string | null | undefined, fallback = ""): string {
  if (!id) return fallback;
  return api.machine(id)?.name ?? fallback;
}

/** Whether a machine answers now. */
export function machineUp(api: PlatformApi, id: string | null | undefined): boolean {
  return !!api.machine(id)?.up;
}

/** What a room is called: `#name`, the other person, or the pane's title. */
export function roomLabel(r: ChatRoom, api: PlatformApi): string {
  if (r.kind === "channel") return r.name;
  if (r.kind === "dm") return dmLabel(r);
  if (r.kind === "pane" && r.target !== null) {
    const what = api.paneTitle(r.machine, r.target);
    // While its machine sleeps the fleet may not know it: the last title
    // this browser saw.
    return `%${r.target}${titled(r.id, what) ? ` ${titled(r.id, what)}` : ""}`;
  }
  if (r.kind === "session" && r.target !== null) {
    return titled(r.id, api.sessionName(r.machine, r.target)) || r.name;
  }
  return r.name;
}

const TITLES_KEY = "chat.roomTitles";
let titles: Record<string, string> | null = null;

/** A pane's or session's title for its room: `now` when known (kept), else
 * the last one kept in this browser. */
function titled(room: string, now: string): string {
  if (!titles) {
    try {
      titles = JSON.parse(localStorage.getItem(TITLES_KEY) ?? "{}") as Record<string, string>;
    } catch {
      titles = {};
    }
  }
  if (!now) return titles[room] ?? "";
  if (titles[room] !== now) {
    titles[room] = now;
    try {
      localStorage.setItem(TITLES_KEY, JSON.stringify(titles));
    } catch {
      // kept for this page only
    }
  }
  return now;
}

/** A room's mark: # a channel, @ a DM, ↳ a pane, ▤ a session, ◇ an
 * object's thread (M196), ⌂ an archive. */
export function roomSigil(r: ChatRoom): string {
  if (r.kind === "object") return "◇";
  return r.kind === "channel" ? "#" : r.kind === "dm" ? "@" : r.kind === "pane" ? "↳" : r.kind === "session" ? "▤" : "⌂";
}

// ---- agents (M197)

/** A message's picture: its author's, or its agent's machine's (M197). */
export function AuthorAvatar({ m }: { m: ChatMessage }) {
  return m.agent ? (
    <MsgAvatar who={`agent:${m.agent.machine}`} name={m.agent.machine_name || "agent"} account={m.author || undefined} agent={m.agent.machine} dot />
  ) : (
    <MsgAvatar who={m.author || "gone"} name={m.author ? m.name || "someone" : "a deleted account"} account={m.author || undefined} dot />
  );
}

/** Whose agents reach a room, for a host with no header of its own (an
 * issue, a run, the shell's thread drawer): the same chip the chat page
 * has in its header (./agents-chip, #122), alone on a slim line, and no
 * line at all when no agent reaches the room. */
function RoomAgents({ room }: { room: string }) {
  useSubscribe((fn) => chat.subscribe(fn));
  if (!chat.agents(room).length) return null;
  return (
    <div class="room-agents">
      <AgentsChip room={room} />
    </div>
  );
}

// ---- the room

function day(at: number): string {
  return new Date(at).toDateString();
}

/** "Today", "Yesterday", or the date, for the line between days. */
function dayLabel(at: number): string {
  const d = new Date(at);
  const today = new Date();
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString([], {
    weekday: "long",
    month: "long",
    day: "numeric",
    ...(d.getFullYear() === today.getFullYear() ? {} : { year: "numeric" }),
  });
}

function when(at: number): string {
  return new Date(at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

/** The time for the gutter, which is narrower than the head's: "10:42", no AM/PM. */
function whenShort(at: number): string {
  return when(at).replace(/\s?[AP]\.?M\.?$/i, "");
}

/** How near the bottom still counts as at it, for following new messages. */
const NEAR_BOTTOM = 80;

function draftOf(key: string): string {
  try {
    return sessionStorage.getItem(key) ?? "";
  } catch {
    return "";
  }
}

function keepDraft(key: string, text: string) {
  try {
    if (text) sessionStorage.setItem(key, text);
    else sessionStorage.removeItem(key);
  } catch {
    // storage refused: the draft lasts while the room is open
  }
}

/** What the room says before anything's in it. */
function emptyNote(r: ChatRoom): string {
  switch (r.kind) {
    case "channel":
      return r.private
        ? `This is the start of the private channel #${r.name}. Only the people in it read it; write @name to notify one of them.`
        : `This is the start of #${r.name}. Anyone in the team can join it; write @name to notify someone.`;
    case "dm":
      return r.others.length > 1 ? `Just you and ${r.others.map((p) => p.name).join(", ")}.` : `Just you and ${r.with?.name ?? "them"}.`;
    case "pane":
      return "Talk about this pane here. Whoever may read its thread sees it; write @name to notify one of them.";
    case "session":
      return "Talk about this session here; write @name to notify someone.";
    case "object":
      return "Talk about this here. Its project's members read it; write @name to notify one of them.";
    default:
      return "Nothing kept here.";
  }
}

/** Why someone can't write here. */
function readOnly(r: ChatRoom): string {
  if (r.kind === "archive") return "An archive: read only.";
  if (r.archived) return "This channel is archived: read only.";
  if (r.kind === "dm") return "Not enough of the people in it are left in the team: you can read this, not write.";
  return "You can read this room, not write in it.";
}

/** Something shown in a room among its messages, at its time. */
export interface Between {
  key: string;
  at: number;
  el: ComponentChildren;
}

/** A room's messages and what's between them, in time order: each thing
 * before the first message after it, the rest after the last. */
type Row = { m: ChatMessage; x?: undefined } | { x: Between; m?: undefined };

function rowsOf(msgs: ChatMessage[], between: Between[] = []): Row[] {
  const xs = [...between].sort((a, b) => a.at - b.at);
  const out: Row[] = [];
  let i = 0;
  for (const m of msgs) {
    while (i < xs.length && xs[i].at <= m.at) out.push({ x: xs[i++] });
    out.push({ m });
  }
  while (i < xs.length) out.push({ x: xs[i++] });
  return out;
}

/** A room's messages and the box to write in. */
export function RoomBody({
  room,
  phone,
  quote: initialQuote,
  focus,
  name,
  between,
  empty,
  chip,
  embedded,
}: {
  room: ChatRoom;
  phone: boolean;
  quote?: PaneQuote;
  /** Scroll to this message and flash it (a link's `&msg=`). */
  focus?: number;
  /** What the composer calls the room: "Message #general". */
  name: string;
  /** Other things in the room's time order, between its messages: an
   * issue's timeline (M201). */
  between?: Between[];
  /** What the room says before anything's in it, instead of its own. */
  empty?: string;
  /** The host's header shows whose agents reach the room (a chip), so the
   * room doesn't repeat it above its messages. */
  chip?: boolean;
  /** Part of a page about something else (an issue's or a run's): its box
   * to write in doesn't take focus, so the page opens at its top (UI audit
   * #144). */
  embedded?: boolean;
}) {
  useSubscribe((fn) => chat.subscribe(fn));
  // "Reply in thread" opens the thread where this room is shown (#106).
  const { nav } = usePlatformApi();
  useEffect(() => {
    setThreadNav(nav);
  }, [nav]);
  const [error, setError] = useState<string | null>(null);
  const [quote, setQuote] = useState<PaneQuote | undefined>(initialQuote);
  const [dropping, setDropping] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  // The message being edited (↑ in an empty composer picks my last).
  const [editing, setEditing] = useState<number | null>(null);
  // Whether the view is at the newest message, and the last one it was at
  // when it was: the "new messages" button counts what came after.
  const [atBottom, setAtBottom] = useState(true);
  const stuck = useRef(true);
  const [seenTo, setSeenTo] = useState(0);
  const loaded = chat.messages(room.id);
  const msgs = loaded?.list ?? null;
  // The red "New" line: the first message after the read mark when the
  // room opened (not mine), kept while it stays open.
  const newFrom = useRef<number | null | undefined>(undefined);
  if (newFrom.current === undefined && msgs) {
    newFrom.current = room.unread > 0 ? (msgs.find((m) => m.seq > room.read && !chat.mine(m) && !m.system)?.seq ?? null) : null;
  }
  // Marked unread from a message (#116): the line is there, and stays.
  const newAt = chat.heldAt(room.id) ?? newFrom.current;

  // Who has this room open, to control: this view, not the page; and only
  // while it's on screen (a chat block in another tab isn't).
  const opener = useRef({}).current;
  const shown = useChatShown();
  useEffect(() => {
    if (!shown) return;
    chat.setOpen(room.id, opener);
    return () => chat.setOpen(null, opener);
  }, [room.id, shown]);
  useEffect(() => {
    chat.load(room.id).catch((e: Error) => setError(e.message));
    // Fresh each time it opens: who reads it, and whose agents (M197).
    chat.detailOf(room.id, true).catch(() => {});
    setEditing(null);
    stuck.current = true;
    setAtBottom(true);
    setSeenTo(0);
  }, [room.id]);

  // What's shown is read; the newest stays in view (or the message a link
  // names, once).
  const last = msgs?.at(-1)?.seq;
  const focused = useRef<number | null>(null);
  useStickToBottom(list, () => stuck.current && focused.current === null);
  const topBefore = useRef<{ h: number; first?: number } | null>(null);
  useLayoutEffect(() => {
    const el = list.current;
    if (!el || !msgs) return;
    // Older messages came in above: keep the view where it was.
    const kept = topBefore.current;
    if (kept && kept.first !== msgs[0]?.seq) {
      el.scrollTop += el.scrollHeight - kept.h;
      topBefore.current = null;
      return;
    }
    if (focus !== undefined && focused.current !== focus) {
      const m = el.querySelector<HTMLElement>(`[data-msg="${focus}"]`);
      if (m) {
        focused.current = focus;
        m.scrollIntoView({ block: "center" });
        m.classList.add("flash");
        setTimeout(() => m.classList.remove("flash"), 1600);
        return;
      }
      // Further back than what's loaded: fetch older until it's there.
      if (loaded?.older && msgs.length && focus < msgs[0].seq) void chat.older(room.id);
    }
    // Follow new messages only from the bottom (or my own): someone reading
    // further up isn't pulled away.
    const lastMsg = msgs.at(-1);
    if (focused.current === null && (stuck.current || (lastMsg && chat.mine(lastMsg)))) {
      el.scrollTop = el.scrollHeight;
      stuck.current = true;
      setAtBottom(true);
    }
  }, [last, msgs?.[0]?.seq, focus, !!msgs]);
  useEffect(() => {
    if (atBottom && last !== undefined) setSeenTo(last);
  }, [atBottom, last]);
  const onScroll = () => {
    const el = list.current;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM;
    if (near !== stuck.current) {
      stuck.current = near;
      setAtBottom(near);
    }
  };
  const toNewest = () => {
    const el = list.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  };
  const current = chat.room(room.id) ?? room;
  useEffect(() => {
    // Scrolled up, what's below isn't read yet: the button says so.
    // A room marked unread on purpose (#116) stays so while it's open.
    const held = chat.heldAt(room.id) !== undefined;
    if (last !== undefined && atBottom && shown && !held && document.visibilityState === "visible" && (current.unread > 0 || last > current.read))
      void chat.read(room.id, last);
  }, [room.id, last, current.unread, atBottom, shown, chat.heldAt(room.id)]);
  // Esc: read it all, and to the newest. Not when something else is using
  // Esc (a menu, a picker, an edit, the mention list, a dialog), and Shift+Esc
  // is everything (rooms.tsx).
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.shiftKey || e.defaultPrevented || e.isComposing || last === undefined) return;
      if (document.querySelector("[data-react-menu], [data-msg-menu], [data-emoji-picker], .mention-pick, .menu, .prompt")) return;
      if ((e.target as Element | null)?.closest(".msg-edit")) return;
      const c = chat.room(room.id);
      if (!c || (c.unread === 0 && c.read >= last && stuck.current)) return;
      void chat.read(room.id, last).catch(() => {});
      toNewest();
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [room.id, last]);
  const unseen = msgs ? msgs.filter((m) => m.seq > seenTo && m.deleted === null && !chat.mine(m)).length : 0;
  // My last message that I may rewrite: not an agent's, not an archive's.
  const lastOwn = msgs?.findLast((m) => m.deleted === null && !m.imported && !!m.author && m.author === chat.me);

  const more = () => {
    const el = list.current;
    topBefore.current = { h: el?.scrollHeight ?? 0, first: msgs?.[0]?.seq };
    void chat.older(room.id);
  };

  return (
    <>
      {!chip && <RoomAgents room={room.id} />}
      <div
        class={`thread-list${dropping ? " dropping" : ""}`}
        ref={list}
        data-room={room.id}
        onScroll={onScroll}
        {...(current.writable ? dropTarget(setDropping) : {})}
      >
        {loaded?.older && (
          <button class="chat-older" onClick={more} data-room-older>
            Earlier messages
          </button>
        )}
        {msgs === null && !error && <p class="thread-empty">Loading…</p>}
        {msgs?.length === 0 && !between?.length && <p class="thread-empty">{empty ?? emptyNote(current)}</p>}
        {msgs &&
          rowsOf(msgs, between).map((row, i, rows) => {
            const at = row.m ? row.m.at : row.x.at;
            const before = rows[i - 1];
            const beforeAt = before && (before.m ? before.m.at : before.x.at);
            const newDay = beforeAt === undefined || day(beforeAt) !== day(at);
            const dayLine = newDay && (
              <div class="msg-day" role="separator">
                <span>{dayLabel(at)}</span>
              </div>
            );
            if (row.x) {
              return (
                <Fragment key={`x:${row.x.key}`}>
                  {dayLine}
                  {row.x.el}
                </Fragment>
              );
            }
            const m = row.m;
            // A message follows on from the one before it, unless something
            // else came between them.
            const prev = !newDay && before?.m && !before.m.system && !m.system && m.seq !== newAt ? before.m : undefined;
            return (
              <Fragment key={m.seq}>
                {dayLine}
                {m.seq === newAt && (
                  <div class="msg-new" role="separator" data-new-line>
                    <span>New</span>
                  </div>
                )}
                <RoomMessage
                  room={current}
                  m={m}
                  prev={prev}
                  reply={(q) => composeRef.current?.reply(q)}
                  setError={setError}
                  editing={editing === m.seq}
                  setEditing={(on) => {
                    setEditing(on ? m.seq : null);
                    // Back to the composer once the edit is saved or cancelled.
                    if (!on) composeRef.current?.focus();
                  }}
                  relative={embedded}
                />
              </Fragment>
            );
          })}
        {!atBottom && (
          <div class="jump-newest">
            <button type="button" data-jump-newest onClick={toNewest}>
              {unseen > 0 ? `↓ ${unseen} new ${unseen === 1 ? "message" : "messages"}` : "↓ Jump to newest"}
            </button>
          </div>
        )}
      </div>
      {error && <p class="thread-error">{error}</p>}
      {current.writable ? (
        <RoomCompose room={current} phone={phone} quote={quote} setQuote={setQuote} setError={setError} name={name} editLast={lastOwn ? () => setEditing(lastOwn.seq) : undefined} still={embedded} />
      ) : (
        <p class="thread-note" data-room-readonly>
          {readOnly(current)}
        </p>
      )}
    </>
  );
}

// The composer's handle for "Quote in reply" (one is shown at a time), and
// the thread panel's own beside it (#106).
type ComposeHandle = { current: { reply(text: string): void; focus(): void; attach(files: File[]): void } | null };
const composeRef: ComposeHandle = { current: null };
export const threadComposeRef: ComposeHandle = { current: null };

/** One message: its author and time, its text, the pane it names, and its
 * tools (quote in reply, copy link, edit and delete for its author). */
export function RoomMessage({
  room,
  m,
  prev,
  reply,
  setError,
  editing,
  setEditing,
  inThread = false,
  relative = false,
}: {
  room: ChatRoom;
  m: ChatMessage;
  prev?: ChatMessage;
  reply: (text: string) => void;
  setError: (e: string | null) => void;
  editing: boolean;
  setEditing: (on: boolean) => void;
  /** Shown in a thread's panel (#106): no thread of its own to open there. */
  inThread?: boolean;
  /** On a page about something else (an issue's, a run's): the head says
   * how long ago, as the page's timeline does (UI audit #182). */
  relative?: boolean;
}) {
  const head =
    !prev || prev.author !== m.author || prev.agent?.machine !== m.agent?.machine || prev.via !== m.via || m.at - prev.at > 5 * 60_000;
  const mine = chat.mine(m);
  // Mine or my agent's: I may delete it, not rewrite what an agent said.
  const owned = !!m.author && m.author === chat.me;
  const forMe = m.mentions.includes(chat.me);
  // The same function while it's for the same people: Markdown parses again for a new one.
  const landed = useCallback((t: string) => forMe && chat.isMe(t), [forMe]);
  const [copied, setCopied] = useState(false);
  // Which of its pop-ups is open: the emoji menu, the ⋯ menu, or the sheet
  // a long-press opens on a phone (the emoji and the menu's items together).
  const [panel, setPanel] = useState<"react" | "menu" | "sheet" | null>(null);
  const may = mayReact(room, m);
  const name = authorLabel(m);
  const link = usePlatformApi().nav.roomLink(room.id, m.seq, m.parent);
  const close = useCallback(() => setPanel(null), []);
  const ctx = { room, m };
  const thread = inThread ? undefined : hookFor("thread", ctx);
  const save = hookFor("save", ctx);
  const items = itemsOf(ctx, {
    quote: room.writable ? () => reply(m.text || m.quote || "") : undefined,
    copied,
    copy: () => {
      void navigator.clipboard?.writeText(link).then(
        () => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        },
        () => {},
      );
    },
    edit: mine && room.writable ? () => setEditing(true) : undefined,
    remove: owned
      ? () => {
          chat.remove(room.id, m.seq).catch((e: Error) => setError(e.message));
        }
      : undefined,
  });
  if (isSystemLine(m)) {
    return (
      <div class="msg-system" data-msg={m.seq} data-system role="note">
        <span>{m.text}</span>
        <time title={new Date(m.at).toLocaleString()}>{when(m.at)}</time>
      </div>
    );
  }
  if (m.system) return <SystemLine room={room} m={m} />;
  // M257: an agent session's card, drawn live in place of its message.
  if (sessionOf(m) && m.deleted === null) return <SessionCardMessage room={room} m={m} />;
  // Arugula #214: what a session still needs from its asker, as a form.
  if (askOfMessage(m) && m.deleted === null) return <AskMessage room={room} m={m} />;
  if (m.deleted !== null) {
    return (
      <div class="thread-msg first deleted" data-msg={m.seq}>
        <div class="msg-gutter" />
        <div class="msg-body">
          <p class="thread-text dim">This message was deleted.</p>
          {!inThread && <ThreadFooter room={room} m={m} />}
        </div>
      </div>
    );
  }
  // Arugula #215: what an agent session said (its answer, a review), drawn
  // as the session's, apart from people's.
  const said = saidOf(m);
  return (
    <div
      class={`thread-msg${head ? " first" : ""}${mine ? " mine" : ""}${forMe ? " for-me" : ""}${m.agent ? " by-agent" : ""}${said ? " by-session" : ""}`}
      data-msg={m.seq}
      data-author={m.author}
      data-agent={m.agent?.machine}
      data-said={said ?? undefined}
      {...longPress(() => setPanel("sheet"))}
    >
      <div class="msg-gutter">
        {head ? (
          said ? (
            <SaidAvatar ses={said} />
          ) : (
            <AuthorAvatar m={m} />
          )
        ) : (
          <time class="msg-hover-time" title={new Date(m.at).toLocaleString()}>
            {whenShort(m.at)}
          </time>
        )}
        {/* Compact rows have no picture: the time takes the gutter. */}
        {head && (
          <time class="msg-hover-time compact-time" title={new Date(m.at).toLocaleString()}>
            {whenShort(m.at)}
          </time>
        )}
      </div>
      <div class="msg-body">
        {head && (
          <div class="thread-head">
            {said ? <SaidHead ses={said} project={room.project} /> : <span class="thread-name">{name}</span>}
            {m.agent && (
              <span class="msg-badge agent-badge" title={`Written by an agent on ${m.agent.machine_name || "a machine"}, through its owner's grant`}>
                agent
              </span>
            )}
            {m.imported && (
              <span class="msg-badge" title={`Brought in from its machine's own thread, where it was ${m.imported}'s`}>
                imported
              </span>
            )}
            {m.via && !said && (
              <span
                class="msg-badge via-badge"
                title={answerVia(m, name)?.title ?? `Posted for ${name} by ${m.via}, where they wrote it`}
                data-via={m.via}
              >
                via {answerVia(m, name)?.label ?? m.via}
              </span>
            )}
            {relative ? <Time at={m.at} long /> : <time title={new Date(m.at).toLocaleString()}>{when(m.at)}</time>}
          </div>
        )}
        {!editing && <ReplyTag room={room} m={m} inThread={inThread} />}
        {editing ? (
          <EditBox
            text={m.text}
            done={async (text) => {
              if (text !== null && text !== m.text) {
                try {
                  await chat.edit(room.id, m.seq, text);
                } catch (e) {
                  setError((e as Error).message);
                }
              }
              setEditing(false);
            }}
          />
        ) : (
          m.text && <ChatMarkdown text={m.text} isMe={chat.isMe} landed={landed} />
        )}
        {m.edited !== null && !editing && <span class="thread-tag" title={new Date(m.edited).toLocaleString()}>(edited)</span>}
        <PinnedMark m={m} />
        <MessageFiles files={m.files} />
        {!editing && m.text && <MessagePreviews room={room.id} seq={m.seq} text={m.text} mine={owned} />}
        {m.pane && <PaneChip pane={m.pane} quote={m.quote} />}
        {!m.pane && m.quote && (
          <div class="thread-quote">
            <pre>{m.quote}</pre>
          </div>
        )}
        <ReactionChips room={room} m={m} open={() => setPanel("react")} setError={setError} />
        {!inThread && <ThreadFooter room={room} m={m} />}
      </div>
      <div class={panel === "react" || panel === "menu" ? "msg-tools open" : "msg-tools"} role="toolbar" aria-label="Message actions">
        {may && (
          <>
            {QUICK.slice(0, 3).map((e) => (
              <button
                key={e}
                class="msg-quick"
                title={`React with ${e}`}
                aria-label={`React with ${e}`}
                aria-pressed={!!m.reactions?.some((c) => c.emoji === e && c.mine)}
                data-tool-quick={e}
                onClick={() => chat.react(room.id, m.seq, e, !m.reactions?.some((c) => c.emoji === e && c.mine)).catch((x: Error) => setError(x.message))}
              >
                {e}
              </button>
            ))}
            <button title="Add a reaction" aria-label="Add a reaction" aria-expanded={panel === "react"} data-react-button onClick={() => setPanel(panel === "react" ? null : "react")}>
              ☺
            </button>
          </>
        )}
        {thread && (
          <button title="Reply in thread" aria-label="Reply in thread" data-msg-thread onClick={() => thread.run(ctx)}>
            ↩
          </button>
        )}
        {save && (
          <button
            title={save.label?.(ctx) ?? "Save for later"}
            aria-label={save.label?.(ctx) ?? "Save for later"}
            aria-pressed={save.on?.(ctx)}
            data-msg-save
            onClick={() => save.run(ctx)}
          >
            {/* A bookmark, not ☆: that's a room's star (UI audit #169). */}
            <BookmarkIcon on={save.on?.(ctx)} />
          </button>
        )}
        <button title="More actions" aria-label="More actions" aria-haspopup="menu" aria-expanded={panel === "menu"} data-msg-more onClick={() => setPanel(panel === "menu" ? null : "menu")}>
          ⋯
        </button>
      </div>
      {panel === "menu" && <MsgMenu items={items} close={close} />}
      {panel === "react" && may && <ReactMenu room={room} m={m} close={close} setError={setError} />}
      {panel === "sheet" && (
        <ReactMenu room={room} m={m} close={close} setError={setError} react={may}>
          {thread && (
            <button type="button" role="menuitem" class="msg-item" data-msg-thread onClick={() => (close(), thread.run(ctx))}>
              <span class="msg-item-icon" aria-hidden="true">↩</span>Reply in thread
            </button>
          )}
          {save && (
            <button type="button" role="menuitem" class="msg-item" aria-pressed={save.on?.(ctx)} data-msg-save onClick={() => (close(), save.run(ctx))}>
              <span class="msg-item-icon" aria-hidden="true">
                <BookmarkIcon on={save.on?.(ctx)} />
              </span>
              {save.label?.(ctx) ?? "Save for later"}
            </button>
          )}
          <MsgItems items={items} done={close} />
        </ReactMenu>
      )}
    </div>
  );
}

/** A room accepts files dropped on it (#107): the handlers for a drop
 * target, and whether a drag is over it. */
function dropTarget(over: (v: boolean) => void) {
  return {
    onDragOver: (e: DragEvent) => {
      if (!dragsFiles(e)) return;
      e.preventDefault();
      over(true);
    },
    onDragLeave: (e: DragEvent) => {
      // Moving between the target's children isn't leaving it.
      if (!(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node | null)) over(false);
    },
    onDrop: (e: DragEvent) => {
      over(false);
      const dropped = filesOf(e.dataTransfer);
      if (!dropped.length) return;
      e.preventDefault();
      composeRef.current?.attach(dropped);
    },
  };
}

/** Whether the caret is inside a ``` block that hasn't closed: Enter there is
 * a new line in the code, and Ctrl/Cmd+Enter sends. */
function inOpenFence(text: string, caret: number): boolean {
  return ((text.slice(0, caret).match(/^ {0,3}(?:```|~~~)/gm) ?? []).length & 1) === 1;
}

function EditBox({ text: start, done }: { text: string; done: (t: string | null) => void }) {
  const [text, setText] = useState(start);
  const input = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    input.current?.focus();
    input.current?.scrollIntoView({ block: "nearest" });
  }, []);
  return (
    <div class="msg-edit">
      <textarea
        ref={input}
        value={text}
        aria-label="Edit message"
        onInput={(e) => setText((e.target as HTMLTextAreaElement).value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") done(null);
          if (e.key === "Enter" && !e.shiftKey && !e.isComposing && !inOpenFence(text, (e.target as HTMLTextAreaElement).selectionStart)) {
            e.preventDefault();
            done(text.trim() ? text : null);
          }
        }}
      />
      <span class="dim">Enter saves, Shift+Enter is a new line, Esc cancels</span>
    </div>
  );
}

/** The pane a message names, with its quote: opens it where the reader
 * may (control says), else shows only what was quoted. */
function PaneChip({ pane, quote }: { pane: NonNullable<ChatMessage["pane"]>; quote: string | null }) {
  const api = usePlatformApi();
  useSubscribe((fn) => api.subscribe(fn));
  const where = `%${pane.pane} on ${pane.machine_name || "a machine"}`;
  // One that would open, on a machine that's asleep: marked, not just said
  // on hover.
  const asleep = pane.opens && !machineUp(api, pane.machine);
  const from = (
    <span class="thread-quote-from">
      <span class="chat-pane-chip" data-pane-chip={`${pane.machine}/${pane.pane}`} data-opens={pane.opens ? "1" : "0"} data-asleep={asleep ? "1" : undefined}>
        ↳ {where}
      </span>
      {!pane.opens && <span class="dim"> · not a pane you can open</span>}
      {asleep && <span class="chat-pane-asleep"> · asleep</span>}
    </span>
  );
  if (!pane.opens) {
    return (
      <div class="thread-quote inert" title="Not a pane you can open: the quote is what was shared">
        {from}
        {quote && <pre>{quote}</pre>}
      </div>
    );
  }
  return (
    <button
      class={asleep ? "thread-quote asleep" : "thread-quote"}
      title={!asleep ? "Show it in the pane" : `${pane.machine_name} is asleep: the quote is all there is for now`}
      aria-label={`Open ${where}`}
      onClick={() => api.goToPane(pane.machine, pane.pane, quote)}
    >
      {from}
      {quote && <pre>{quote}</pre>}
    </button>
  );
}

// ---- writing

/** Whom `@` reaches here, but me, as tokens control resolves: control
 * matches a login, a whole name, or the one word a team's roster calls
 * someone. A name with spaces can't be typed after `@`, so it's their
 * roster word, from the teams this page knows. */
type Mentionable = { token: string; name: string; who: string; note?: string };

function mentionables(api: PlatformApi, room: string): Mentionable[] {
  const out: Mentionable[] = [];
  const words = new Map<string, string>();
  for (const t of api.teams()) for (const m of t.roster.members) if (m.name) words.set(m.account, m.name);
  for (const p of chat.people(room)) {
    if (p.account === chat.me || !p.name) continue;
    const name = p.name.trim();
    const token = (/\s/.test(name) ? (words.get(p.account) ?? mentionToken(name)) : name).toLowerCase();
    if (token) out.push({ token, name: p.name, who: p.account });
  }
  return out;
}

export function RoomCompose({
  room,
  phone,
  quote,
  setQuote,
  setError,
  name,
  editLast,
  thread,
  still,
}: {
  room: ChatRoom;
  phone: boolean;
  quote?: PaneQuote;
  setQuote: (q: PaneQuote | undefined) => void;
  setError: (e: string | null) => void;
  name: string;
  /** Edit my last message (↑ in an empty box), if I have one. */
  editLast?: () => void;
  /** Writes a reply in the thread of this message (#106), with a draft of
   * its own and "Also send to the channel". */
  thread?: { parent: number };
  /** Doesn't take focus when it opens (`RoomBody`'s `embedded`). */
  still?: boolean;
}) {
  const key = thread ? `chat.draft:thread:${room.id}:${thread.parent}` : `chat.draft:room:${room.id}`;
  const handle = thread ? threadComposeRef : composeRef;
  const [also, setAlso] = useState(false);
  const [text, setText] = useState(() => draftOf(key));
  const [sending, setSending] = useState(false);
  const [pick, setPick] = useState(0);
  const [missed, setMissed] = useState<string[]>([]);
  const [picking, setPicking] = useState(false);
  const [emoji, setEmoji] = useState(false);
  const [dropping, setDropping] = useState(false);
  const files = useAttachments(room.id, setError);
  // "Notify anyway" (#115): for the next message only.
  const [anyway, setAnyway] = useState(false);
  const paused = room.kind === "dm" && !!room.with?.dnd;
  // Quoting reads a pane on a machine of yours: none, no button.
  const api = usePlatformApi();
  useSubscribe((fn) => api.subscribe(fn));
  const canQuote = !thread && !!api.quote?.can();
  const QuotePicker = api.quote?.Picker;
  const input = useRef<HTMLTextAreaElement>(null);
  useEffect(() => keepDraft(key, text), [key, text]);
  useEffect(() => {
    if (!phone && !still) input.current?.focus();
  }, [phone, still]);
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, innerHeight * 0.4)}px`;
  }, [text]);
  useEffect(() => {
    handle.current = {
      reply(q: string) {
        setText((t) => `${q.split("\n").map((l) => `> ${l}`).join("\n")}\n${t}`);
        input.current?.focus();
      },
      focus: () => input.current?.focus(),
      attach: files.add,
    };
    return () => {
      handle.current = null;
    };
  }, [files.add]);

  const caret = input.current?.selectionStart ?? text.length;
  const typing = /(?:^|[^\w])@([\w.-]*)$/.exec(text.slice(0, caret));
  const options: Mentionable[] = typing
    ? [
        ...mentionables(api, room.id)
          .filter((p) => p.token.startsWith(typing[1].toLowerCase()) || p.name.toLowerCase().startsWith(typing[1].toLowerCase()))
          .slice(0, 6),
        // `@channel` and `@here` after the people (#114).
        ...broadcastOptions(typing[1]).map((b) => ({ token: b.token, name: `@${b.token}`, who: "", note: b.note })),
      ]
    : [];
  const whole = !!typing && options.some((p) => p.token === typing[1].toLowerCase());
  const choose = (token: string) => {
    const start = caret - typing![1].length;
    setText(`${text.slice(0, start)}${token} ${text.slice(caret)}`);
    setPick(0);
    placeCaret(start + token.length + 1, true);
  };

  // Where the caret goes once a change to the text has drawn: right after the
  // render, not on the next frame, which a page in the background doesn't
  // get until something else draws it (and which then moved a later caret).
  const caretAfter = useRef<{ at: number; focus: boolean } | null>(null);
  const placeCaret = (at: number, focus: boolean) => {
    caretAfter.current = { at, focus };
  };
  useLayoutEffect(() => {
    const c = caretAfter.current;
    const el = input.current;
    if (!c || !el) return;
    caretAfter.current = null;
    el.setSelectionRange(c.at, c.at);
    if (c.focus) el.focus();
  }, [text]);

  // `:jo` after a space or at the start: the shortcodes that begin so (two
  // letters, so a time like 10:3 doesn't ask). The table loads on the first.
  const coloned = /(?:^|[^\w:]):([\w+-]{2,})$/.exec(text.slice(0, caret));
  const table = useEmoji(!!coloned);
  const emojis = coloned && table && !options.length ? findEmoji(table, coloned[1], 8) : [];
  const chooseEmoji = (char: string) => {
    const start = caret - coloned![1].length - 1;
    rememberEmoji(char);
    setText(`${text.slice(0, start)}${char} ${text.slice(caret)}`);
    setPick(0);
    placeCaret(start + char.length + 1, true);
  };
  // From the picker: at the caret as it was when the picker opened (the box
  // has lost focus by the pick, and its selection with it in some browsers),
  // then just after each pick.
  const at = useRef<{ from: number; to: number } | null>(null);
  const openEmoji = () => {
    // The box blurred on the button's press and kept where it was (onBlur):
    // by the click, a rerender may have moved its caret to the end.
    const el = input.current;
    if (el && document.activeElement === el) at.current = { from: el.selectionStart, to: el.selectionEnd };
    setEmoji((v) => !v);
  };
  const insert = (char: string) => {
    const from = Math.min(at.current?.from ?? text.length, text.length);
    const to = Math.min(at.current?.to ?? from, text.length);
    at.current = { from: from + char.length, to: from + char.length };
    setText(`${text.slice(0, from)}${char}${text.slice(to)}`);
    // A phone keeps the keyboard down: the picker stays for the next one.
    if (!phone) setEmoji(false);
    placeCaret(from + char.length, !phone);
  };

  const send = async () => {
    if (sending || files.busy || (!text.trim() && !quote && !files.items.length)) return;
    if (files.ids === null) {
      setError("A file didn't upload: remove it, or wait for it");
      return;
    }
    // A big room is woken only on purpose (#114).
    if (usesBroadcast(text) && chat.people(room.id).length > BROADCAST_CONFIRM_OVER) {
      const n = chat.people(room.id).length - 1;
      if (!confirm(`This notifies ${n} people. Send it?`)) return;
    }
    setSending(true);
    setError(null);
    setMissed([]);
    try {
      const posted = thread
        ? await chat.reply(room.id, thread.parent, text, also, files.ids)
        : await chat.post(room.id, text, quote, files.ids, paused && anyway);
      setMissed(posted.unreached.map((w) => `Nobody here called ${w} can read this room`));
      setText("");
      chat.composed(room.id, thread?.parent);
      setQuote(undefined);
      setAlso(false);
      files.clear();
      setAnyway(false);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSending(false);
      input.current?.focus();
    }
  };

  return (
    <form
      class={`thread-compose${dropping ? " dropping" : ""}`}
      {...dropTarget(setDropping)}
      onSubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
      {!thread && <TypingLine room={room.id} />}
      {missed.map((n) => (
        <p key={n} class="thread-note unreached">
          {n}
        </p>
      ))}
      {picking && QuotePicker && (
        <QuotePicker
          close={() => setPicking(false)}
          take={(q) => {
            setQuote(q);
            setPicking(false);
            input.current?.focus();
          }}
        />
      )}
      {options.length > 0 && (
        <ul class="mention-pick" role="listbox" aria-label="People to mention">
          {options.map((p, i) => (
            <li
              key={p.who || p.token}
              role="option"
              aria-selected={i === pick % options.length}
              class={i === pick % options.length ? "picked" : ""}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(p.token);
              }}
            >
              {p.who ? <MsgAvatar who={p.who} name={p.name} account={p.who} /> : <span class="mention-all" aria-hidden="true">@</span>}
              <b>{p.who ? p.name : `@${p.token}`}</b>
              <span>{p.note ?? `@${p.token}`}</span>
            </li>
          ))}
        </ul>
      )}
      {paused && (
        <p class="thread-note compose-paused" data-compose-paused>
          {room.with?.name} has paused notifications.
          <button type="button" class="link" aria-pressed={anyway} data-notify-anyway onClick={() => setAnyway((v) => !v)}>
            {anyway ? "Will notify anyway" : "Notify anyway"}
          </button>
        </p>
      )}
      {emoji && <EmojiPicker pick={insert} close={() => setEmoji(false)} />}
      {emojis.length > 0 && (
        <ul class="mention-pick emoji-pick" role="listbox" aria-label="Emoji">
          {emojis.map((e, i) => (
            <li
              key={e.char}
              role="option"
              aria-selected={i === pick % emojis.length}
              class={i === pick % emojis.length ? "picked" : ""}
              onMouseDown={(ev) => {
                ev.preventDefault();
                chooseEmoji(e.char);
              }}
            >
              <span class="emoji-char">{e.char}</span>
              <b>:{e.names[0]}:</b>
            </li>
          ))}
        </ul>
      )}
      <div class={canQuote ? "compose-box with-tools" : "compose-box"}>
        {quote && (
          <div class="thread-quote pending" data-quote-pending>
            <span class="thread-quote-from">
              ↳ %{quote.pane} on {machineName(api, quote.machine, "a machine")}
            </span>
            <pre>{quote.text}</pre>
            <button type="button" title="Don't quote it" aria-label="Don't quote it" onClick={() => setQuote(undefined)}>
              ✕
            </button>
          </div>
        )}
        <AttachChips items={files.items} remove={files.remove} />
        <textarea
          ref={input}
          rows={1}
          value={text}
          aria-label={thread ? "Reply in thread" : `Message ${name}`}
          placeholder={thread ? "Reply…" : `Message ${name}`}
          onBlur={(e) => {
            const t = e.currentTarget as HTMLTextAreaElement;
            at.current = { from: t.selectionStart, to: t.selectionEnd };
          }}
          onPaste={(e) => {
            // A screenshot on the clipboard is a file; text pastes as text.
            const pasted = filesOf(e.clipboardData);
            if (!pasted.length) return;
            e.preventDefault();
            files.add(pasted);
          }}
          onInput={(e) => {
            setMissed([]);
            const v = (e.target as HTMLTextAreaElement).value;
            setText(v);
            // Never while it's empty; the store holds it to one in 3 s.
            // An `@arugula` being written warms the project's box (M262);
            // in a reply thread that's all the ping is.
            if (!v.trim()) chat.composed(room.id, thread?.parent);
            else chat.typing(room.id, warmsIn(room.kind) && asksArugula(v), thread?.parent);
          }}
          onKeyDown={(e) => {
            const n = options.length || emojis.length;
            if (n && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
              e.preventDefault();
              setPick((p) => (p + (e.key === "ArrowDown" ? 1 : n - 1)) % n);
              return;
            }
            if (emojis.length && (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && !e.isComposing))) {
              e.preventDefault();
              chooseEmoji(emojis[pick % emojis.length].char);
              return;
            }
            if (options.length && (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && !whole))) {
              e.preventDefault();
              choose(options[pick % options.length].token);
              return;
            }
            // Slack's: ↑ with nothing written (and nothing quoted) edits the last one.
            if (e.key === "ArrowUp" && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && !e.isComposing && !text && !quote && editLast) {
              e.preventDefault();
              editLast();
              return;
            }
            if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
              // In a ``` block Enter writes the next line; Ctrl/Cmd+Enter sends.
              if (!(e.ctrlKey || e.metaKey) && inOpenFence(text, caret)) return;
              e.preventDefault();
              void send();
            }
          }}
        />
        {canQuote && (<button type="button" class="compose-tool" title="Quote lines from a pane" aria-label="Quote a pane" aria-pressed={picking} data-quote-pane onClick={() => setPicking((p) => !p)}>
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
            <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
            <path d="M4 6l2 2-2 2M8 10h4" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </button>)}
        <AttachButton add={files.add} />
        <button type="button" class="compose-tool" title="Emoji" aria-label="Emoji" aria-pressed={emoji} data-emoji-button onClick={openEmoji}>
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
            <circle cx="8" cy="8" r="6.2" />
            <path d="M5.4 9.6c.6 1.1 1.5 1.6 2.6 1.6s2-.5 2.6-1.6" stroke-linecap="round" />
            <path d="M5.9 6.2v.6M10.1 6.2v.6" stroke-linecap="round" stroke-width="1.8" />
          </svg>
        </button>
        <button class="compose-send" type="submit" title="Send (Enter; Shift+Enter for a new line)" aria-label="Send" disabled={sending || files.busy || (!text.trim() && !quote && !files.items.length)}>
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <path d="M2 2.5l12 5.5-12 5.5 1.8-5.5L2 2.5zm1.8 5.5h6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
          </svg>
        </button>
      </div>
      {thread && (
        <label class="thread-also" data-thread-also>
          <input type="checkbox" checked={also} onChange={(e) => setAlso((e.currentTarget as HTMLInputElement).checked)} />
          Also send to {room.kind === "channel" ? `#${room.name}` : room.kind === "dm" ? "the conversation" : "the room"}
        </label>
      )}
    </form>
  );
}
