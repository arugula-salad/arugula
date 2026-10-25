// The threads the page has open (#106): a thread's first message with its
// replies as far as they're fetched, and the Threads view's list. The
// store (../chat-store) owns one and tells it what the stream says; it
// reads and writes through the store's `call`, so there is one place a
// request leaves from.

import type { ChatLive, ChatMessage, ChatMessages, ChatPost, ChatPosted, ChatThreads } from "../proto";
import { followed, mergeReplies, readTo } from "./threads";

type Call = <T>(method: string, path: string, body?: unknown) => Promise<T>;

const enc = encodeURIComponent;

/** A thread as far as the page has it. */
export interface OpenThread {
  room: string;
  /** Its first message's number. */
  seq: number;
  /** Its first message, as the server last said (its `thread` is the
   * reader's own mark and following), once fetched. */
  parent: ChatMessage | null;
  /** The replies, oldest first. */
  replies: ChatMessage[];
  /** Earlier replies are left to fetch. */
  older: boolean;
}

export const threadKey = (room: string, parent: number) => `${room}/${parent}`;

export class ThreadState {
  private open = new Map<string, OpenThread>();
  private fetching = new Map<string, Promise<void>>();
  /** The Threads view's list, once asked for. */
  list: ChatThreads | null = null;
  private watching = 0;
  private timer: number | undefined;

  /** `changed`: redraw. `parentChanged`: the first message of a thread has
   * a new summary, which the room's list should show too. */
  constructor(
    private call: Call,
    private changed: () => void,
    private parentChanged: (room: string, parent: ChatMessage) => void,
    private mine: (m: ChatMessage) => boolean,
  ) {}

  get(room: string, parent: number): OpenThread | undefined {
    return this.open.get(threadKey(room, parent));
  }

  /** A message the page has in an open thread: its first, or a reply. */
  find(room: string, seq: number): ChatMessage | undefined {
    for (const t of this.open.values()) {
      if (t.room !== room) continue;
      if (t.parent?.seq === seq) return t.parent;
      const m = t.replies.find((x) => x.seq === seq);
      if (m) return m;
    }
    return undefined;
  }

  private once(key: string, fn: () => Promise<void>): Promise<void> {
    const had = this.fetching.get(key);
    if (had) return had;
    const p = fn().finally(() => this.fetching.delete(key));
    this.fetching.set(key, p);
    return p;
  }

  /** The newest page of a thread, once; later replies come by the stream. */
  load(room: string, parent: number): Promise<void> {
    if (this.get(room, parent)?.parent) return Promise.resolve();
    return this.once(`load:${threadKey(room, parent)}`, async () => {
      const r = await this.call<ChatMessages>("GET", `/api/chat/rooms/${enc(room)}/messages?thread=${parent}`);
      this.open.set(threadKey(room, parent), { room, seq: parent, parent: r.parent ?? null, replies: r.messages, older: r.more });
      if (r.parent) this.parentChanged(room, r.parent);
      this.changed();
    });
  }

  /** The page before the oldest reply loaded. */
  older(room: string, parent: number): Promise<void> {
    const had = this.get(room, parent);
    if (!had?.older || !had.replies.length) return Promise.resolve();
    return this.once(`older:${threadKey(room, parent)}`, async () => {
      const r = await this.call<ChatMessages>("GET", `/api/chat/rooms/${enc(room)}/messages?thread=${parent}&before=${had.replies[0].seq}`);
      const now = this.get(room, parent) ?? had;
      this.open.set(threadKey(room, parent), { ...now, replies: mergeReplies(r.messages, now.replies), older: r.more });
      this.changed();
    });
  }

  /** What's come since the newest reply loaded. */
  newer(room: string, parent: number): Promise<void> {
    return this.once(`newer:${threadKey(room, parent)}`, async () => {
      for (;;) {
        const had = this.get(room, parent);
        if (!had) return;
        const last = had.replies.at(-1)?.seq;
        const r = await this.call<ChatMessages>("GET", `/api/chat/rooms/${enc(room)}/messages?thread=${parent}${last === undefined ? "" : `&after=${last}`}`).catch(() => null);
        if (!r) return;
        const now = this.get(room, parent) ?? had;
        this.open.set(threadKey(room, parent), { ...now, parent: r.parent ?? now.parent, replies: mergeReplies(now.replies, r.messages) });
        if (r.parent) this.parentChanged(room, r.parent);
        this.changed();
        if (last === undefined || !r.more) return;
      }
    });
  }

