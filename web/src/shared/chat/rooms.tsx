// The chat page on control's chat (M123): a team's channels and DMs, and
// the threads on panes and sessions, all rooms that control holds. The
// sidebar lists them (Starred and the person's own sections first, then each
// team's channels and DMs, the threads by machine, and archives); the room
// is ./room's. Nothing here needs a
// machine to be up: a pane's thread reads and posts with its machine
// asleep, and only its chip waits for it.
//
// Routes: `#chat` (the newest unread room, or the newest), `#chat=<room>`
// and `#chat=<room>&msg=<n>`, `#chat=activity`, `#chat=search/<words>`.

import { useEffect, useRef, useState } from "preact/hooks";
import { chat, type ChatStore } from "../chat-store";
import { bold, counted, listedInChat } from "./unread";
import { bellFor, openBell } from "./bell";
import { loadNotify, subscribeNotify } from "./notify";
import type { ChatRoom } from "../proto";
import { MaintenanceNote } from "../ui/maintenance-note";
import { usePhone, useSubscribe } from "../ui/hooks";
import { askText, openMenu, type MenuItem } from "../ui/menu";
import type { ChatRoute } from "./route";
import { SavedView } from "./saved";
import { RoomActivityView, RoomSearchView, SearchField } from "./find";
import { MsgAvatar } from "./avatar";
import { WebPreviewsSwitch } from "./previews";
import { AboutButton, editDescription, editTopic, PinsButton, TopicLine } from "./pins-ui";
import { ThreadPanel } from "./thread-panel";
import { ThreadsView } from "./threads-view";
import { badge } from "./threads";
import { browseChannels, ChatDialogs, LeaveButton, leaveItem, MemberTools, newChannelDialog, newDmDialog } from "./members";
import { machineName, machineUp, roomLabel, roomSigil, RoomBody } from "./room";
import { AgentsChip } from "./agents-chip";
import { newSection, openGroupMenu, openRoomMenu, RoomStar, toggleGroup } from "./sections";
import { inScope, ordered, place, placed, present, sortOf, STARRED, type ChatScope } from "./sidebar";
import { BookmarkIcon, SearchIcon } from "./icons";
import type { ChatSort } from "../proto";
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
  /** The channel browser (#118). */
  browse?: () => void;
  rooms: ChatRoom[];
  none?: string;
  /** Where dropping a room here puts it: `starred`, a section's id, or
   * null for the built-in groups (where its kind puts it). */
  drop: string | null;
  /** A section of the person's own (#120): renamed and deleted. */
  custom?: boolean;
  /** How it sorts until the person chooses. */
  sort: ChatSort;
}

const byNewest = (a: ChatRoom, b: ChatRoom) => b.last - a.last || a.name.localeCompare(b.name);

/** The sidebar's groups (#120): Starred (while it has rooms, or a room is
 * being dragged), the person's own sections, then the built-in ones, minus
 * the rooms they placed. With a `scope`, only its rooms (UI audit #169): a
 * section of other projects' rooms only isn't shown. */
