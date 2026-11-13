// The fixture page for which room chat opens in (#965): the real chat page
// (RoomsView) over a fake control, with the platform page's own router,
// navigation and links (platformApi), so the history entries are the real
// ones. Four rooms; `?me=` is the account reading, `window.__fx` is the
// spec's hand. chat-last-cases.ts builds this with Vite and serves it at
// /p/prj_aaaa0002/…, so no daemon or control is involved.

import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import "../src/shared/style.css";
import "../src/shared/buttons.css";
import { chat } from "../src/shared/chat-store";
import { ChatApp } from "../src/shared/chat/app";
import { PlatformApiContext } from "../src/shared/platform-api";
import type { ChatMessage, ChatRoom } from "../src/shared/proto";
import { platformApi } from "../src/platform/api";
import { go, useRoute } from "../src/platform/router";
import { session } from "../src/platform/session";

// `?me=` is kept for the tab, so a reload (or a path without it) is the same account.
const params = new URLSearchParams(location.search);
if (params.has("me")) sessionStorage.setItem("fx.me", params.get("me")!);
chat.me = sessionStorage.getItem("fx.me") ?? "acct_a";
chat.on = true;

session.projects = [
  { id: "prj_aaaa0001", name: "Me", team: null },
  { id: "prj_aaaa0002", name: "Team", team: { id: "tm_1", name: "Team", role: "owner", members: [] } },
] as never;
session.state = "ready";

const room = (id: string, name: string, last: number, kind: "channel" | "dm" = "channel"): ChatRoom =>
  ({
    id,
    kind,
    team: "tm_1",
    name,
    machine: null,
    machine_name: null,
    target: null,
    project: null,
    with: null,
    others: [],
    private: false,
    last,
    read: last,
    unread: 0,
    mentions: 0,
    wants: 0,
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

// alpha is the newest, then beta, gamma and the old DM.
const rooms = [room("ch_aaaa0001", "alpha", 400), room("ch_aaaa0002", "beta", 300), room("ch_aaaa0003", "gamma", 200), room("ch_aaaa0004", "old", 100)];
chat.rooms = rooms;

const message = (seq: number): ChatMessage =>
  ({
    seq,
    at: Date.UTC(2026, 10, 1) + seq * 60_000,
    author: "acct_other",
    name: "Sam",
    text: `Message ${seq}`,
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
  }) as unknown as ChatMessage;

const json = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
const realFetch = window.fetch.bind(window);
window.fetch = async (input, init) => {
  const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
  if (!url.pathname.startsWith("/api/chat")) return realFetch(input, init);
  const m = /^\/api\/chat\/rooms\/([^/]+)(?:\/(messages|read))?$/.exec(url.pathname);
  const r = m && rooms.find((x) => x.id === m[1]);
  if (!r) return json({});
  if (m![2] === "messages") return json({ room: r, messages: Array.from({ length: 5 }, (_, i) => message(i + 1)), more: false });
  if (m![2] === "read") return json({});
  return json({ room: r, people: [], can_manage: false, agents: [] });
};

declare global {
  interface Window {
    __fx: {
      /** Messages arrive in a room: it's unread, and now the newest. */
      unread(id: string): void;
      /** Leave chat for another page of the project. */
      away(): void;
    };
  }
}

/** Messages arrive in a room: it's unread, and now the newest. */
function unread(id: string) {
  const r = rooms.find((x) => x.id === id)!;
  r.last = 999;
  r.unread = 2;
  // The store's `emit` is private: the page's own stream calls it.
  (chat as unknown as { emit(): void }).emit();
}

window.__fx = { unread, away: () => go("/p/prj_aaaa0002") };
// `?unread=` is unread before the page draws.
for (const id of params.get("unread")?.split(",") ?? []) if (id) unread(id);

const api = { ...platformApi, Bar: ({ children }: { children?: unknown }) => <div>{children as never}</div> };

function Page() {
  const r = useRoute();
  // The platform page draws chat once its session has loaded: after its
  // router is listening, which a chat drawn in the first pass would beat.
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);
  if (!ready) return null;
  if (r.page === "chat" || r.page === "dm") {
    const p = session.project(r.project);
    const scope = p ? { team: p.team?.id ?? null, name: p.name } : undefined;
    return (
      <div style={{ height: "100dvh", display: "flex", flexDirection: "column" }}>
        <ChatApp route={{ room: r.room, msg: r.msg, thread: r.thread, view: r.view, q: r.q, all: r.all }} scope={scope} />
      </div>
    );
  }
  return (
    <div data-away>
      <button data-go-chat onClick={() => platformApi.nav.openChat()}>
        Chat
      </button>
    </div>
  );
}

render(
  <PlatformApiContext.Provider value={api as never}>
    <Page />
  </PlatformApiContext.Provider>,
  document.getElementById("root")!,
);