  /** One reply again (edited, deleted or reacted to). */
  async refetch(room: string, parent: number, seq: number) {
    const r = await this.call<ChatMessages>("GET", `/api/chat/rooms/${enc(room)}/messages?thread=${parent}&after=${seq - 1}&limit=1`).catch(() => null);
    const had = this.get(room, parent);
    if (!r || !had) return;
    this.open.set(threadKey(room, parent), { ...had, parent: r.parent ?? had.parent, replies: mergeReplies(had.replies, r.messages.filter((m) => m.seq === seq)) });
    if (r.parent) this.parentChanged(room, r.parent);
    this.changed();
  }

  /** A message the page changed (an edit, a reaction): in the thread it's in. */
  replace(room: string, m: ChatMessage) {
    if (m.parent === undefined) {
      // A first message: a deletion or a reaction on it.
      const had = this.get(room, m.seq);
      if (had) this.open.set(threadKey(room, m.seq), { ...had, parent: m });
      return;
    }
    const had = this.get(room, m.parent);
    if (had) this.open.set(threadKey(room, m.parent), { ...had, replies: mergeReplies(had.replies, [m]) });
  }

  /** Write a reply. The thread reads it back, and the room's list does when
   * it was also sent there (the store's job). */
  async post(room: string, parent: number, text: string, also: boolean, files?: string[] | null): Promise<ChatPosted> {
    const body: ChatPost = { text, parent, also };
    if (files?.length) body.files = files;
    const posted = await this.call<ChatPosted>("POST", `/api/chat/rooms/${enc(room)}/messages`, body);
    await this.newer(room, parent);
    this.soon();
    return posted;
  }

  /** Read the thread up to `seq`: it shows at once. */
  async read(room: string, parent: number, seq: number) {
    const had = this.get(room, parent);
    const first = had?.parent;
    if (!had || !first?.thread || seq <= first.thread.read) return;
    const next = readTo(first, seq, had.replies, this.mine);
    this.open.set(threadKey(room, parent), { ...had, parent: next });
    this.parentChanged(room, next);
    this.changed();
    await this.call("POST", `/api/chat/rooms/${enc(room)}/threads/${parent}/read`, { seq }).catch(() => {});
    this.soon();
  }

  /** Follow or stop: it shows at once, and the server's answer replaces it. */
  async follow(room: string, parent: number, on: boolean) {
    const had = this.get(room, parent);
    const first = had?.parent;
    if (had && first) {
      const next = followed(first, on, had.replies.at(-1)?.seq ?? 0);
      this.open.set(threadKey(room, parent), { ...had, parent: next });
      this.parentChanged(room, next);
      this.changed();
    }
    try {
      const m = await this.call<ChatMessage>("POST", `/api/chat/rooms/${enc(room)}/threads/${parent}/follow`, { following: on });
      const now = this.get(room, parent);
      if (now) this.open.set(threadKey(room, parent), { ...now, parent: m });
      this.parentChanged(room, m);
    } catch (e) {
      if (had) this.open.set(threadKey(room, parent), had);
      if (first) this.parentChanged(room, first);
      throw e;
    } finally {
      this.changed();
      this.soon();
    }
  }

  // ---- the Threads view

  /** The Threads view is showing: keep its list current. Returns the stop. */
  watch(): () => void {
    this.watching++;
    void this.loadList();
    return () => void this.watching--;
  }

  async loadList() {
    this.list = await this.call<ChatThreads>("GET", "/api/chat/threads");
    this.changed();
  }

  /** The list again, soon, if anyone is looking at it. */
  private soon() {
    if (!this.watching) return;
    clearTimeout(this.timer);
    this.timer = window.setTimeout(() => void this.loadList().catch(() => {}), 150);
  }

  // ---- the stream

  /** Anything fetched may have missed something: read what's open again. */
  reloadAll() {
    for (const t of this.open.values()) void this.newer(t.room, t.seq);
    this.soon();
  }

  clear() {
    this.open.clear();
    this.list = null;
  }

  /** What the stream said of threads. Whether it was one's. */
  heard(ev: ChatLive): boolean {
    switch (ev.type) {
      case "reply": {
        const open = this.get(ev.room, ev.parent);
        // A new reply, or one changed: the newer ones, and that one again.
        if (open) void this.newer(ev.room, ev.parent).then(() => this.refetch(ev.room, ev.parent, ev.seq));
        this.soon();
        return true;
      }
      case "threadread": {
        // My own mark moved (another page of mine), or I followed there.
        if (this.get(ev.room, ev.parent)) void this.newer(ev.room, ev.parent);
        this.soon();
        return true;
      }
      default:
        return false;
    }
  }
}