function groupsOf(store: ChatStore, api: PlatformApi, dragging: boolean, scope?: ChatScope): Group[] {
  const sb = store.sidebar;
  const out: Group[] = [];
  const label = (r: ChatRoom) => roomLabel(r, api);
  const all = store.rooms.filter((r) => inScope(r, scope));
  const starred = present(sb.starred, all);
  if (starred.length || dragging) {
    out.push({ key: STARRED, title: "★ Starred", rooms: ordered(starred, sortOf(sb, STARRED), label), none: "Drag a room here, or star it.", drop: STARRED, sort: "name" });
  }
  for (const x of sb.sections) {
    const rooms = present(x.rooms, all);
    if (scope && !rooms.length && present(x.rooms, store.rooms).length) continue;
    out.push({ key: x.id, title: x.name, rooms: ordered(rooms, sortOf(sb, x.id), label), none: "Drag rooms here, or move one from its ⋯ menu.", drop: x.id, custom: true, sort: "name" });
  }
  const taken = placed(sb);
  const free = (r: ChatRoom) => !taken.has(r.id);
  // A built-in group sorted as the person chose, else as it always was.
  const sorted = (key: string, rooms: ChatRoom[], fallback: ChatSort) => ordered(rooms.filter(free), sortOf(sb, key, fallback), label);
  const ts = teams(api);
  const known = new Set(ts.map((t) => t.team));
  // Rooms of a team this page hasn't loaded (yet): still listed.
  for (const r of store.rooms) if (r.team && !known.has(r.team)) known.add(r.team);
  // A project's page: its team's, or none for the reader's own.
  if (scope) for (const team of known) if (team !== scope.team) known.delete(team);
  const many = known.size > 1;
  for (const team of known) {
    const t = ts.find((x) => x.team === team);
    const name = t?.roster.name ?? teamName(api, team);
    const mayMake = t?.role === "owner" || t?.role === "editor";
    const channels = all.filter((r) => r.kind === "channel" && r.team === team);
    const dms = all.filter((r) => r.kind === "dm" && r.team === team);
    out.push({
      key: `${team}/channels`,
      title: many ? `${name} · Channels` : "Channels",
      make: mayMake ? { label: "+", aria: many ? `New channel in ${name}` : "New channel", run: () => newChannelDialog(store, team, name, api) } : undefined,
      browse: t ? () => browseChannels(store, team, name, api) : undefined,
      rooms: sorted(`${team}/channels`, channels, "name"),
      none: mayMake ? "No channels joined. Browse them, or make one with +." : "No channels joined. Browse them.",
      drop: null,
      sort: "name",
    });
    out.push({
      key: `${team}/dms`,
      title: many ? `${name} · Direct messages` : "Direct messages",
      make: t ? { label: "+", aria: many ? `New direct message in ${name}` : "New direct message", run: () => newDmDialog(store, t, api) } : undefined,
      rooms: sorted(`${team}/dms`, dms, "recent"),
      drop: null,
      sort: "recent",
    });
  }
  const threads = all.filter((r) => r.kind === "pane" || r.kind === "session");
  const machines = new Map<string, ChatRoom[]>();
  for (const r of threads) {
    const m = r.machine ?? "";
    (machines.get(m) ?? machines.set(m, []).get(m)!).push(r);
  }
  for (const [m, rooms] of machines) {
    const name = machineName(api, m, rooms[0].machine_name ?? "a machine");
    const shown = sorted(`machine/${m}`, rooms, "recent");
    if (shown.length) out.push({ key: `machine/${m}`, title: `${name}${machineUp(api, m) ? "" : " (asleep)"}`, rooms: shown, drop: null, sort: "recent" });
  }
  const archives = sorted("archives", all.filter((r) => r.kind === "archive"), "recent");
  if (archives.length) out.push({ key: "archives", title: "Archives", rooms: archives, drop: null, sort: "recent" });
  return out;
}

/** The room shown: the route's, else (on a computer) the newest unread,
 * else the newest. */
function pickRoom(store: ChatStore, route: ChatRoute, phone: boolean, scope?: ChatScope): ChatRoom | null {
  if (route.room) return store.room(route.room) ?? null;
  if (phone || route.view) return null;
  // Object threads (M196) aren't listed here: one opens from its object,
  // and its unread counts there (./unread).
  const live = store.rooms.filter((r) => r.kind !== "archive" && listedInChat(r) && inScope(r, scope));
  return live.filter((r) => r.unread).sort(byNewest)[0] ?? live.filter((r) => r.last).sort(byNewest)[0] ?? live[0] ?? null;
}

/** How a host fits chat in its box (#123). The platform page fills the
 * window and has neither. */
export interface ChatFit {
  /** Draw the phone's one-pane-at-a-time layout though the window is wide: a
   * block is as narrow as its slot. */
  narrow?: boolean;
  /** One room, no sidebar: the room is the route's. */
  pinned?: boolean;
  /** The page has other views of chat (blocks): keys answer only while focus
   * is in this one. */
  scoped?: boolean;
}

/** `scope`: the one project's rooms a page lists (UI audit #169); none
 * for every room. */
