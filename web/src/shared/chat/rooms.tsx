// The chat page on control's chat (M123): a team's channels and DMs, and
// the threads on panes and sessions, all rooms that control holds. The
// sidebar lists them (each team's channels and DMs, then the threads by
// machine, then archives); the room is ./room's. Nothing here needs a
// machine to be up: a pane's thread reads and posts with its machine
// asleep, and only its chip waits for it.
//
// Routes: `#chat` (the newest unread room, or the newest), `#chat=<room>`
// and `#chat=<room>&msg=<n>`, `#chat=activity`, `#chat=search/<words>`.

import { useEffect, useRef, useState } from "preact/hooks";
import { chat, type ChatStore } from "../chat-store";
import { listedInChat } from "./unread";
import type { ChatRoom } from "../proto";
import { MaintenanceNote } from "../ui/maintenance-note";
import { usePhone, useSubscribe } from "../ui/hooks";
import { askText, openMenu, type MenuItem } from "../ui/menu";
import type { ChatRoute } from "./route";
import { RoomActivityView, RoomSearchView, SearchField, openSwitcher, useSwitcherOpen, closeSwitcher } from "./find";
import { MsgAvatar } from "./avatar";
import { WebPreviewsSwitch } from "./previews";
import { machineName, machineUp, roomLabel, roomSigil, RoomBody } from "./room";
import { fuzzy } from "../fuzzy";
import { usePlatformApi, type ChatNav, type PlatformApi, type PlatformTeam } from "../platform-api";

function remembered<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function remember(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    // private window: it lasts for this page only
  }
}

const SIDE_MIN = 200;
const SIDE_MAX = 420;
let sideWidth = remembered("chat.side", 260);
let unreadOnly = remembered("chat.unreadOnly", false);
let collapsed = new Set<string>(remembered<string[]>("chat.rooms.collapsed", []));
let detailsOpen = remembered<unknown>("chat.details", 0) === 1;

/** The teams this account is in, as the page knows them. */
function teams(api: PlatformApi): PlatformTeam[] {
  return api.teams().filter((t) => t.role !== null);
}

function teamName(api: PlatformApi, id: string | null): string {
  return (id && teams(api).find((t) => t.team === id)?.roster.name) || "a team";
}

/** The sidebar's groups, in order: per team its channels then its DMs,
 * then the threads by machine, then archives: the rooms `listedInChat`
 * says, which are what the Chat button counts. */
interface Group {
  key: string;
  title: string;
  /** What `+` makes here. */
  make?: { label: string; aria: string; run: () => void };
  rooms: ChatRoom[];
  none?: string;
}

const byNewest = (a: ChatRoom, b: ChatRoom) => b.last - a.last || a.name.localeCompare(b.name);

function groupsOf(store: ChatStore, api: PlatformApi): Group[] {
  const out: Group[] = [];
  const ts = teams(api);
  const known = new Set(ts.map((t) => t.team));
  // Rooms of a team this page hasn't loaded (yet): still listed.
  for (const r of store.rooms) if (r.team && !known.has(r.team)) known.add(r.team);
  const many = known.size > 1;
  for (const team of known) {
    const t = ts.find((x) => x.team === team);
    const name = t?.roster.name ?? teamName(api, team);
    const mayMake = t?.role === "owner" || t?.role === "editor";
    const channels = store.rooms
      .filter((r) => r.kind === "channel" && r.team === team)
      .sort((a, b) => Number(a.archived) - Number(b.archived) || a.name.localeCompare(b.name));
    out.push({
      key: `${team}/channels`,
      title: many ? `${name} · Channels` : "Channels",
      make: mayMake ? { label: "+", aria: many ? `New channel in ${name}` : "New channel", run: () => void newChannel(store, team, name, api) } : undefined,
      rooms: channels,
      none: mayMake ? "No channels yet. Make one with +." : "No channels yet.",
    });
    out.push({
      key: `${team}/dms`,
      title: many ? `${name} · Direct messages` : "Direct messages",
      make: t ? { label: "+", aria: many ? `New direct message in ${name}` : "New direct message", run: () => void newDm(store, t, api) } : undefined,
      rooms: store.rooms.filter((r) => r.kind === "dm" && r.team === team).sort(byNewest),
    });
  }
  const threads = store.rooms.filter((r) => r.kind === "pane" || r.kind === "session");
  const machines = new Map<string, ChatRoom[]>();
  for (const r of threads) {
    const m = r.machine ?? "";
    (machines.get(m) ?? machines.set(m, []).get(m)!).push(r);
  }
  for (const [m, rooms] of machines) {
    const name = machineName(api, m, rooms[0].machine_name ?? "a machine");
    out.push({ key: `machine/${m}`, title: `${name}${machineUp(api, m) ? "" : " (asleep)"}`, rooms: rooms.sort(byNewest) });
  }
  const archives = store.rooms.filter((r) => r.kind === "archive").sort(byNewest);
  if (archives.length) out.push({ key: "archives", title: "Archives", rooms: archives });
  return out;
}

