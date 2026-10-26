// Chat held by control (M123, on M122's API): the rooms this person sees
// (a team's channels and DMs, the threads on panes and sessions), the
// messages the page has fetched, and one live stream (SSE) that says what
// changed. Only on control's page, and only while control's `chat` flag is
// on for the account; the page asks `GET /api/chat`, which says 403
// `chat_off` otherwise. While it's off there is no chat: no machine keeps
// threads of its own (M125), and the Chat page says chat is control's.
//
// No machine has to be up for any of it: a pane's quote is copied into
// the message, and its chip says whether it opens for this reader.

import type {
  ChatChannel,
  ChatChannels,
  ChatFound,
  ChatLive,
  ChatMessage,
  ChatMessages,
  ChatPaneRef,
  ChatPerson,
  ChatPin,
  ChatPins,
  ChatPost,
  ChatPosted,
  ChatRoom,
  ChatRoomAgent,
  ChatRoomDetail,
  ChatRooms,
  ChatSidebar,
  ChatSaved,
  ChatSavedItem,
  ChatTargetKind,
  NotifyLevel,
} from "./proto";
import { platformFetch, refusalOf } from "./maintenance";
import { reacted } from "./chat/reactions";
import { chatUnread, listedInChat } from "./chat/unread";
import { presence } from "./chat/presence";
import { emptySidebar } from "./chat/sidebar";
import { ThreadState } from "./chat/thread-state";

/** A refusal from control's chat API: its status, and `reason`
 * (`chat_off`, or `maintenance` for a window that outlasted the wait,
 * M141) when it gave one. */
export class ChatError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly reason?: string,
  ) {
    super(message);
  }
}

