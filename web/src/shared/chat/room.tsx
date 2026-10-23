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
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { chat, type PaneQuote } from "../chat-store";
import type { ChatMessage, ChatRoom } from "../proto";
import { agentLines, authorLabel } from "./agents";
import { Markup, mentionToken } from "../ui/markup";
import { MsgAvatar } from "./avatar";
import { useSubscribe } from "../ui/hooks";
import { usePlatformApi, type PlatformApi } from "../platform-api";

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
  if (r.kind === "dm") return r.with?.name || "someone who left";
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
    <MsgAvatar who={`agent:${m.agent.machine}`} name={m.agent.machine_name || "agent"} />
  ) : (
    <MsgAvatar who={m.author || "gone"} name={m.author ? m.name || "someone" : "a deleted account"} />
  );
}

/** The line every room an agent grant reaches shows, to everyone in it. */
function RoomAgents({ room }: { room: string }) {
  const lines = agentLines(chat.agents(room));
  if (!lines.length) return null;
  return (
    <div class="room-agents" data-room-agents role="note">
      {lines.map((l) => (
        <p key={l}>{l}.</p>
      ))}
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
      return `This is the start of #${r.name}. Everyone in the team reads it; write @name to notify someone.`;
    case "dm":
      return `Just you and ${r.with?.name ?? "them"}.`;
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
  if (r.kind === "dm") return `${r.with?.name ?? "They"} left the team: you can read this, not write.`;
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
}) {
  useSubscribe((fn) => chat.subscribe(fn));
  const [error, setError] = useState<string | null>(null);
  const [quote, setQuote] = useState<PaneQuote | undefined>(initialQuote);
  const list = useRef<HTMLDivElement>(null);
  const loaded = chat.messages(room.id);
  const msgs = loaded?.list ?? null;
  // The red "New" line: the first message after the read mark when the
  // room opened (not mine), kept while it stays open.
  const newFrom = useRef<number | null | undefined>(undefined);
  if (newFrom.current === undefined && msgs) {
    newFrom.current = room.unread > 0 ? (msgs.find((m) => m.seq > room.read && !chat.mine(m))?.seq ?? null) : null;
  }

  useEffect(() => {
    chat.load(room.id).catch((e: Error) => setError(e.message));
    // Fresh each time it opens: who reads it, and whose agents (M197).
    chat.detailOf(room.id, true).catch(() => {});
    chat.setOpen(room.id);
    return () => chat.setOpen(null);
  }, [room.id]);

  // What's shown is read; the newest stays in view (or the message a link
  // names, once).
  const last = msgs?.at(-1)?.seq;
  const focused = useRef<number | null>(null);
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
    if (focused.current === null) el.scrollTop = el.scrollHeight;
  }, [last, msgs?.[0]?.seq, focus, !!msgs]);
  const current = chat.room(room.id) ?? room;
  useEffect(() => {
    if (last !== undefined && document.visibilityState === "visible" && (current.unread > 0 || last > current.read)) void chat.read(room.id, last);
  }, [room.id, last, current.unread]);

  const more = () => {
    const el = list.current;
    topBefore.current = { h: el?.scrollHeight ?? 0, first: msgs?.[0]?.seq };
    void chat.older(room.id);
  };

  return (
    <>
      <RoomAgents room={room.id} />
      <div class="thread-list" ref={list} data-room={room.id}>
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
            const prev = !newDay && before?.m && m.seq !== newFrom.current ? before.m : undefined;
            return (
              <Fragment key={m.seq}>
                {dayLine}
                {m.seq === newFrom.current && (
                  <div class="msg-new" role="separator" data-new-line>
                    <span>New</span>
                  </div>
                )}
                <RoomMessage room={current} m={m} prev={prev} reply={(q) => composeRef.current?.reply(q)} setError={setError} />
              </Fragment>
            );
          })}
      </div>
      {error && <p class="thread-error">{error}</p>}
      {current.writable ? (
        <RoomCompose room={current} phone={phone} quote={quote} setQuote={setQuote} setError={setError} name={name} />
      ) : (
        <p class="thread-note" data-room-readonly>
          {readOnly(current)}
        </p>
      )}
    </>
  );
}

// The composer's handle for "Quote in reply" (one is shown at a time).
const composeRef: { current: { reply(text: string): void } | null } = { current: null };