async function newChannel(store: ChatStore, team: string, name: string, api: PlatformApi) {
  const v = await askText(`New channel in ${name}`, "", "one word, like general");
  if (!v?.trim()) return;
  try {
    const r = await store.newChannel(team, v.trim());
    api.nav.openRoom(r.id);
  } catch (e) {
    api.toast(`Couldn't make it: ${(e as Error).message}`);
  }
}

/** Someone in the team to write to: picked from its people, or typed. */
async function newDm(store: ChatStore, t: PlatformTeam, api: PlatformApi) {
  const toast = api.toast;
  const people = t.roster.members
    .filter((m) => m.account !== store.me)
    .map((m) => ({ value: m.account, label: t.names?.[m.account] || m.name }));
  if (!people.length) return toast(`Nobody else is in ${t.roster.name} yet`);
  const v = await askText(`Message someone in ${t.roster.name}`, "", "their name", Promise.resolve(people));
  if (!v?.trim()) return;
  const typed = v.trim().toLowerCase();
  const who = people.find((p) => p.value === v || p.label.toLowerCase() === typed || p.label.toLowerCase().startsWith(typed));
  if (!who) return toast(`Nobody in ${t.roster.name} is called ${v.trim()}`);
  try {
    const r = await store.newDm(t.team, who.value);
    api.nav.openRoom(r.id);
  } catch (e) {
    toast(`Couldn't open it: ${(e as Error).message}`);
  }
}

/** The room shown: the route's, else (on a computer) the newest unread,
 * else the newest. */
function pickRoom(store: ChatStore, route: ChatRoute, phone: boolean): ChatRoom | null {
  if (route.room) return store.room(route.room) ?? null;
  if (phone || route.view) return null;
  // Object threads (M196) aren't listed here: one opens from its object,
  // and its unread counts there (./unread).
  const live = store.rooms.filter((r) => r.kind !== "archive" && listedInChat(r));
  return live.filter((r) => r.unread).sort(byNewest)[0] ?? live.filter((r) => r.last).sort(byNewest)[0] ?? live[0] ?? null;
}