export function RoomsView({ route, store, fit = {}, scope }: { route: ChatRoute; store: ChatStore; fit?: ChatFit; scope?: ChatScope }) {
  const phone = usePhone() || !!fit.narrow;
  // The keys (and the switcher they open) are this view's while focus is in
  // it: a page with two chat blocks has two views.
  const root = useRef<HTMLDivElement>(null);
  const [switching, setSwitching] = useState(false);
  const api = usePlatformApi();
  const { nav, Bar } = api;
  useSubscribe((fn) => store.subscribe(fn));
  useSubscribe((fn) => api.subscribe(fn));
  const [, setTick] = useState(0);
  const redraw = () => setTick((t) => t + 1);
  // The room being dragged to a section (#120), on a computer.
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const picked = pickRoom(store, route, phone, scope);
  const groups = groupsOf(store, api, dragging !== null, scope);
  // Mark all as read is this page's rooms'.
  const readAll = () => void store.readAll((r) => inScope(r, scope));
  const collapsed = new Set(store.sidebar.collapsed);
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

  const order = groups.flatMap((g) => (collapsed.has(g.key) ? [] : g.rooms));
  useRoomKeys(fit.pinned ? [] : order, picked?.id ?? null, readAll, nav, () => setSwitching(true), fit.scoped && !fit.pinned ? root : null);
  const savedTodo = store.savedTodo();
  const mentions = store.rooms.reduce((n, r) => n + (r.mentions ? 1 : 0), 0);

  const toggle = (key: string) => toggleGroup(store, key, api.toast);
  const drop = (g: Group, e: DragEvent) => {
    e.preventDefault();
    const id = e.dataTransfer?.getData("text/x-arugula-room") || dragging;
    setDragging(null);
    setOver(null);
    if (id) void store.editSidebar((s) => place(s, id, g.drop)).catch((err: Error) => api.toast(`Couldn't keep that: ${err.message}`));
  };

  const row = (r: ChatRoom) => {
    const n = counted(r);
    // A link to the room (UI audit #185), so a middle click and "Copy
    // link" work; a plain click stays on the page.
    return (
      <a
        key={r.id}
        href={nav.roomHref(r.id)}
        class={`chat-row${r.id === picked?.id ? " selected" : ""}${bold(r) ? " unread" : ""}${r.last ? "" : " quiet"}${r.archived || r.level === "nothing" ? " muted" : ""}${dragging === r.id ? " dragging" : ""}`}
        data-chat-room={r.id}
        data-room-kind={r.kind}
        title={r.kind === "channel" ? `#${r.name} in ${teamName(api, r.team)}${r.topic ? `\n${r.topic}` : ""}` : r.kind === "dm" ? r.others.length > 1 ? `You and ${r.others.map((p) => p.name).join(", ")}` : `You and ${r.with?.name ?? "them"}` : `${r.name} on ${machineName(api, r.machine, r.machine_name ?? "")}`}
        draggable={!phone}
        onDragStart={(e) => {
          e.dataTransfer?.setData("text/x-arugula-room", r.id);
          if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
          // After the browser has taken its drag image: drawing the drop
          // targets first would cancel the drag on some of them.
          setTimeout(() => setDragging(r.id));
        }}
        onDragEnd={() => {
          setDragging(null);
          setOver(null);
        }}
        onContextMenu={(e) => openRoomMenu(e, store, r, api.toast, [leaveItem(store, r, api)].filter((x) => x !== null))}
        onClick={(e) => {
          if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
          e.preventDefault();
          nav.openRoom(r.id);
        }}
      >
        {r.kind === "dm" && r.with && r.others.length < 2 ? (
          <span class="chat-sigil chat-dm-pic">
            <MsgAvatar who={r.with.account} name={r.with.name} account={r.with.account} dot />
          </span>
        ) : (
          <span class="chat-sigil">{roomSigil(r)}</span>
        )}
        <span class="chat-label">{roomLabel(r, api)}</span>
        {r.with?.dnd && (
          <span class="chat-dnd" title={`${r.with.name} has paused notifications`} aria-label="Paused notifications" data-dnd>
            🌙
          </span>
        )}
        {n > 0 && <span class={r.mentions ? "chat-count mention" : "chat-count"}>{r.mentions ? `@${n}` : n}</span>}
      </a>
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
        <button class={`chat-row chat-saved${route.view === "saved" ? " selected" : ""}`} data-chat-saved onClick={() => nav.openChatView("saved")}>
          {/* A bookmark: ★ is Starred's (UI audit #169). */}
          <span class="chat-sigil">
            <BookmarkIcon size={12} />
          </span>
          <span class="chat-label">Saved</span>
          {savedTodo > 0 && <span class="chat-count">{savedTodo}</span>}
        </button>
        <button
          class={`chat-row chat-activity${route.view === "threads" ? " selected" : ""}${store.threadsUnread.n ? " unread" : ""}`}
          data-chat-threads
          onClick={() => nav.openChatView("threads")}
        >
          <span class="chat-sigil">↩</span>
          <span class="chat-label">Threads</span>
          {store.threadsUnread.n > 0 && <span class={store.threadsUnread.mentions ? "chat-count mention" : "chat-count"}>{badge(store.threadsUnread.n, store.threadsUnread.mentions)}</span>}
        </button>
      </div>
      {groups.length === 0 && (
        <p class="chat-none" data-chat-no-team>
          {scope && scope.team === null
            ? "Your machines' threads show here: a pane's or a session's. A team's channels and messages are in the team's project."
            : "Chat is a team's: its channels and messages. Make a team or join one (your account's menu, Teams), and a pane's thread shows here too."}
        </p>
      )}
      {groups.map((g) => {
        const shut = collapsed.has(g.key);
        const shown = unreadOnly ? g.rooms.filter((r) => r.unread || r.id === picked?.id) : g.rooms;
        const unread = g.rooms.reduce((n, r) => n + r.unread, 0);
        return (
          <section
            key={g.key}
            data-chat-group={g.key}
            class={dragging && over === g.key ? "chat-drop" : undefined}
            onDragOver={(e) => {
              if (!dragging) return;
              e.preventDefault();
              if (over !== g.key) setOver(g.key);
            }}
            onDrop={(e) => drop(g, e)}
          >
            <h2 class="chat-section chat-channels-head">
              <button aria-expanded={!shut} onClick={() => toggle(g.key)} title={shut ? "Show them" : "Hide them"}>
                <span class="chat-caret" aria-hidden="true">
                  {shut ? "▸" : "▾"}
                </span>
                {g.title}
                {shut && unread > 0 && <span class="chat-count">{unread}</span>}
              </button>
              {g.browse && (
                <button class="chat-new" title="Browse channels" aria-label="Browse channels" data-room-browse={g.key} onClick={g.browse}>
                  <SearchIcon size={13} />
                </button>
              )}
              <button
                class="chat-new chat-group-menu"
                title={`Sort${g.custom ? ", rename" : ""}`}
                aria-label={`Options for ${g.title}`}
                data-group-menu={g.key}
                onClick={(e) => {
                  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                  openGroupMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, store, { key: g.key, custom: !!g.custom, title: g.title, fallback: g.sort }, api.toast);
                }}
              >
                ⋯
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
      <ChatDialogs />
    </nav>
  );

  const view = route.view ? (
    route.view === "activity" ? (
      <RoomActivityView store={store} phone={phone} />
    ) : route.view === "saved" ? (
      <SavedView phone={phone} />
    ) : route.view === "threads" ? (
      <ThreadsView store={store} phone={phone} />
    ) : (
      <RoomSearchView store={store} q={route.q ?? ""} phone={phone} />
    )
  ) : picked ? (
    <RoomView key={picked.id} store={store} room={picked} phone={phone} focus={route.msg} thread={route.thread} />
  ) : missing ? (
    <RoomMissing />
  ) : null;

  const workspaceMenu = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const items: MenuItem[] = [
      { label: "Mark all as read", run: readAll },
      { label: "New section…", run: () => void newSection(store, api.toast) },
      "separator",
      { label: "Show every room", checked: !unreadOnly, run: () => ((unreadOnly = false), remember("chat.unreadOnly", false), redraw()) },
      { label: "Show unread only", checked: unreadOnly, run: () => ((unreadOnly = true), remember("chat.unreadOnly", true), redraw()) },
    ];
    openMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, items);
  };

  if (fit.pinned) {
    return (
      <div class={phone ? "chat phone pinned" : "chat pinned"} ref={root} data-chat data-chat-control data-chat-pinned={route.room}>
        <MaintenanceNote />
        {picked ? (
          <div class="chat-body">
            <RoomView key={picked.id} store={store} room={picked} phone={phone} focus={route.msg} pinned thread={route.thread} />
          </div>
        ) : (
          <RoomMissing pinned />
        )}
      </div>
    );
  }

  if (phone) {
    return (
      <div class="chat phone" ref={root} data-chat data-chat-control>
        <MaintenanceNote />
        {view ?? (
          <>
            <header class="chat-head">
              {/* Whose rooms: a phone has no bar to say (UI audit #169). */}
              <strong>{scope?.name ?? "Chat"}</strong>
              <button class="chat-search-open" title="Search chat" aria-label="Search chat" data-chat-search-open onClick={() => nav.openChatView("search")}>
                <SearchIcon size={17} />
              </button>
              <button class="chat-search-open" title="Mark all as read, sections, unread only" aria-label="Chat options" data-chat-options onClick={workspaceMenu}>
                ⋯
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

  // The sidebar's head: whose rooms these are, and a ⋯ for the view's menu
  // (UI audit #169: a "Team ▾" here read as the bar's project switcher).
  const ts = teams(api);
  const head = scope?.name ?? (ts.length === 1 ? ts[0].roster.name : "Chat");
  return (
    <div class="chat" ref={root} data-chat data-chat-control>
      <Bar>
        <SearchField route={route} />
      </Bar>
      <MaintenanceNote />
      <RoomSwitcher rooms={order} open={switching} close={() => setSwitching(false)} />
      <div class="chat-body">
        <aside class="chat-side">
          <header class="chat-side-head" onContextMenu={workspaceMenu}>
            <strong class="chat-side-name" title={head}>
              {head}
            </strong>
            <button class="chat-new chat-workspace" title="Mark all as read, sections, unread only" aria-label="Chat options" aria-haspopup="menu" data-chat-workspace onClick={workspaceMenu}>
              ⋯
            </button>
          </header>
          {list}
          <div class="chat-resize" role="separator" aria-orientation="vertical" title="Drag to resize" onPointerDown={resize} />
        </aside>
        {view ?? (
          <div class="chat-thread chat-empty">
            {scope && scope.team === null ? "No threads yet. A pane's or a session's thread shows here once someone writes on it." : "No rooms yet. Make a channel, or write to someone in your team."}
          </div>
        )}
      </div>
    </div>
  );
}

/** A room the address names that the reader can't read (UI audit #169),
 * with the way back to the rooms; a block pinned to it has no list to go
 * back to. */
function RoomMissing({ pinned }: { pinned?: boolean }) {
  const { nav } = usePlatformApi();
  return (
    <div class="chat-thread chat-empty" data-room-missing>
      <p>That room isn't one you can read (or it's gone).</p>
      {!pinned && (
        <p>
          <button class="link" data-room-missing-back onClick={() => nav.openChat()}>
            Back to your rooms
          </button>
        </p>
      )}
    </div>
  );
}

/** What the header's second line says. */
function topicOf(r: ChatRoom, api: PlatformApi): string {
  switch (r.kind) {
    case "channel":
      return [r.private ? "private" : "", r.topic, teamName(api, r.team), r.archived ? "archived" : ""].filter(Boolean).join(" · ");
    case "dm":
      return `${r.others.length > 1 ? "Group message" : "Direct message"} · ${teamName(api, r.team)}`;
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

function RoomView({
  store,
  room,
  phone,
  focus,
  pinned,
  thread,
}: {
  store: ChatStore;
  room: ChatRoom;
  phone: boolean;
  focus?: number;
  pinned?: boolean;
  thread?: number;
}) {
  const [details, setDetails] = useState(detailsOpen && !phone);
  const api = usePlatformApi();
  const { nav, toast } = api;
  // The bell's moon follows a snooze, here or in Settings.
  useSubscribe(subscribeNotify);
  useEffect(() => void loadNotify(), []);
  const people = store.people(room.id);
  const label = roomLabel(room, api);
  const name = room.kind === "channel" ? `#${room.name}` : label;
  const toggleDetails = () => {
    detailsOpen = !details;
    remember("chat.details", detailsOpen ? 1 : 0);
    setDetails(detailsOpen);
  };
  const reachable = (room.kind === "pane" || room.kind === "session") && !!room.machine && !!api.machine(room.machine);
  // Leaving is the room menu's, and Details' (UI audit #169).
  const leave = leaveItem(store, room, api);
  const go = () => {
    if (!room.machine || room.target === null) return;
    if (room.kind === "pane") return api.goToPane(room.machine, room.target, null);
    api.goToSession(room.machine);
  };
  // A thread open beside the room (#106): the whole screen on a phone. The
  // room's own `&msg=` is the room's unless it's a reply in that thread.
  const closeThread = () => nav.openRoom(room.id);
  if (thread !== undefined && phone) {
    return (
      <div class="chat-main">
        <ThreadPanel key={thread} store={store} room={room} parent={thread} phone focus={focus} close={closeThread} />
      </div>
    );
  }
  return (
    <div class="chat-main">
      <section class="chat-thread" aria-label={name} data-room-view={room.id}>
        <header class="chat-head">
          {phone && !pinned && (
            <button class="chat-back" title="Every room" aria-label="Every room" onClick={() => nav.openChat()}>
              ‹
            </button>
          )}
          <div class="chat-title">
            <strong>
              <span class="chat-title-sigil">{roomSigil(room)}</span>
              {label}
            </strong>
            <TopicLine room={room} fallback={topicOf(room, api)} toast={toast} />
          </div>
          <PinsButton room={room} toast={toast} />
          {/* On a computer the room's About is in Details (UI audit #169):
              one ⓘ, not two. A phone has no Details. */}
          {phone && <AboutButton room={room} toast={toast} />}
          {!phone && people.length > 0 && (
            <button class="chat-members" title={people.map((p) => p.name).join(", ")} onClick={() => !details && toggleDetails()}>
              {people.slice(0, 3).map((p) => (
                <MsgAvatar key={p.account} who={p.account} name={p.name} account={p.account} dot />
              ))}
              <span>{people.length}</span>
            </button>
          )}
          {reachable && (
            <button class="chat-go" data-chat-go onClick={go}>
              {room.kind === "pane" ? "Go to pane" : "Go to session"}
            </button>
          )}
          <AgentsChip room={room.id} />
          <RoomStar store={store} room={room} toast={toast} more={leave ? [leave] : []} />
          <button
            class={room.level === "all" ? "chat-info" : "chat-info on"}
            title={bellFor(room).title}
            aria-label={bellFor(room).title}
            aria-haspopup="menu"
            data-room-bell
            data-room-level={room.level}
            onClick={(e) => openBell(e, room, (l) => store.setLevel(room.id, l), toast)}
          >
            {bellFor(room).glyph}
          </button>
          {!phone && (
            <button class={details ? "chat-info on" : "chat-info"} title={details ? "Hide details" : "Details: who's in it"} aria-pressed={details} data-chat-details onClick={toggleDetails}>
              ⓘ
            </button>
          )}
          {phone && !pinned && (
            <button class="thread-close" title="Close" aria-label="Close chat" onClick={nav.closeChat}>
              ✕
            </button>
          )}
        </header>
        <RoomBody room={room} phone={phone} focus={thread === undefined ? focus : undefined} name={name} chip />
      </section>
      {thread !== undefined ? (
        <ThreadPanel key={thread} store={store} room={room} parent={thread} phone={false} focus={focus} close={closeThread} />
      ) : (
        details && <RoomDetails store={store} room={room} close={toggleDetails} />
      )}
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
        {room.kind === "channel" && (room.topic || room.description || room.topic_writable) && (
          <div class="chat-about" data-chat-about-panel>
            <h3>About</h3>
            <p class={room.topic ? "" : "dim"}>{room.topic || "No topic yet."}</p>
            {room.description && <p class="chat-pop-text">{room.description}</p>}
            {room.topic_writable && (
              <div class="thread-offer-buttons">
                <button onClick={() => void editTopic(store, room, toast)}>{room.topic ? "Change topic" : "Set topic"}</button>
                <button onClick={() => void editDescription(store, room, toast)}>{room.description ? "Change description" : "Add description"}</button>
              </div>
            )}
          </div>
        )}
        <h3>People</h3>
        {people.length === 0 && <p class="chat-none">Just you, so far.</p>}
        <MemberTools store={store} room={room} people={people} />
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
        <LeaveButton store={store} room={room} />
      </div>
    </aside>
  );
}

// ---- the switcher (Ctrl/Cmd+K) and keys

function RoomSwitcher({ rooms, open, close }: { rooms: ChatRoom[]; open: boolean; close: () => void }) {
  const api = usePlatformApi();
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
    close();
    if (r) api.nav.openRoom(r.id);
  };
  return (
    <div class="chat-switcher-back" onMouseDown={(e) => e.target === e.currentTarget && close()}>
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
              close();
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
              {r.kind === "channel" && r.topic && <span class="chat-switcher-host">{r.topic}</span>}
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
function useRoomKeys(rooms: ChatRoom[], picked: string | null, markAll: () => void, nav: ChatNav, openSwitcher: () => void, within: { current: HTMLElement | null } | null) {
  const latest = useRef({ rooms, picked, markAll, nav });
  latest.current = { rooms, picked, markAll, nav };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (document.querySelector(".menu, .prompt")) return;
      // A block answers only while focus is in it.
      if (within && !within.current?.contains(document.activeElement)) return;
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
        if (!e.shiftKey || counted(r)) return nav.openRoom(r.id);
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