/** One message: its author and time, its text, the pane it names, and its
 * tools (quote in reply, copy link, edit and delete for its author). */
function RoomMessage({
  room,
  m,
  prev,
  reply,
  setError,
}: {
  room: ChatRoom;
  m: ChatMessage;
  prev?: ChatMessage;
  reply: (text: string) => void;
  setError: (e: string | null) => void;
}) {
  const head =
    !prev || prev.author !== m.author || prev.agent?.machine !== m.agent?.machine || prev.via !== m.via || m.at - prev.at > 5 * 60_000;
  const mine = chat.mine(m);
  // Mine or my agent's: I may delete it, not rewrite what an agent said.
  const owned = !!m.author && m.author === chat.me;
  const forMe = m.mentions.includes(chat.me);
  const [copied, setCopied] = useState(false);
  const [editing, setEditing] = useState(false);
  const name = authorLabel(m);
  const link = usePlatformApi().nav.roomLink(room.id, m.seq);
  if (m.deleted !== null) {
    return (
      <div class="thread-msg first deleted" data-msg={m.seq}>
        <div class="msg-gutter" />
        <div class="msg-body">
          <p class="thread-text dim">This message was deleted.</p>
        </div>
      </div>
    );
  }
  return (
    <div
      class={`thread-msg${head ? " first" : ""}${mine ? " mine" : ""}${forMe ? " for-me" : ""}${m.agent ? " by-agent" : ""}`}
      data-msg={m.seq}
      data-author={m.author}
      data-agent={m.agent?.machine}
    >
      <div class="msg-gutter">
        {head ? (
          <AuthorAvatar m={m} />
        ) : (
          <time class="msg-hover-time" title={new Date(m.at).toLocaleString()}>
            {when(m.at)}
          </time>
        )}
      </div>
      <div class="msg-body">
        {head && (
          <div class="thread-head">
            <span class="thread-name">{name}</span>
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
            {m.via && (
              <span class="msg-badge via-badge" title={`Posted for ${name} by ${m.via}, where they wrote it`} data-via={m.via}>
                via {m.via}
              </span>
            )}
            <time title={new Date(m.at).toLocaleString()}>{when(m.at)}</time>
          </div>
        )}
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
          m.text && <Markup text={m.text} isMe={chat.isMe} landed={(t) => forMe && chat.isMe(t)} />
        )}
        {m.edited !== null && !editing && <span class="thread-tag" title={new Date(m.edited).toLocaleString()}>(edited)</span>}
        {m.pane && <PaneChip pane={m.pane} quote={m.quote} />}
        {!m.pane && m.quote && (
          <div class="thread-quote">
            <pre>{m.quote}</pre>
          </div>
        )}
      </div>
      <div class="msg-tools" role="toolbar" aria-label="Message actions">
        {room.writable && (
          <button title="Quote in reply" aria-label="Quote in reply" data-msg-reply onClick={() => reply(m.text || m.quote || "")}>
            ❝
          </button>
        )}
        <button
          title={copied ? "Copied" : "Copy link"}
          aria-label="Copy link"
          data-msg-link
          onClick={() => {
            void navigator.clipboard?.writeText(link).then(
              () => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1200);
              },
              () => {},
            );
          }}
        >
          {copied ? "✓" : "🔗"}
        </button>
        {mine && room.writable && (
          <button title="Edit" aria-label="Edit message" data-msg-edit onClick={() => setEditing(true)}>
            ✎
          </button>
        )}
        {owned && (
          <button
            title="Delete"
            aria-label="Delete message"
            data-msg-delete
            onClick={() => {
              chat.remove(room.id, m.seq).catch((e: Error) => setError(e.message));
            }}
          >
            🗑
          </button>
        )}
      </div>
    </div>
  );
}