export function RoomsView({ route, store }: { route: ChatRoute; store: ChatStore }) {
  const phone = usePhone();
  const api = usePlatformApi();
  const { nav, Bar } = api;
  useSubscribe((fn) => store.subscribe(fn));
  useSubscribe((fn) => api.subscribe(fn));
  const [, setTick] = useState(0);
  const redraw = () => setTick((t) => t + 1);
  const picked = pickRoom(store, route, phone);
  const groups = groupsOf(store, api);
  // A room the route names that isn't (yet) in the list: loaded once, in
  // case it came since (a push for a room just made).
  const missing = route.room && !picked ? route.room : null;
  useEffect(() => {
    if (missing) void store.loadRooms().catch(() => {});
  }, [missing]);

  useEffect(() => {
    document.documentElement.style.setProperty("--chat-side", `${sideWidth}px`);
  });
  useEffect(() => () => document.documentElement.style.removeProperty("--chat-side"), []);

  const order = groups.flatMap((g) => (collapsed.has(g.key) && !phone ? [] : g.rooms));
  useRoomKeys(order, picked?.id ?? null, () => void store.readAll(), nav);
  const mentions = store.rooms.reduce((n, r) => n + (r.mentions ? 1 : 0), 0);

  const toggle = (key: string) => {
    if (collapsed.has(key)) collapsed.delete(key);
    else collapsed.add(key);
    remember("chat.rooms.collapsed", [...collapsed]);
    redraw();
  };

  const row = (r: ChatRoom) => {
    const n = r.muted ? r.mentions : r.unread;
    return (
      <button
        key={r.id}
        class={`chat-row${r.id === picked?.id ? " selected" : ""}${n ? " unread" : ""}${r.last ? "" : " quiet"}${r.archived || r.muted ? " muted" : ""}`}
        data-chat-room={r.id}
        data-room-kind={r.kind}
        title={r.kind === "channel" ? `#${r.name} in ${teamName(api, r.team)}` : r.kind === "dm" ? `You and ${r.with?.name ?? "them"}` : `${r.name} on ${machineName(api, r.machine, r.machine_name ?? "")}`}
        onClick={() => nav.openRoom(r.id)}
      >
        {r.kind === "dm" && r.with ? (
          <span class="chat-sigil chat-dm-pic">
            <MsgAvatar who={r.with.account} name={r.with.name} />
          </span>
        ) : (
          <span class="chat-sigil">{roomSigil(r)}</span>
        )}
        <span class="chat-label">{roomLabel(r, api)}</span>
        {n > 0 && <span class={r.mentions ? "chat-count mention" : "chat-count"}>{r.mentions ? `@${n}` : n}</span>}
      </button>
    );
  };

  const list = (
    <nav class="chat-list" aria-label="Rooms">
      <div class="chat-top">
        <button class={`chat-row chat-activity${route.view === "activity" ? " selected" : ""}${mentions ? " unread" : ""}`} data-chat-activity onClick={() => nav.openChatView("activity")}>
          <span class="chat-sigil">@</span>
          <span class="chat-label">Activity</span>
          {mentions > 0 && <span class="chat-count mention">{mentions}</span>}
        </button>
      </div>
      {groups.length === 0 && (
        <p class="chat-none" data-chat-no-team>
          Chat is a team's: its channels and messages. Make a team or join one (your account's menu, Teams), and a pane's thread shows here too.
        </p>
      )}
      {groups.map((g) => {
        const shut = !phone && collapsed.has(g.key);
        const shown = unreadOnly ? g.rooms.filter((r) => r.unread || r.id === picked?.id) : g.rooms;
        const unread = g.rooms.reduce((n, r) => n + r.unread, 0);
        return (
          <section key={g.key} data-chat-group={g.key}>
            <h2 class="chat-section chat-channels-head">
              <button aria-expanded={!shut} onClick={() => toggle(g.key)} title={shut ? "Show them" : "Hide them"}>
                <span class="chat-caret" aria-hidden="true">
                  {shut ? "▸" : "▾"}
                </span>
                {g.title}
                {shut && unread > 0 && <span class="chat-count">{unread}</span>}
              </button>
              {g.make && (
                <button class="chat-new" title={g.make.aria} aria-label={g.make.aria} data-room-new={g.key} onClick={g.make.run}>
                  {g.make.label}
                </button>
              )}
            </h2>
            {!shut && g.rooms.length === 0 && g.none && <p class="chat-none">{g.none}</p>}
            {!shut && g.rooms.length > 0 && shown.length === 0 && <p class="chat-none">Nothing unread.</p>}
            {!shut && shown.map(row)}
          </section>
        );
      })}
    </nav>
  );

  const view = route.view ? (
    route.view === "activity" ? (
      <RoomActivityView store={store} phone={phone} />
    ) : (
      <RoomSearchView store={store} q={route.q ?? ""} phone={phone} />
    )
  ) : picked ? (
    <RoomView key={picked.id} store={store} room={picked} phone={phone} focus={route.msg} />
  ) : missing ? (
    <div class="chat-thread chat-empty" data-room-missing>
      That room isn't one you can read (or it's gone).
    </div>
  ) : null;

  if (phone) {
    return (
      <div class="chat phone" data-chat data-chat-control>
        {view ?? (
          <>
            <header class="chat-head">
              <strong>Chat</strong>
              <button class="chat-search-open" title="Search chat" aria-label="Search chat" data-chat-search-open onClick={() => nav.openChatView("search")}>
                <svg viewBox="0 0 16 16" width="17" height="17" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">
                  <circle cx="7" cy="7" r="4.5" />
                  <path d="M10.5 10.5 14 14" />
                </svg>
              </button>
              <button class="thread-close" title="Close" aria-label="Close chat" onClick={nav.closeChat}>
                ✕
              </button>
            </header>
            {list}
          </>
        )}
      </div>
    );
  }

  const workspaceMenu = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const items: MenuItem[] = [
      { label: "Mark all as read", run: () => void store.readAll() },
      "separator",
      { label: "Show every room", checked: !unreadOnly, run: () => ((unreadOnly = false), remember("chat.unreadOnly", false), redraw()) },
      { label: "Show unread only", checked: unreadOnly, run: () => ((unreadOnly = true), remember("chat.unreadOnly", true), redraw()) },
    ];
    openMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, items);
  };

  const resize = (e: PointerEvent) => {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = sideWidth;
    const move = (m: PointerEvent) => {
      sideWidth = Math.max(SIDE_MIN, Math.min(SIDE_MAX, w0 + m.clientX - x0));
      redraw();
    };
    const up = () => {
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", up);
      remember("chat.side", sideWidth);
    };
    addEventListener("pointermove", move);
    addEventListener("pointerup", up);
  };

  const ts = teams(api);
  return (
    <div class="chat" data-chat data-chat-control>
      <Bar>
        <SearchField route={route} />
      </Bar>
      <MaintenanceNote />
      <RoomSwitcher rooms={order} />
      <div class="chat-body">
        <aside class="chat-side">
          <header class="chat-side-head">
            <button class="chat-workspace" data-chat-workspace onClick={workspaceMenu} onContextMenu={workspaceMenu}>
              {ts.length === 1 ? ts[0].roster.name : "Chat"} <span class="caret">▾</span>
            </button>
          </header>
          {list}
          <div class="chat-resize" role="separator" aria-orientation="vertical" title="Drag to resize" onPointerDown={resize} />
        </aside>
        {view ?? <div class="chat-thread chat-empty">No rooms yet. Make a channel, or write to someone in your team.</div>}
      </div>
    </div>
  );
}

