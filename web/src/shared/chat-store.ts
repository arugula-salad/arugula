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
  ChatFound,
  ChatLive,
  ChatMessage,
  ChatMessages,
  ChatPaneRef,
  ChatPerson,
  ChatPost,
  ChatPosted,
  ChatRoom,
  ChatRoomAgent,
  ChatRoomDetail,
  ChatRooms,
  ChatTargetKind,
} from "./proto";
import { platformFetch, refusalOf } from "./maintenance";
import { reacted } from "./chat/reactions";
import { chatUnread, listedInChat } from "./chat/unread";
import { presence } from "./chat/presence";

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
  /** The account reading: control's id for it. */
  me = "";
  /** What `@` of me looks like: my login and name, lowercased. */
  myNames: string[] = [];
  rooms: ChatRoom[] = [];
  /** This page's live stream, once control said hello. */
  stream: string | null = null;
  private loaded = new Map<string, Loaded>();
  private detail = new Map<string, ChatRoomDetail>();
  private fetching = new Map<string, Promise<void>>();
  private es: EventSource | null = null;
  private open: string | null = null;
  private listeners = new Set<() => void>();
  private probe: number | undefined;
  private roomsTimer: number | undefined;
  private started = false;
  /** Who is typing where (#111), by `room account`, until when. Never
   * fetched: only what the stream said in the last few seconds. */
  private typers = new Map<string, { room: string; account: string; name: string; until: number }>();
  private typed = new Map<string, number>();

  /** Whether control has signed this page in (so chat is only off). */
  get signedIn(): boolean {
    return this.started;
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => void this.listeners.delete(fn);
  }

  /** Who wants to hear that what an object's thread is on changed (M203:
   * a native issue), by its id. */
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
      if (!this.es) this.listen();
    } catch (e) {
      if (e instanceof ChatError && (e.reason === "chat_off" || e.status === 401 || e.status === 404)) this.turnOff();
      // Control didn't answer, or a maintenance window (M141) outlasted
      // the wait: as it was, and ask again soon, the live stream too.
      else failed = true;
    }
    if (this.on !== true || failed) this.probe = window.setTimeout(() => void this.check(), failed ? 15_000 : 60_000);
  }

  private turnOff() {
    const was = this.on;
    this.on = false;
    this.es?.close();
    this.es = null;
    this.stream = null;
    this.rooms = [];
    this.loaded.clear();
    this.detail.clear();
    presence.stop();
    if (was !== false) this.emit();
  }

  async loadRooms() {
    const r = await call<ChatRooms>("GET", "/api/chat");
    this.on = true;
    presence.start();
    this.rooms = r.rooms;
    this.emit();
  }

  /** The room list again, soon: several events in a row load it once. */
  private roomsSoon() {
    clearTimeout(this.roomsTimer);
    this.roomsTimer = window.setTimeout(() => {
      this.loadRooms().catch((e) => {
        if (e instanceof ChatError && e.reason === "chat_off") this.turnOff();
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
        // Anything missed while there was no stream.
        this.roomsSoon();
        for (const room of this.loaded.keys()) void this.newer(room);
        this.objectChanged();
        return;
      case "object":
        this.objectChanged(ev.target);
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
      case "read": {
        const r = this.room(ev.room);
        if (r && ev.seq > r.read) {
          this.rooms = this.rooms.map((x) => (x.id === ev.room ? { ...x, read: ev.seq } : x));
          this.emit();
        }
        this.roomsSoon();
        return;
      }
      case "room":
        this.detail.delete(ev.room);
        this.roomsSoon();
        return;
      case "typing":
        this.typers.set(`${ev.room} ${ev.account}`, { ...ev, until: Date.now() + TYPING_MS });
        this.emit();
        return;
      case "resync":
        this.typers.clear();
        this.objectChanged();
        this.loaded.clear();
        this.detail.clear();
        this.roomsSoon();
        this.emit();
        return;
    }
  }

  // ---- what the page has open

  /** The room this page shows (null: none), so control pushes nobody here
   * for it. A hidden page has none open. */
  setOpen(room: string | null) {
    if (room === this.open) return;
    this.open = room;
    this.tellOpen();
  }

  private told: string | null | undefined = undefined;
  private tellOpen(force = false) {
    if (!this.stream) return;
    const room = document.visibilityState === "visible" ? this.open : null;
    if (!force && room === this.told) return;
    this.told = room;
    void call("POST", "/api/chat/live/open", { stream: this.stream, room }).catch(() => {});
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

  async newChannel(team: string, name: string): Promise<ChatRoom> {
    return this.took(await call<ChatRoom>("POST", `/api/chat/teams/${enc(team)}/channels`, { name }));
  }

  async newDm(team: string, withAccount: string): Promise<ChatRoom> {
    return this.took(await call<ChatRoom>("POST", `/api/chat/teams/${enc(team)}/dms`, { with: withAccount }));
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

  /** Read up to `seq` (it only moves forward). */
  async read(id: string, seq: number) {
    const r = this.room(id);
    if (!r || (seq <= r.read && r.unread === 0)) return;
    this.rooms = this.rooms.map((x) => (x.id === id ? { ...x, read: Math.max(x.read, seq), unread: 0, mentions: 0 } : x));
    this.emit();
    await call("POST", `/api/chat/rooms/${enc(id)}/read`, { seq }).catch(() => {});
  }

  /** Everything the chat page lists read, as its button counts it. The
   * threads on objects stay as they are: they're read on their objects'
   * pages, or from the inbox. */
  async readAll() {
    await Promise.all(this.rooms.filter((r) => listedInChat(r) && r.unread).map((r) => this.read(r.id, r.last)));
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
        this.loaded.set(id, { list: merge(had.list, r.messages), older: last === undefined ? r.more : had.older });
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

  async post(id: string, text: string, quote?: PaneQuote): Promise<ChatPosted> {
    const body: ChatPost = { text };
    if (quote) {
      body.pane = { machine: quote.machine, pane: String(quote.pane) } satisfies ChatPaneRef;
      if (quote.text) body.quote = quote.text;
    }
    const posted = await call<ChatPosted>("POST", `/api/chat/rooms/${enc(id)}/messages`, body);
    if (!this.loaded.has(id)) this.loaded.set(id, { list: [], older: true });
    await this.newer(id);
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
    const was = this.loaded.get(id)?.list.find((m) => m.seq === seq);
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
    const had = this.loaded.get(id);
    if (!had) return;
    this.loaded.set(id, { ...had, list: merge(had.list, [m]) });
    this.emit();
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
