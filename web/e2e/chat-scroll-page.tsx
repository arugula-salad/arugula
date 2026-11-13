// The fixture page for a room's scroll (#965): the real room over a fake
// control (its fetch is answered here), laid out as the shell lays a chat
// out on a phone: the page is `--app-height` tall, which follows the visual
// viewport, so a keyboard shrinks it. Two rooms of 120 messages (the newest
// 40 first, older ones 40 at a time), and `window.__fx` for the spec.
// chat-scroll-cases.ts builds this with Vite and serves it, so no daemon is
// involved.

import { render } from "preact";
import { useState } from "preact/hooks";
import "../src/shared/style.css";
import "../src/shared/buttons.css";
import { chat } from "../src/shared/chat-store";
import { RoomBody } from "../src/shared/chat/room";
import type { ChatMessage, ChatRoom } from "../src/shared/proto";

const ME = "acct_me";
const NEWEST = 120;
const PAGE = 40;

const room = (id: string, name: string, unread: number): ChatRoom =>
  ({
    id,
    kind: "channel",
    team: "tm_1",
    name,
    machine: null,
    machine_name: null,
    target: null,
    project: null,
    with: null,
    others: [],
    private: false,
    last: NEWEST,
    read: NEWEST - unread,
    unread,
    mentions: 0,
    wants: unread,
    muted: false,
    level: "all",
    level_own: false,
    archived: false,
    writable: true,
    topic: "",
    description: "",
    pins: 0,
    topic_writable: false,
  }) as unknown as ChatRoom;

const WORDS = "the quick brown fox jumps over the lazy dog and keeps going until the line wraps".split(" ");
const message = (seq: number): ChatMessage => {
  // Every third one is long, so the rows are different heights.
  const text = seq % 3 === 0 ? `Message ${seq}: ${Array.from({ length: 12 + (seq % 5) * 8 }, (_, i) => WORDS[(seq + i) % WORDS.length]).join(" ")}` : `Message ${seq}`;
  return {
    seq,
    at: Date.UTC(2026, 10, 1) + seq * 60_000,
    author: seq % 4 === 0 ? ME : "acct_other",
    name: seq % 4 === 0 ? "Me" : "Sam",
    text,
    quote: null,
    pane: null,
    mentions: [],
    edited: null,
    deleted: null,
    imported: null,
    agent: null,
    reactions: [],
    files: [],
    pinned: false,
    also: false,
  } as unknown as ChatMessage;
};

const rooms = new Map([
  ["ch_aaaa0001", room("ch_aaaa0001", "alpha", 0)],
  ["ch_aaaa0002", room("ch_aaaa0002", "beta", 0)],
  ["ch_aaaa0003", room("ch_aaaa0003", "gamma", 3)],
]);
const newest = new Map([...rooms.keys()].map((id) => [id, NEWEST]));

const json = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
const realFetch = window.fetch.bind(window);
window.fetch = async (input, init) => {
  const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
  const m = /^\/api\/chat\/rooms\/([^/]+)(?:\/(messages|read))?$/.exec(url.pathname);
  if (!m) return realFetch(input, init);
  const [, id, what] = m;
  const r = rooms.get(id);
  if (!r) return new Response("{}", { status: 404 });
  const last = newest.get(id)!;
  if (what === "messages" && init?.method === "POST") {
    const seq = last + 1;
    newest.set(id, seq);
    const text = JSON.parse(String(init.body)).text as string;
    (window.__fx.sent[id] ??= []).push({ ...message(seq), author: ME, name: "Me", text });
    return json({ seq, mentions: [], unreached: [] });
  }
  if (what === "messages") {
    const before = url.searchParams.get("before");
    const after = url.searchParams.get("after");
    const sent = window.__fx.sent[id] ?? [];
    let list: ChatMessage[];
    let more = false;
    if (before) {
      const hi = Number(before) - 1;
      const lo = Math.max(1, hi - PAGE + 1);
      list = Array.from({ length: hi - lo + 1 }, (_, i) => message(lo + i));
      more = lo > 1;
    } else if (after) {
      list = [...Array.from({ length: Math.max(0, NEWEST - Number(after)) }, (_, i) => message(Number(after) + 1 + i)), ...sent].filter((x) => x.seq > Number(after));
    } else {
      const lo = NEWEST - PAGE + 1;
      list = [...Array.from({ length: PAGE }, (_, i) => message(lo + i)), ...sent];
      more = true;
    }
    await window.__fx.hold?.(url.pathname + url.search);
    return json({ room: { ...r, last: newest.get(id)! }, messages: list, more });
  }
  if (what === "read") return json({});
  return json({ room: r, people: [], can_manage: false, agents: [] });
};