/** What the header's second line says. */
function topicOf(r: ChatRoom, api: PlatformApi): string {
  switch (r.kind) {
    case "channel":
      return [teamName(api, r.team), r.archived ? "archived" : ""].filter(Boolean).join(" · ");
    case "dm":
      return `Direct message · ${teamName(api, r.team)}`;
    case "pane":
    case "session": {
      const name = machineName(api, r.machine, r.machine_name ?? "a machine");
      return `${r.kind === "pane" ? "A pane's thread" : "A session's thread"} · ${name}${machineUp(api, r.machine) ? "" : " is asleep"}`;
    }
    case "object":
      return "A thread on an object · its project's members read it";
    default:
      return "Archive";
  }
}

function RoomView({ store, room, phone, focus }: { store: ChatStore; room: ChatRoom; phone: boolean; focus?: number }) {
  const [details, setDetails] = useState(detailsOpen && !phone);
  const api = usePlatformApi();
  const { nav, toast } = api;
  const people = store.people(room.id);
  const label = roomLabel(room, api);
  const name = room.kind === "channel" ? `#${room.name}` : label;
  const toggleDetails = () => {
    detailsOpen = !details;
    remember("chat.details", detailsOpen ? 1 : 0);
    setDetails(detailsOpen);
  };
  const reachable = (room.kind === "pane" || room.kind === "session") && !!room.machine && !!api.machine(room.machine);
  const go = () => {
    if (!room.machine || room.target === null) return;
    if (room.kind === "pane") return api.goToPane(room.machine, room.target, null);
    api.goToSession(room.machine);
  };
  return (
    <div class="chat-main">
      <section class="chat-thread" aria-label={name} data-room-view={room.id}>
        <header class="chat-head">
          {phone && (
            <button class="chat-back" title="Every room" aria-label="Every room" onClick={() => nav.openChat()}>
              ‹
            </button>
          )}
          <div class="chat-title">
            <strong>
              <span class="chat-title-sigil">{roomSigil(room)}</span>
              {label}
            </strong>
            <span data-chat-topic>{topicOf(room, api)}</span>
          </div>
          {!phone && people.length > 0 && (
            <button class="chat-members" title={people.map((p) => p.name).join(", ")} onClick={() => !details && toggleDetails()}>
              {people.slice(0, 3).map((p) => (
                <MsgAvatar key={p.account} who={p.account} name={p.name} />
              ))}
              <span>{people.length}</span>
            </button>
          )}
          {reachable && (
            <button class="chat-go" data-chat-go onClick={go}>
              {room.kind === "pane" ? "Go to pane" : "Go to session"}
            </button>
          )}
          <button
            class={room.muted ? "chat-info on" : "chat-info"}
            title={room.muted ? "Muted: no notifications from it but mentions' counts. Unmute" : "Mute: no notifications from it"}
            aria-label={room.muted ? "Unmute this room" : "Mute this room"}
            aria-pressed={room.muted}
            data-room-mute
            onClick={() => store.mute(room.id, !room.muted).catch((e: Error) => toast(e.message))}
          >
            {room.muted ? "🔕" : "🔔"}
          </button>
          {!phone && (
            <button class={details ? "chat-info on" : "chat-info"} title={details ? "Hide details" : "Details: who's in it"} aria-pressed={details} data-chat-details onClick={toggleDetails}>
              ⓘ
            </button>
          )}
          {phone && (
            <button class="thread-close" title="Close" aria-label="Close chat" onClick={nav.closeChat}>
              ✕
            </button>
          )}
        </header>
        <RoomBody room={room} phone={phone} focus={focus} name={name} />
      </section>
      {details && <RoomDetails store={store} room={room} close={toggleDetails} />}
    </div>
  );
}