function EditBox({ text: start, done }: { text: string; done: (t: string | null) => void }) {
  const [text, setText] = useState(start);
  const input = useRef<HTMLTextAreaElement>(null);
  useEffect(() => input.current?.focus(), []);
  return (
    <div class="msg-edit">
      <textarea
        ref={input}
        value={text}
        aria-label="Edit message"
        onInput={(e) => setText((e.target as HTMLTextAreaElement).value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") done(null);
          if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
            e.preventDefault();
            done(text.trim() ? text : null);
          }
        }}
      />
      <span class="dim">Enter saves, Esc cancels</span>
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
function mentionables(api: PlatformApi, room: string): { token: string; name: string; who: string }[] {
  const out: { token: string; name: string; who: string }[] = [];
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

function RoomCompose({
  room,
  phone,
  quote,
  setQuote,
  setError,
  name,
}: {
  room: ChatRoom;
  phone: boolean;
  quote?: PaneQuote;
  setQuote: (q: PaneQuote | undefined) => void;
  setError: (e: string | null) => void;
  name: string;
}) {
  const key = `chat.draft:room:${room.id}`;
  const [text, setText] = useState(() => draftOf(key));
  const [sending, setSending] = useState(false);
  const [pick, setPick] = useState(0);
  const [missed, setMissed] = useState<string[]>([]);
  const [picking, setPicking] = useState(false);
  // Quoting reads a pane on a machine of yours: none, no button.
  const api = usePlatformApi();
  useSubscribe((fn) => api.subscribe(fn));
  const canQuote = !!api.quote?.can();
  const QuotePicker = api.quote?.Picker;
  const input = useRef<HTMLTextAreaElement>(null);
  useEffect(() => keepDraft(key, text), [key, text]);
  useEffect(() => {
    if (!phone) input.current?.focus();
  }, [phone]);
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, innerHeight * 0.4)}px`;
  }, [text]);
  useEffect(() => {
    composeRef.current = {
      reply(q: string) {
        setText((t) => `${q.split("\n").map((l) => `> ${l}`).join("\n")}\n${t}`);
        input.current?.focus();
      },
    };
    return () => {
      composeRef.current = null;
    };
  }, []);

  const caret = input.current?.selectionStart ?? text.length;
  const typing = /(?:^|[^\w])@([\w.-]*)$/.exec(text.slice(0, caret));
  const options = typing
    ? mentionables(api, room.id)
        .filter((p) => p.token.startsWith(typing[1].toLowerCase()) || p.name.toLowerCase().startsWith(typing[1].toLowerCase()))
        .slice(0, 6)
    : [];
  const whole = !!typing && options.some((p) => p.token === typing[1].toLowerCase());
  const choose = (token: string) => {
    const start = caret - typing![1].length;
    setText(`${text.slice(0, start)}${token} ${text.slice(caret)}`);
    setPick(0);
    requestAnimationFrame(() => {
      const at = start + token.length + 1;
      input.current?.setSelectionRange(at, at);
      input.current?.focus();
    });
  };

  const send = async () => {
    if (sending || (!text.trim() && !quote)) return;
    setSending(true);
    setError(null);
    setMissed([]);
    try {
      const posted = await chat.post(room.id, text, quote);
      setMissed(posted.unreached.map((w) => `Nobody here called ${w} can read this room`));
      setText("");
      setQuote(undefined);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSending(false);
      input.current?.focus();
    }
  };

  return (
    <form
      class="thread-compose"
      onSubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
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
              key={p.who}
              role="option"
              aria-selected={i === pick % options.length}
              class={i === pick % options.length ? "picked" : ""}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(p.token);
              }}
            >
              <MsgAvatar who={p.who} name={p.name} />
              <b>{p.name}</b>
              <span>@{p.token}</span>
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
        <textarea
          ref={input}
          rows={1}
          value={text}
          aria-label={`Message ${name}`}
          placeholder={`Message ${name}`}
          onInput={(e) => {
            setMissed([]);
            setText((e.target as HTMLTextAreaElement).value);
          }}
          onKeyDown={(e) => {
            if (options.length && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
              e.preventDefault();
              setPick((p) => (p + (e.key === "ArrowDown" ? 1 : options.length - 1)) % options.length);
              return;
            }
            if (options.length && (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && !whole))) {
              e.preventDefault();
              choose(options[pick % options.length].token);
              return;
            }
            if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
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
        <button class="compose-send" type="submit" title="Send (Enter)" aria-label="Send" disabled={sending || (!text.trim() && !quote)}>
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <path d="M2 2.5l12 5.5-12 5.5 1.8-5.5L2 2.5zm1.8 5.5h6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
          </svg>
        </button>
      </div>
    </form>
  );
}