declare global {
  interface Window {
    __fx: {
      sent: Record<string, ChatMessage[]>;
      /** Waits before answering a read (a spec holds the older page). */
      hold?: (path: string) => Promise<void>;
      open(id: string): void;
      /** The keyboard, as an iPhone has it: the visual viewport shrinks, the layout viewport doesn't. */
      keyboard(px: number): void;
      /** Message `seq`'s top from the list's top edge (or the gap to the bottom), as painted
       * after each change of size, until `stop()`. */
      watch(what: number | "gap"): { clear(): void; stop(): number[] };
      top(seq: number): number | null;
    };
  }
}

let show: (id: string) => void = () => {};
chat.me = ME;
chat.rooms = [...rooms.values()];

// As the shell does (shell/main.tsx): what's visible, less the keyboard.
let keyboard = 0;
const vv = window.visualViewport!;
const fit = () => document.documentElement.style.setProperty("--app-height", `${(vv.height ?? innerHeight) - keyboard}px`);
vv.addEventListener("resize", fit);
fit();

const listOf = () => document.querySelector<HTMLElement>(".thread-list")!;
window.__fx = {
  sent: {},
  open: (id) => show(id),
  keyboard(px) {
    keyboard = px;
    fit();
    vv.dispatchEvent(new Event("resize"));
  },
  top(seq) {
    const at = document.querySelector(`[data-msg="${seq}"]`);
    return at ? at.getBoundingClientRect().top - listOf().getBoundingClientRect().top : null;
  },
  watch(what) {
    const el = listOf();
    const read = () => (what === "gap" ? el.scrollHeight - el.scrollTop - el.clientHeight : window.__fx.top(what));
    const seen: number[] = [];
    // After the list's own observer (made first) has put it back: a frame
    // sampled before it runs would show a layout that's never painted.
    const ro = new ResizeObserver(() => {
      const t = read();
      if (t !== null) seen.push(t);
    });
    const watch = (n: Node) => n instanceof Element && ro.observe(n, { box: "border-box" });
    ro.observe(el, { box: "border-box" });
    el.childNodes.forEach(watch);
    const mo = new MutationObserver((changes) => changes.forEach((c) => c.addedNodes.forEach(watch)));
    mo.observe(el, { childList: true });
    return {
      clear() {
        seen.length = 0;
      },
      stop() {
        ro.disconnect();
        mo.disconnect();
        return seen;
      },
    };
  },
};

/** A link's `?msg=` names a message in the first room. */
const focus = Number(new URLSearchParams(location.search).get("msg")) || undefined;

function Page() {
  const [id, setId] = useState("");
  show = setId;
  const r = id ? (chat.room(id) ?? rooms.get(id)) : undefined;
  return (
    <div style={{ display: "flex", flexDirection: "column", height: "var(--app-height)" }}>
      <div style={{ flex: "none" }}>
        {[...rooms.keys()].map((k) => (
          <button key={k} data-pick={k} onClick={() => setId(k)}>
            {k}
          </button>
        ))}
        <button data-pick="none" onClick={() => setId("")}>
          none
        </button>
      </div>
      <section class="chat-thread">
        {r && <RoomBody room={r} phone name={r.name} focus={focus} />}
      </section>
    </div>
  );
}

render(<Page />, document.getElementById("root")!);