/** The right-hand panel: who reads the room; a team owner's channel tools. */
function RoomDetails({ store, room, close }: { store: ChatStore; room: ChatRoom; close: () => void }) {
  const api = usePlatformApi();
  const toast = api.toast;
  const people = store.people(room.id);
  const owner = room.kind === "channel" && teams(api).find((t) => t.team === room.team)?.role === "owner";
  return (
    <aside class="chat-details" aria-label="Details" data-chat-details-panel>
      <header class="chat-head">
        <strong>Details</strong>
        <button class="thread-close" title="Close" onClick={close}>
          ✕
        </button>
      </header>
      <div class="chat-details-body">
        {(room.kind === "pane" || room.kind === "session") && (
          <p class="chat-none">
            {machineUp(api, room.machine)
              ? `On ${machineName(api, room.machine, room.machine_name ?? "")}. Who reads this thread is whom the machine says may read its ${room.kind}.`
              : `${machineName(api, room.machine, room.machine_name ?? "Its machine")} is asleep. Its thread is still here.`}
          </p>
        )}
        <h3>People</h3>
        {people.length === 0 && <p class="chat-none">Just you, so far.</p>}
        <ul class="chat-people">
          {people.map((p) => (
            <li key={p.account} data-room-person={p.account}>
              <MsgAvatar who={p.account} name={p.name} />
              <span>{p.name}</span>
            </li>
          ))}
        </ul>
        {room.team && teams(api).find((t) => t.team === room.team)?.role === "owner" && <WebPreviewsSwitch team={room.team} toast={toast} />}
        {owner && (
          <div class="thread-offer-buttons">
            <button
              onClick={async () => {
                const v = await askText(`Rename #${room.name}`, room.name, "one word");
                if (v?.trim() && v.trim() !== room.name) store.rename(room.id, v.trim()).catch((e: Error) => toast(e.message));
              }}
            >
              Rename
            </button>
            <button onClick={() => store.archive(room.id, !room.archived).catch((e: Error) => toast(e.message))}>{room.archived ? "Unarchive" : "Archive"}</button>
          </div>
        )}
      </div>
    </aside>
  );
}

