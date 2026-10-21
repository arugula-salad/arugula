// Getting around the chat page (M75): search and Activity (your mentions
// and what needs you) over control's rooms (M123), and whether the
// switcher (Ctrl/Cmd+K, drawn by ./chat-rooms) is open.

import { useEffect, useRef, useState } from "preact/hooks";
import { useSubscribe } from "../../shared/ui/hooks";
import { Markup } from "../../shared/ui/markup";
import { MsgAvatar } from "../../shared/chat/avatar";
import type { ChatStore } from "../../shared/chat-store";
import type { ChatHit, ChatMessage, ChatRoom } from "../../shared/proto";
import { roomLabel, roomSigil, machineName } from "../../shared/chat/room";
import { usePlatformApi, type PlatformApi } from "../../shared/platform-api";
import type { ChatRoute } from "../../shared/chat/route";

function when(at: number): string {
  const d = new Date(at);
  return new Date().toDateString() === d.toDateString()
    ? d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
    : d.toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

function ViewHead({ title, sub, phone }: { title: string; sub?: string; phone: boolean }) {
  const { nav } = usePlatformApi();
  return (
    <header class="chat-head">
      {phone && (
        <button class="chat-back" title="Every thread" onClick={() => nav.openChat()}>
          ‹
        </button>
      )}
      <div class="chat-title">
        <strong>{title}</strong>
        {sub && <span>{sub}</span>}
      </div>
    </header>
  );
}

// ---- control's chat (M123): search and Activity over its rooms

/** A message found in a room, and a way there. */
function RoomHits({ store, hits }: { store: ChatStore; hits: { room: string; m: ChatMessage }[] }) {
  const api = usePlatformApi();
  return (
    <ul class="chat-hits">
      {hits.map(({ room, m }) => {
        const r = store.room(room);
        const unread = !!r && m.seq > r.read && !store.mine(m);
        return (
          <li key={`${room}/${m.seq}`}>
            <button class={unread ? "chat-hit unread" : "chat-hit"} data-chat-hit={m.seq} data-hit-room={room} onClick={() => api.nav.openRoom(room, m.seq)}>
              <span class="chat-hit-where">
                <span>{r ? whereOf(r, api) : "a room"}</span>
                <time>{when(m.at)}</time>
              </span>
              <span class="chat-hit-body">
                <MsgAvatar who={m.author || "gone"} name={m.name || "someone"} />
                <span class="chat-hit-text">
                  <b>{m.name || "someone"}</b>
                  {m.text ? <Markup text={m.text} /> : <p class="thread-text">{m.quote}</p>}
                </span>
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function whereOf(r: ChatRoom, api: PlatformApi): string {
  const name = `${roomSigil(r)} ${roomLabel(r, api)}`;
  return r.machine ? `${name} · ${machineName(api, r.machine, r.machine_name ?? "")}` : name;
}

/** Search over the rooms this person reads, by control (`GET
 * /api/chat/search`: words, `mea*`, `in:#ch`, `from:@login`,
 * `has:quote`). */
export function RoomSearchView({ store, q, phone }: { store: ChatStore; q: string; phone: boolean }) {
  const [hits, setHits] = useState<ChatHit[] | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    let live = true;
    setErr("");
    if (!q.trim()) return setHits([]);
    setHits(null);
    store.search(q).then(
      (r) => live && setHits(r.found),
      (e: Error) => live && (setErr(e.message), setHits([])),
    );
    return () => {
      live = false;
    };
  }, [q]);
  const n = hits?.length ?? 0;
  return (
    <section class="chat-thread chat-view" aria-label="Search" data-chat-search-view>
      <ViewHead title={q ? `Results for “${q}”` : "Search"} sub={q ? (hits === null ? "Looking…" : `${n >= 50 ? "50+" : n} ${n === 1 ? "message" : "messages"}`) : undefined} phone={phone} />
      {phone && <SearchField route={{ view: "search", q }} phone />}
      <div class="thread-list">
        {!q && <p class="thread-empty">Type to search every room you read. in:#channel, from:@someone and has:quote narrow it.</p>}
        {q && hits?.length === 0 && !err && <p class="thread-empty">No messages match.</p>}
        {err && <p class="thread-error">{err}</p>}
        <RoomHits store={store} hits={(hits ?? []).map((h) => ({ room: h.room, m: h.message }))} />
      </div>
    </section>
  );
}

/** How many rooms Activity reads for mentions of you. */
const ACTIVITY_ROOMS = 30;

/** What needs you on every machine, and messages that mention you. */
export function RoomActivityView({ store, phone }: { store: ChatStore; phone: boolean }) {
  const api = usePlatformApi();
  useSubscribe((fn) => api.subscribe(fn));
  useSubscribe((fn) => store.subscribe(fn));
  // Recent mentions, read or not: the newest page of the rooms that spoke
  // most recently (those with an unread mention first), not only those
  // with one unread.
  const recent = [...store.rooms]
    .filter((r) => r.last !== 0)
    .sort((a, b) => b.mentions - a.mentions || b.last - a.last)
    .slice(0, ACTIVITY_ROOMS)
    .map((r) => r.id);
  useEffect(() => {
    for (const id of recent) void store.load(id).catch(() => {});
  }, [recent.join(",")]);
  const hits: { room: string; m: ChatMessage }[] = [];
  for (const r of store.rooms) for (const m of store.messages(r.id)?.list ?? []) if (m.mentions.includes(store.me) && m.deleted === null) hits.push({ room: r.id, m });
  hits.sort((a, b) => b.m.at - a.m.at);
  // What needs you on the machines, where this page sees them (the shell's).
  const needs = api.needsYou?.count() ?? 0;
  const NeedsYou = api.needsYou?.View;
  return (
    <section class="chat-thread chat-view" aria-label="Activity" data-chat-activity-view>
      <ViewHead title="Activity" sub="What needs you on every machine, and mentions of you" phone={phone} />
      <div class="thread-list">
        {NeedsYou && <NeedsYou />}
        {hits.length === 0 && needs === 0 && <p class="thread-empty">Nothing for you yet. When an agent asks you something or finishes, or someone @mentions you, it's here.</p>}
        {hits.length > 0 && needs > 0 && <h3 class="activity-head">Messages</h3>}
        <RoomHits store={store} hits={hits.slice(0, 100)} />
      </div>
    </section>
  );
}

/** The bar's search field: results show as you type. On a phone (#889)
 * it heads the results instead, opened from the list's search button: it
 * takes the focus when it's empty, and emptying it stays on the results. */
export function SearchField({ route, phone }: { route: ChatRoute; phone?: boolean }) {
  const { nav } = usePlatformApi();
  const shown = route.view === "search" ? (route.q ?? "") : "";
  const [text, setText] = useState(shown);
  const timer = useRef<number | undefined>(undefined);
  const input = useRef<HTMLInputElement>(null);
  // Leaving the results (a channel picked) clears it.
  useEffect(() => {
    if (route.view !== "search") setText("");
  }, [route.view]);
  useEffect(() => () => clearTimeout(timer.current), []);
  useEffect(() => {
    if (phone && !shown) input.current?.focus();
  }, []);
  const go = (v: string, now = false) => {
    clearTimeout(timer.current);
    const run = () => (v.trim() || phone ? nav.openChatView("search", v.trim()) : route.view === "search" && nav.openChat());
    if (now) run();
    else timer.current = window.setTimeout(run, 200);
  };
  return (
    <input
      ref={input}
      class="chat-search"
      type="search"
      placeholder="Search chat"
      aria-label="Search chat"
      data-chat-search
      value={text}
      onInput={(e) => {
        const v = (e.target as HTMLInputElement).value;
        setText(v);
        go(v);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") go(text, true);
        if (e.key === "Escape") {
          setText("");
          go("", true);
          input.current?.blur();
        }
      }}
    />
  );
}

// ---- the switcher (Ctrl/Cmd+K)

let switcherOpen = false;
const switcherListeners = new Set<() => void>();

export function openSwitcher() {
  switcherOpen = true;
  switcherListeners.forEach((fn) => fn());
}

export function closeSwitcher() {
  switcherOpen = false;
  switcherListeners.forEach((fn) => fn());
}

/** Whether the switcher is open, kept current (control's chat, M123,
 * draws its own list of rooms in it). */
export function useSwitcherOpen(): boolean {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    switcherListeners.add(fn);
    return () => {
      switcherListeners.delete(fn);
      switcherOpen = false;
    };
  }, []);
  return switcherOpen;
}