export async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await platformFetch(path, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!res.ok) throw new ChatError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`), j.reason);
  return j;
}

const enc = encodeURIComponent;

/** How long a typing ping shows, and the gap between a page's pings:
 * control's `TYPING_MS` and `TYPING_PING_MS` (#111). */
const TYPING_MS = 6_000;
const TYPING_PING_MS = 3_000;

/** A room's messages as far as the page has them, oldest first, and
 * whether older ones are left to fetch. */
export interface Loaded {
  list: ChatMessage[];
  older: boolean;
}

/** A pane quoted from a page: the machine (control's id for it), the pane
 * and the lines chosen. */
export interface PaneQuote {
  machine: string;
  pane: number;
  text: string;
}

/** Room ids as control makes them (`ch_`, `dm_`, `th_` and 16 hex), and
 * any later prefix of the same shape (an archive's). */
export const ROOM_ID = /^[a-z]{2}_[0-9a-f]{8,32}$/;

export class ChatStore {
  /** Whether chat is on for this account: null until control has said. */
  on: boolean | null = null;
  /** Off because control no longer knows this page's session (a 401), not
   * because chat is off for the account (UI audit #143). */
  signedOut = false;
  /** The last ask didn't reach control (it's asked again soon): while `on`
   * is still null, the page says so rather than "loading" forever. */
  unreachable = false;
  /** The account reading: control's id for it. */
  me = "";
  /** What `@` of me looks like: my login and name, lowercased. */
  myNames: string[] = [];
  rooms: ChatRoom[] = [];
  /** How this account arranges the sidebar (#120), the same on every device. */
  sidebar: ChatSidebar = emptySidebar();
  /** This page's live stream, once control said hello. */
  stream: string | null = null;
  private loaded = new Map<string, Loaded>();
  private detail = new Map<string, ChatRoomDetail>();
  /** Rooms' pins (#119), once asked for: kept current by the stream. */
  private pinned = new Map<string, ChatPin[]>();
  private fetching = new Map<string, Promise<void>>();
  private es: EventSource | null = null;
  /** The rooms this page shows, by who shows each: a page with two chat
   * blocks (#123) has two, and closing one mustn't say the other's shut. */
  private open = new Map<unknown, string>();
  private listeners = new Set<() => void>();
  private probe: number | undefined;
  private roomsTimer: number | undefined;
  private started = false;
  /** Who is typing where (#111), by `room account`, until when. Never
   * fetched: only what the stream said in the last few seconds. */
  private typers = new Map<string, { room: string; account: string; name: string; until: number }>();
  private typed = new Map<string, number>();
  /** Saved for later (#116): null until control has said. */
  saved: ChatSavedItem[] | null = null;
  /** Rooms I marked unread from a message (or another page of mine did),
   * and the message: while the room stays open its "New" line is there and
   * nothing reads it again. Leaving the room lets go. */
  private held = new Map<string, number>();
  /** Reply threads (#106): the ones open, and the Threads view's list. */
  readonly threads = new ThreadState(
    (m, p, b) => call(m, p, b),
    () => this.emit(),
    (room, parent) => this.replace(room, parent),
    (m) => this.mine(m),
  );
  /** Unread replies in the threads I follow, and how many mention me: the
   * Threads view's badge. Not counted in `unread()`: Activity's is apart. */
  threadsUnread = { n: 0, mentions: 0 };

  /** Whether control has signed this page in (so chat is only off). */
  get signedIn(): boolean {
    return this.started;
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => void this.listeners.delete(fn);
  }

  /** Who wants to hear that what an object's thread is on changed (M203:
   * a native issue), or that an agent session changed (M257), by its id. */
  private objects = new Map<string, Set<() => void>>();

  /** Call `fn` when what `target`'s thread is on changes, or the stream
   * comes back after missing what might have. */
  onObject(target: string, fn: () => void): () => void {
    const fns = this.objects.get(target) ?? new Set<() => void>();
    this.objects.set(target, fns.add(fn));
    return () => {
      fns.delete(fn);
      if (!fns.size) this.objects.delete(target);
    };
  }

  private objectChanged(target?: string) {
    for (const [t, fns] of this.objects) if (target === undefined || t === target) fns.forEach((fn) => fn());
  }

  private emit() {
    for (const fn of [...this.listeners]) fn();
  }

  /** Begin, as `account`, once control has signed it in: ask whether
   * chat is on, and if it is, load the rooms and open the stream. While
   * it's off, ask again now and then (a team owner may turn it on). */
  start(account: string, names: string[]) {
    this.me = account;
    this.myNames = names.map((n) => n.toLowerCase()).filter(Boolean);
    if (this.started) return;
    this.started = true;
    void this.check();
    addEventListener("visibilitychange", () => this.tellOpen());
  }

  /** Ask control whether chat is on (and load the rooms if it is). */
  async check() {
    clearTimeout(this.probe);
    let failed = false;
    try {
      await this.loadRooms();
      void this.loadSidebar();
      if (!this.es) this.listen();
    } catch (e) {
      if (e instanceof ChatError && (e.reason === "chat_off" || e.status === 401 || e.status === 404)) this.turnOff(e.status === 401);
      // Control didn't answer, or a maintenance window (M141) outlasted
      // the wait: as it was, and ask again soon, the live stream too.
      else failed = true;
    }
    if (failed !== this.unreachable) {
      this.unreachable = failed;
      this.emit();
    }
    if (this.on !== true || failed) this.probe = window.setTimeout(() => void this.check(), failed ? 15_000 : 60_000);
  }

  private turnOff(signedOut: boolean) {
    const was = this.on;
    const wasOut = this.signedOut;
    this.on = false;
    this.signedOut = signedOut;
    this.es?.close();
    this.es = null;
    this.stream = null;
    this.rooms = [];
    this.sidebar = emptySidebar();
    this.saved = null;
    this.held.clear();
    this.loaded.clear();
    this.detail.clear();
    presence.stop();
    if (was !== false || wasOut !== signedOut) this.emit();
  }

  async loadRooms() {
    const r = await call<ChatRooms>("GET", "/api/chat");
    this.on = true;
    this.signedOut = false;
    presence.start();
    this.rooms = r.rooms;
    this.threadsUnread = { n: r.threads_unread ?? 0, mentions: r.threads_mentions ?? 0 };
    this.emit();
  }

  /** Edits sent and not yet answered: what is read meanwhile is older. */
  private sidebarPending = 0;

  async loadSidebar() {
    try {
      const s = await call<ChatSidebar>("GET", "/api/chat/sidebar");
      if (this.sidebarPending) return;
      this.sidebar = s;
      this.emit();
    } catch {
      // As it was: the rooms still list, in their kinds' groups.
    }
  }

  /** Change the sidebar: shown at once, kept on the server (the last write
   * wins), and told to the account's other pages. If control refuses, it's
   * read again so the page shows what is kept. */
  async editSidebar(change: (s: ChatSidebar) => ChatSidebar) {
    const was = this.sidebar;
    const next = change(was);
    if (next === was) return;
    this.sidebar = next;
    this.emit();
    this.sidebarPending++;
    try {
      const kept = await call<ChatSidebar>("PUT", "/api/chat/sidebar", next);
      if (this.sidebarPending === 1) this.sidebar = kept;
    } catch (e) {
      this.sidebarPending--;
      await this.loadSidebar();
      throw e;
    }
    this.sidebarPending--;
    this.emit();
  }

  /** The room list again, soon: several events in a row load it once. */
  private roomsSoon() {
    clearTimeout(this.roomsTimer);
    this.roomsTimer = window.setTimeout(() => {
      this.loadRooms().catch((e) => {
        if (e instanceof ChatError && e.reason === "chat_off") this.turnOff(false);
      });
    }, 120);
  }

  private listen() {
    if (typeof EventSource === "undefined") return;
    const es = new EventSource("/api/chat/live");
    this.es = es;
    es.onmessage = (e) => {
      let ev: ChatLive;
      try {
        ev = JSON.parse(e.data as string) as ChatLive;
      } catch {
        return;
      }
      this.heard(ev);
    };
    es.onerror = () => {
      // EventSource reconnects by itself (with Last-Event-ID); a refusal
      // (chat turned off, signed out, a maintenance window's 503) closes
      // it for good: ask again, which waits a window out before listening.
      if (es.readyState === EventSource.CLOSED && this.es === es) {
        this.es = null;
        this.stream = null;
        window.setTimeout(() => void this.check(), 5_000);
      }
    };
  }

  private heard(ev: ChatLive) {
    switch (ev.type) {
      case "hello":
        this.stream = ev.stream;
        this.tellOpen(true);
        void this.loadSaved();
        // Anything missed while there was no stream.
        this.roomsSoon();
        void this.loadSidebar();
        for (const room of this.loaded.keys()) void this.newer(room);
        for (const room of this.pinned.keys()) void this.loadPins(room);
        this.threads.reloadAll();
        this.objectChanged();
        return;
      case "object":
        this.objectChanged(ev.target);
        return;
      case "session":
        // M257: a session's card reads it again (`onObject(ses)`).
        this.objectChanged(ev.ses);
        return;
      case "message":
        this.roomsSoon();
        if (this.loaded.has(ev.room)) void this.newer(ev.room);
        return;
      case "changed":
        if (this.loaded.has(ev.room)) void this.refetch(ev.room, ev.seq);
        this.roomsSoon();
        return;
      case "reacted":
        // Only the message: no room's count or order moved.
        if (this.loaded.has(ev.room)) void this.refetch(ev.room, ev.seq);
        return;
      case "pinned":
        if (this.pinned.has(ev.room) || this.loaded.has(ev.room)) void this.loadPins(ev.room);
        return;
      case "reply":
      case "threadread":
        // A thread moved (#106): its first message's summary in the room's
        // list, the thread if it's open, and my Threads badge.
        if (this.loaded.has(ev.room)) void this.refetch(ev.room, ev.parent);
        this.threads.heard(ev);
        this.roomsSoon();
        return;
      case "read": {
        const r = this.room(ev.room);
        if (r && ev.seq !== r.read) {
          // Backwards is another page marking a message unread (#116):
          // this one, if it shows the room, keeps it unread too.
          if (ev.seq < r.read) this.held.set(ev.room, ev.seq + 1);
          this.rooms = this.rooms.map((x) => (x.id === ev.room ? { ...x, read: ev.seq } : x));
          this.emit();
        }
        this.roomsSoon();
        return;
      }
      case "sidebar":
        void this.loadSidebar();
        return;
      case "room":
        this.detail.delete(ev.room);
        this.roomsSoon();
        return;
      case "saved":
        void this.loadSaved();
        return;
      case "typing":
        this.typers.set(`${ev.room} ${ev.account}`, { ...ev, until: Date.now() + TYPING_MS });
        this.emit();
        return;
      case "resync":
        void this.loadSaved();
        this.typers.clear();
        this.threads.clear();
        this.objectChanged();
        this.loaded.clear();
        this.detail.clear();
        this.pinned.clear();
        this.roomsSoon();
        void this.loadSidebar();
        this.emit();
        return;
    }
  }

  // ---- what the page has open

  /** The room `by` shows (null: it shows none), so control pushes nobody
   * here for it. A hidden page has none open. `by` is whatever one view
   * of a room passes every time; the default is the page's only one. */
  setOpen(room: string | null, by: unknown = this) {
    const was = this.open.get(by) ?? null;
    if (room === was) return;
    if (room === null) this.open.delete(by);
    else this.open.set(by, room);
    // Closed (#116): a room marked unread is no longer held, unless another view still shows it.
    if (was && ![...this.open.values()].includes(was)) this.held.delete(was);
    this.tellOpen();
  }

  /** The thread open beside it (#106): replies in it aren't pushed for. */
  private openThread: number | null = null;
  setOpenThread(parent: number | null) {
    if (parent === this.openThread) return;
    this.openThread = parent;
    this.tellOpen();
  }

  private told: string | undefined = undefined;
  private tellOpen(force = false) {
    if (!this.stream) return;
    const rooms = document.visibilityState === "visible" ? [...new Set(this.open.values())] : [];
    // The thread is the first room's (the one a page shows first).
    const thread = rooms.length ? this.openThread : null;
    const key = `${rooms.join(" ")}|${thread ?? ""}`;
    if (!force && key === this.told) return;
    this.told = key;
    void call("POST", "/api/chat/live/open", { stream: this.stream, room: rooms[0] ?? null, more: rooms.slice(1), ...(thread !== null ? { thread } : {}) }).catch(() => {});
  }

  // ---- typing (#111)

  /** Who else is typing in a room now, by name, longest first. */
  typingIn(room: string): string[] {
    const now = Date.now();
    return [...this.typers.values()].filter((t) => t.room === room && t.until > now && t.account !== this.me).map((t) => t.name);
  }

  /** The next time a name in a room stops showing, for a redraw. */
  typingEnds(room: string): number | null {
    const ends = [...this.typers.values()].filter((t) => t.room === room).map((t) => t.until);
    return ends.length ? Math.min(...ends) : null;
  }

  /** Tell the room I'm typing: at most one ping per [`TYPING_PING_MS`] per
   * room, and the caller sends none while the composer is empty. */
  typing(room: string) {
    const now = Date.now();
    if (now - (this.typed.get(room) ?? 0) < TYPING_PING_MS) return;
    this.typed.set(room, now);
    void call("POST", `/api/chat/rooms/${enc(room)}/typing`).catch(() => {});
  }

  /** Whoever wrote a message has stopped typing it. */
  private sent(room: string, list: ChatMessage[]) {
    let any = false;
    for (const m of list) if (m.author && this.typers.delete(`${room} ${m.author}`)) any = true;
    if (any) this.emit();
  }

  // ---- rooms

  room(id: string): ChatRoom | undefined {
    return this.rooms.find((r) => r.id === id);
  }

  /** The thread on a pane or a session, if it's been made. */
  roomOf(machine: string, kind: ChatTargetKind, target: string | number): ChatRoom | undefined {
    const t = String(target);
    return this.rooms.find((r) => r.kind === kind && r.machine === machine && r.target === t);
  }

  /** Unread on a pane's thread, for its badge and the swarm's corner. */
  paneUnread(machine: string | undefined, pane: number): { unread: number; mention: boolean; room?: ChatRoom } {
    const r = machine ? this.roomOf(machine, "pane", pane) : undefined;
    return { unread: r?.unread ?? 0, mention: !!r?.mentions, room: r };
  }

  /** What's unread in the rooms the chat page lists, and whether any of
   * it mentions me: the Chat button's count. Muted rooms count only their
   * mentions. An object's thread (an issue's, a run's) counts on its
   * object's list and in the inbox instead (./chat/unread). */
  unread(): { n: number; mention: boolean } {
    return chatUnread(this.rooms);
  }

  async detailOf(id: string, fresh = false): Promise<ChatRoomDetail> {
    const had = this.detail.get(id);
    if (had && !fresh) return had;
    const d = await call<ChatRoomDetail>("GET", `/api/chat/rooms/${enc(id)}`);
    this.detail.set(id, d);
    this.emit();
    return d;
  }

  /** Who `@` reaches in a room, as loaded (empty until it is). */
  people(id: string): ChatPerson[] {
    return this.detail.get(id)?.people ?? [];
  }

  /** Whether I add and remove a channel's members, as loaded (#118). */
  canManage(id: string): boolean {
    return this.detail.get(id)?.can_manage ?? false;
  }

  /** The machines whose agents a grant lets read or post in a room (M197),
   * as loaded (empty until it is). */
  agents(id: string): ChatRoomAgent[] {
    return this.detail.get(id)?.agents ?? [];
  }

  private took(room: ChatRoom) {
    const i = this.rooms.findIndex((r) => r.id === room.id);
    this.rooms = i < 0 ? [...this.rooms, room] : this.rooms.map((r) => (r.id === room.id ? room : r));
    this.emit();
    return room;
  }

  async newChannel(team: string, name: string, opts: { private?: boolean; topic?: string } = {}): Promise<ChatRoom> {
    return this.took(await call<ChatRoom>("POST", `/api/chat/teams/${enc(team)}/channels`, { name, ...opts }));
  }

  /** The DM with one person or, with up to seven more, a group's: the same
   * people reopen it (#117). */
  async newDm(team: string, who: string | string[]): Promise<ChatRoom> {
    const [first, ...and] = typeof who === "string" ? [who] : who;
    return this.took(await call<ChatRoom>("POST", `/api/chat/teams/${enc(team)}/dms`, { with: first, and }));
  }

  /** The team's public channels, joined or not (#118). */
  async channels(team: string, q = ""): Promise<ChatChannel[]> {
    const r = await call<ChatChannels>("GET", `/api/chat/teams/${enc(team)}/channels${q ? `?q=${enc(q)}` : ""}`);
    return r.channels;
  }

  async join(id: string): Promise<ChatRoom> {
    return this.took(await call<ChatRoom>("POST", `/api/chat/rooms/${enc(id)}/join`));
  }

  /** Out of a channel or a DM: it leaves the list (a public channel stays
   * in the browser). */
  async leave(id: string) {
    await call<unknown>("POST", `/api/chat/rooms/${enc(id)}/leave`);
    this.rooms = this.rooms.filter((r) => r.id !== id);
    this.detail.delete(id);
    this.emit();
  }

  async addMembers(id: string, accounts: string[]) {
    this.took(await call<ChatRoom>("POST", `/api/chat/rooms/${enc(id)}/members`, { accounts }));
    await this.detailOf(id, true);
  }

  async removeMember(id: string, account: string) {
    this.took(await call<ChatRoom>("DELETE", `/api/chat/rooms/${enc(id)}/members/${enc(account)}`));
    await this.detailOf(id, true);
  }

  /** The thread on a pane or a session (made the first time). */
  async openTarget(machine: string, kind: ChatTargetKind, target: string | number): Promise<ChatRoom> {
    return this.took(await call<ChatRoom>("POST", "/api/chat/targets", { machine, kind, target: String(target) }));
  }

  /** The thread on an object (M196: a native issue, `iss_…`), made the
   * first time; for a member of its project. */
  async openObject(target: string): Promise<ChatRoom> {
    return this.took(await call<ChatRoom>("POST", "/api/chat/targets", { machine: "", kind: "object", target }));
  }

  async rename(id: string, name: string) {
    return this.took(await call<ChatRoom>("PATCH", `/api/chat/rooms/${enc(id)}`, { name }));
  }

  async archive(id: string, archived: boolean) {
    return this.took(await call<ChatRoom>("PATCH", `/api/chat/rooms/${enc(id)}`, { archived }));
  }

  async mute(id: string, muted: boolean) {
    await call("POST", `/api/chat/rooms/${enc(id)}/mute`, { muted });
    this.rooms = this.rooms.map((r) => (r.id === id ? { ...r, muted } : r));
    this.emit();
  }

  /** What I'm notified of in a room (#113); none is my default for rooms
   * like it. The server says what that comes to. */
  async setLevel(id: string, level: NotifyLevel | null) {
    return this.took(await call<ChatRoom>("POST", `/api/chat/rooms/${enc(id)}/level`, { level }));
  }

  /** Read up to `seq` (it only moves forward). */
  async read(id: string, seq: number) {
    const r = this.room(id);
    if (!r || (seq <= r.read && r.unread === 0)) return;
    this.rooms = this.rooms.map((x) => (x.id === id ? { ...x, read: Math.max(x.read, seq), unread: 0, mentions: 0 } : x));
    this.emit();
    await call("POST", `/api/chat/rooms/${enc(id)}/read`, { seq }).catch(() => {});
  }

  /** Mark a message unread (#116): the room's read mark goes to just
   * before it, here and on my other pages. The room stays unread while it's
   * open (`held`), with its "New" line at the message. */
  async markUnread(id: string, seq: number) {
    this.held.set(id, seq);
    try {
      this.took(await call<ChatRoom>("POST", `/api/chat/rooms/${enc(id)}/unread`, { seq }));
    } catch (e) {
      this.held.delete(id);
      throw e;
    }
  }

  /** The message my "New" line is at in a room I marked unread, while it's
   * open. */
  heldAt(id: string): number | undefined {
    return this.held.get(id);
  }

  // ---- saved for later (#116)

  async loadSaved() {
    try {
      this.saved = (await call<ChatSaved>("GET", "/api/chat/saved")).items;
      this.emit();
    } catch {
      // Chat off, or control away: as it was.
    }
  }

  isSaved(room: string, seq: number): boolean {
    return !!this.saved?.some((i) => i.room === room && i.seq === seq && i.done === null);
  }

  /** What's saved and not done: the Saved view's count. */
  savedTodo(): number {
    return this.saved?.filter((i) => i.done === null).length ?? 0;
  }

  /** Save a message, with a reminder at `remindAt` (ms) if given; saving
   * it again changes the reminder and takes it out of Done. */
  async save(room: string, seq: number, remindAt?: number) {
    await call("PUT", `/api/chat/rooms/${enc(room)}/messages/${seq}/saved`, remindAt === undefined ? {} : { remind_at: remindAt });
    await this.loadSaved();
  }

  async savedDone(room: string, seq: number, done: boolean) {
    await call("PATCH", `/api/chat/rooms/${enc(room)}/messages/${seq}/saved`, { done });
    await this.loadSaved();
  }

  async unsave(room: string, seq: number) {
    await call("DELETE", `/api/chat/rooms/${enc(room)}/messages/${seq}/saved`);
    await this.loadSaved();
  }

  /** Everything the chat page lists read, as its button counts it. The
   * threads on objects stay as they are: they're read on their objects'
   * pages, or from the inbox. */
  /** Every listed room read; `only`: of these (a project's, UI audit #169). */
  async readAll(only: (r: ChatRoom) => boolean = () => true) {
    await Promise.all(this.rooms.filter((r) => listedInChat(r) && r.unread && only(r)).map((r) => this.read(r.id, r.last)));
  }

  // ---- messages

  messages(id: string): Loaded | undefined {
    return this.loaded.get(id);
  }

  /** The newest page of a room, once (later ones come by the stream). */
  load(id: string): Promise<void> {
    if (this.loaded.has(id)) return Promise.resolve();
    return this.once(`load:${id}`, async () => {
      const r = await call<ChatMessages>("GET", `/api/chat/rooms/${enc(id)}/messages`);
      this.loaded.set(id, { list: r.messages, older: r.more });
      this.took(r.room);
    });
  }

  /** The page before the oldest loaded. */
  older(id: string): Promise<void> {
    const had = this.loaded.get(id);
    if (!had?.older || !had.list.length) return Promise.resolve();
    return this.once(`older:${id}`, async () => {
      const r = await call<ChatMessages>("GET", `/api/chat/rooms/${enc(id)}/messages?before=${had.list[0].seq}`);
      const now = this.loaded.get(id) ?? had;
      this.loaded.set(id, { list: merge(r.messages, now.list), older: r.more });
      this.emit();
    });
  }

  /** What's come since the newest loaded. */
  private newer(id: string): Promise<void> {
    return this.once(`newer:${id}`, async () => {
      for (;;) {
        const had = this.loaded.get(id);
        if (!had) return;
        const last = had.list.at(-1)?.seq;
        const r = await call<ChatMessages>("GET", `/api/chat/rooms/${enc(id)}/messages${last === undefined ? "" : `?after=${last}`}`).catch(
          (e: unknown) => {
            // Gone, or not ours any more.
            if (e instanceof ChatError && e.status === 404) this.loaded.delete(id);
            return null;
          },
        );
        if (!r) return this.emit();
        // Into the list as it is now: a pin, reaction or edit may have
        // landed in it while this was asked.
        const now = this.loaded.get(id) ?? had;
        this.loaded.set(id, { list: merge(now.list, r.messages), older: last === undefined ? r.more : now.older });
        this.sent(id, r.messages);
        this.took(r.room);
        if (last === undefined || !r.more) return;
      }
    });
  }

  /** One message again (edited or deleted). */
  private async refetch(id: string, seq: number) {
    const r = await call<ChatMessages>("GET", `/api/chat/rooms/${enc(id)}/messages?after=${seq - 1}&limit=1`).catch(() => null);
    const had = this.loaded.get(id);
    if (!r || !had) return;
    this.loaded.set(id, { ...had, list: merge(had.list, r.messages.filter((m) => m.seq === seq)) });
    this.emit();
  }

  private once(key: string, fn: () => Promise<void>): Promise<void> {
    const had = this.fetching.get(key);
    if (had) return had;
    const p = fn().finally(() => this.fetching.delete(key));
    this.fetching.set(key, p);
    return p;
  }

  async post(
    id: string,
    text: string,
    quote?: PaneQuote,
    files?: string[] | null,
    notifyAnyway?: boolean,
  ): Promise<ChatPosted> {
    const body: ChatPost = { text };
    if (files?.length) body.files = files;
    if (notifyAnyway) body.notify_anyway = true;
    if (quote) {
      body.pane = { machine: quote.machine, pane: String(quote.pane) } satisfies ChatPaneRef;
      if (quote.text) body.quote = quote.text;
    }
    const posted = await call<ChatPosted>("POST", `/api/chat/rooms/${enc(id)}/messages`, body);
    if (!this.loaded.has(id)) this.loaded.set(id, { list: [], older: true });
    await this.newer(id);
    return posted;
  }

  /** Reply in the thread of message `parent` (#106). Sent to the channel too
   * (`also`), the room's list reads it back as well. */
  async reply(id: string, parent: number, text: string, also: boolean, files?: string[] | null): Promise<ChatPosted> {
    const posted = await this.threads.post(id, parent, text, also, files);
    if (also && this.loaded.has(id)) await this.newer(id);
    // The first message's summary, in the room's list.
    if (this.loaded.has(id)) void this.refetch(id, parent);
    return posted;
  }

  async edit(id: string, seq: number, text: string) {
    const m = await call<ChatMessage>("PATCH", `/api/chat/rooms/${enc(id)}/messages/${seq}`, { text });
    this.replace(id, m);
  }

  async remove(id: string, seq: number) {
    const m = await call<ChatMessage>("DELETE", `/api/chat/rooms/${enc(id)}/messages/${seq}`);
    this.replace(id, m);
  }

  /** React to a message with `emoji`, or take mine back (#104). It shows at
   * once and the server's answer replaces it; a refusal puts it back. */
  async react(id: string, seq: number, emoji: string, on: boolean) {
    const was = this.loaded.get(id)?.list.find((m) => m.seq === seq) ?? this.threads.find(id, seq);
    if (was) this.replace(id, reacted(was, emoji, on, this.me));
    const path = `/api/chat/rooms/${enc(id)}/messages/${seq}/reactions`;
    try {
      this.replace(id, on ? await call<ChatMessage>("POST", path, { emoji }) : await call<ChatMessage>("DELETE", `${path}?emoji=${enc(emoji)}`));
    } catch (e) {
      if (was) this.replace(id, was);
      throw e;
    }
  }

  private replace(id: string, m: ChatMessage) {
    this.threads.replace(id, m);
    const had = this.loaded.get(id);
    // A reply is in the room's list only when it was sent there too.
    if (!had || (m.parent !== undefined && !m.also && !had.list.some((x) => x.seq === m.seq))) return this.emit();
    this.loaded.set(id, { ...had, list: merge(had.list, [m]) });
    this.emit();
  }

  // ---- topic and pins (#119)

  /** A room's pins, newest first, as loaded (`undefined` until they are). */
  pinsOf(id: string): ChatPin[] | undefined {
    return this.pinned.get(id);
  }

  /** Ask for a room's pins, and mark its loaded messages pinned or not. */
  async loadPins(id: string) {
    const r = await call<ChatPins>("GET", `/api/chat/rooms/${enc(id)}/pins`).catch(() => null);
    if (!r) return;
    this.pinned.set(id, r.pins);
    const had = this.loaded.get(id);
    if (had) {
      const set = new Set(r.pins.map((p) => p.message.seq));
      this.loaded.set(id, { ...had, list: had.list.map((m) => (m.pinned === set.has(m.seq) ? m : { ...m, pinned: set.has(m.seq) })) });
    }
    this.emit();
  }

  /** Pin a message, or take its pin off. The stream tells every page,
   * this one too; the answer shows it here at once. */
  async pin(id: string, seq: number, on: boolean) {
    const path = `/api/chat/rooms/${enc(id)}/messages/${seq}/pin`;
    this.replace(id, await call<ChatMessage>(on ? "PUT" : "DELETE", path));
    void this.loadPins(id);
  }

  /** Write a channel's topic and/or description (what's left out stays). */
  async setTopic(id: string, change: { topic?: string; description?: string }) {
    return this.took(await call<ChatRoom>("PUT", `/api/chat/rooms/${enc(id)}/topic`, change));
  }

  /** Search the rooms I see (words, `mea*`, `in:#ch`, `from:@login`,
   * `has:quote`), newest first. */
  async search(q: string, team?: string): Promise<ChatFound> {
    return call<ChatFound>("GET", `/api/chat/search?q=${enc(q)}${team ? `&team=${enc(team)}` : ""}`);
  }

  /** Whether a message is mine: written by me, not by my agent (M197). */
  mine(m: ChatMessage): boolean {
    return !!m.author && m.author === this.me && !m.agent;
  }

  /** Whether an `@word` names me. */
  isMe = (token: string): boolean => this.myNames.includes(token.toLowerCase());
}

/** Two runs of messages as one, by number, the later copy winning. */
function merge(a: ChatMessage[], b: ChatMessage[]): ChatMessage[] {
  const by = new Map<number, ChatMessage>();
  for (const m of a) by.set(m.seq, m);
  for (const m of b) by.set(m.seq, m);
  return [...by.values()].sort((x, y) => x.seq - y.seq);
}

/** The page's one store; `chatStore()` is it while chat is on in control. */
export const chat = new ChatStore();

export function chatStore(): ChatStore | null {
  return chat.on === true ? chat : null;
}