// ---- the switcher (Ctrl/Cmd+K) and keys

function RoomSwitcher({ rooms }: { rooms: ChatRoom[] }) {
  const api = usePlatformApi();
  const open = useSwitcherOpen();
  const [q, setQ] = useState("");
  const [pick, setPick] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    setQ("");
    setPick(0);
    if (open) input.current?.focus();
  }, [open]);
  if (!open) return null;
  const list = rooms
    .map((r) => ({ r, score: fuzzy(q, `${roomLabel(r, api)} ${r.machine_name ?? ""}`) }))
    .filter((x): x is { r: ChatRoom; score: number } => x.score !== null)
    .sort((a, b) => b.score - a.score || b.r.unread - a.r.unread || b.r.last - a.r.last)
    .map((x) => x.r)
    .slice(0, 12);
  const go = (r: ChatRoom | undefined) => {
    closeSwitcher();
    if (r) api.nav.openRoom(r.id);
  };
  return (
    <div class="chat-switcher-back" onMouseDown={(e) => e.target === e.currentTarget && closeSwitcher()}>
      <div class="chat-switcher" role="dialog" aria-label="Jump to a room" data-chat-switcher>
        <input
          ref={input}
          placeholder="Jump to…"
          value={q}
          onInput={(e) => {
            setQ((e.target as HTMLInputElement).value);
            setPick(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              closeSwitcher();
            } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              if (list.length) setPick((p) => (p + (e.key === "ArrowDown" ? 1 : list.length - 1)) % list.length);
            } else if (e.key === "Enter") {
              e.preventDefault();
              go(list[pick]);
            }
          }}
        />
        <ul role="listbox">
          {list.map((r, i) => (
            <li
              key={r.id}
              role="option"
              aria-selected={i === pick}
              class={`${i === pick ? "picked" : ""}${r.unread ? " unread" : ""}`}
              onMouseDown={(e) => {
                e.preventDefault();
                go(r);
              }}
            >
              <span class="chat-sigil">{roomSigil(r)}</span>
              <span class="chat-label">{roomLabel(r, api)}</span>
              {r.machine_name && <span class="chat-switcher-host">{machineName(api, r.machine, r.machine_name)}</span>}
              {r.unread ? <span class={r.mentions ? "chat-count mention" : "chat-count"}>{r.unread}</span> : null}
            </li>
          ))}
          {list.length === 0 && <li class="chat-none">No room matches.</li>}
        </ul>
      </div>
    </div>
  );
}

/** Ctrl/Cmd+K: the switcher. Alt+↑/↓: the room above or below; with
 * Shift, the next unread one. Shift+Esc: everything read. */
function useRoomKeys(rooms: ChatRoom[], picked: string | null, markAll: () => void, nav: ChatNav) {
  const latest = useRef({ rooms, picked, markAll, nav });
  latest.current = { rooms, picked, markAll, nav };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (document.querySelector(".menu, .prompt")) return;
      const { rooms, picked, markAll, nav } = latest.current;
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        e.stopPropagation();
        openSwitcher();
        return;
      }
      if (e.key === "Escape" && e.shiftKey) {
        e.preventDefault();
        markAll();
        return;
      }
      if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown") || !rooms.length) return;
      e.preventDefault();
      const n = rooms.length;
      const at = rooms.findIndex((r) => r.id === picked);
      const down = e.key === "ArrowDown";
      for (let i = 1; i <= n; i++) {
        const idx = at < 0 ? (down ? i - 1 : n - i) : (((at + (down ? i : -i)) % n) + n) % n;
        const r = rooms[idx];
        if (idx === at) return;
        if (!e.shiftKey || r.unread) return nav.openRoom(r.id);
      }
    };
    addEventListener("keydown", key, true);
    return () => removeEventListener("keydown", key, true);
  }, []);
}

/** The chat page's store, re-rendering on its changes. */
export function useChat(): ChatStore | null {
  useSubscribe((fn) => chat.subscribe(fn));
  return chat.on === true ? chat : null;
}
