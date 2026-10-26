// Team chat as a block (#123): the platform page's chat (../../shared/chat)
// in a slot of the layout, split, tabbed, zoomed and on a phone the whole
// screen, like any block. The daemon keeps nothing of it but where it is
// (crates/daemon/src/team_chat.rs, `{room?, pinned?, view?, q?}`); the
// messages are Arugula control's, read as the person looking, through the
// signed-in chat store the thread drawer uses.
//
// What this host gives the shared pieces (`PlatformApiContext`):
//  - `nav`: the route lives here, in the block, and is saved to its layout
//    entry (a debounced `route` call) so a reload and a daemon restart
//    reopen where it was, instead of changing an address;
//  - the quote: one action, from the pane beside it (its selection, else
//    its last lines), rather than the picker's two steps;
//  - a pane chip opens the pane in this layout.
//
// A block pinned to a room (`pinned`) is that room alone, with no sidebar.
// A slot narrower than a sidebar and a thread draws the phone's layout
// (`fit.narrow`). A block in a tab that isn't shown reads nothing and has
// no room open to control (`ChatShown`), and its keys answer only while
// focus is in it (`fit.scoped`), so two blocks neither double-count nor
// fight over Ctrl+K.

import { render } from "preact";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { paneIds, type Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { chat, type PaneQuote } from "../../shared/chat-store";
import { ChatApp } from "../../shared/chat/app";
import { roomLabel } from "../../shared/chat/room";
import type { ChatRoute } from "../../shared/chat/route";
import { ChatShown } from "../../shared/chat/shown";
import { PlatformApiContext, type PlatformApi } from "../../shared/platform-api";
import { askText } from "../../shared/ui/menu";
import { useSubscribe } from "../../shared/ui/hooks";
import { QuotePicker, shellPlatformApi } from "../platform-api";
import { setRoomAsBlock } from "../ui/threads";
import { useControlState } from "../ui/control-state";
import { getControlSession } from "../ui/people";
import { openGettingStarted } from "../ui/welcome";
import { registerBlock, type BlockView } from "./view";

/** The block's state: its layout entry, which the daemon says whole. */
export interface TeamChatState {
  room?: string;
  pinned?: boolean;
  view?: "activity" | "search";
  q?: string;
}

/** Narrower than this a sidebar and a thread don't both fit. */
const NARROW_PX = 560;
/** How long to wait on a navigation before keeping it: a run of them is
 * one write. */
const SAVE_MS = 400;
/** The lines of a pane quoted when none are selected. */
const QUOTE_LINES = 20;

const routeOf = (s: TeamChatState | null): ChatRoute => (s ? { room: s.room, view: s.view, q: s.q } : {});

// ---- opening

/** The mounted blocks, to send a room to: of every machine this page has
 * shown, so each says whose it is. */
const mounted = new Set<{ client: Client; id: PaneId; go: (r: ChatRoute) => void }>();

/** Team chat in a block: beside `from`, or in a new tab of `session`. With
 * no room (or no pin) it shows the one already open in the layout, if
 * there is one, rather than a second sidebar. */
export async function openTeamChat(client: Client, o: { session?: number; from?: PaneId; room?: string; msg?: number; pinned?: boolean } = {}) {
  if (!o.pinned) {
    const have = [...mounted].find((m) => m.client === client && client.tabOfPane(m.id) && !(client.blocks.get(m.id)?.state as TeamChatState | null)?.pinned);
    if (have) {
      client.setActive(have.id);
      if (o.room) have.go({ room: o.room, msg: o.msg });
      return;
    }
  }
  const place = o.from !== undefined ? { split: o.from, from_pane: o.from } : { session: o.session?.toString() };
  await client.openBlock({ type: "team_chat", config: { room: o.room ?? null, pinned: !!o.pinned }, local: true, ...place }, "couldn't open chat");
}

/** "Pin a chat room…": a block for one room of yours, beside `from`. */
export async function pinChatRoom(client: Client, o: { from?: PaneId; session?: number }) {
  const store = chat.on === true ? chat : null;
  // Not signed in to chat: the block says why, and offers to join.
  if (!store) return openTeamChat(client, o);
  const api = shellPlatformApi(client);
  const rooms = store.rooms.filter((r) => r.kind !== "archive").map((r) => ({ value: r.id, label: roomLabel(r, api) }));
  const v = await askText("Pin a chat room", "", "room, like #ops", Promise.resolve(rooms));
  const typed = v?.trim().toLowerCase().replace(/^#/, "");
  if (!typed) return;
  const room = rooms.find((r) => r.value === v) ?? rooms.find((r) => r.label.toLowerCase().replace(/^#/, "") === typed);
  if (!room) return client.toast(`No room of yours is called ${v!.trim()}`);
  await openTeamChat(client, { ...o, room: room.value, pinned: true });
}

// The thread drawer's "Open in a chat block".
setRoomAsBlock((client, room) => void openTeamChat(client, { room, session: client.session ?? undefined }));

// ---- the host

/** The shell's api with the block's `nav`, quote and pane chips. */
function hostApi(client: Client, id: PaneId, go: (r: ChatRoute) => void, pin: () => string | undefined): PlatformApi {
  const base = shellPlatformApi(client);
  return {
    ...base,
    nav: {
      ...base.nav,
      openChat: () => go({}),
      // The ✕ of the phone's list: this block is the whole chat page here.
      closeChat: () => client.intent({ op: "close_pane", pane: id }),
      // A pinned block stays on its room: another is the platform page's.
      // A thread opens beside the room here too (UI audit #156: it was dropped).
      openRoom: (room, msg, thread) => (pin() && room !== pin() ? base.nav.openRoom(room, msg, thread) : go({ room, msg, thread })),
      openChatView: (view, q) => (pin() ? base.nav.openChatView(view, q) : go({ view, q })),
    },
    // The panes of this layout open in it; others as the shell does.
    goToPane(machine, pane, quote) {
      const n = Number(pane);
      if (client.e2e?.daemon.id === machine && client.info(n)) {
        client.setActive(n);
        if (quote) requestAnimationFrame(() => client.panes.get(n)?.view.reveal(quote));
        return;
      }
      base.goToPane(machine, pane, quote);
    },
    Bar: ({ children }) => <div class="chat-bar chat-block-bar">{children}</div>,
    quote: { can: () => true, Picker: (p) => <NeighbourQuote client={client} block={id} {...p} /> },
  };
}

/** The pane beside the block that a quote comes from: the active one of
 * its tab if it's a terminal, else the first. */
function neighbour(client: Client, block: PaneId): PaneId | undefined {
  const tab = client.tabOfPane(block);
  if (!tab) return undefined;
  const terminals = paneIds(tab).filter((p) => p !== block && client.info(p)?.type === "terminal");
  const active = client.active(tab.id);
  return active !== undefined && terminals.includes(active) ? active : terminals[0];
}

/** Quoting is one action: the neighbouring pane's selection, else its last
 * lines, is taken at once into the message (where it shows, and can be
 * dropped). With no pane beside it, or nothing to read, the shell's picker
 * of every pane of yours is the fallback. */
function NeighbourQuote({ client, block, close, take }: { client: Client; block: PaneId; close: () => void; take: (q: PaneQuote) => void }) {
  const [fallback, setFallback] = useState(false);
  useEffect(() => {
    let live = true;
    const machine = client.e2e?.daemon.id;
    const pane = neighbour(client, block);
    if (!machine || pane === undefined) return setFallback(true);
    (async () => {
      let text = client.panes.get(pane)?.view.selection().trim() ?? "";
      if (!text) {
        const res = await client.request("GET", `/api/panes/${pane}/capture?format=text`);
        const lines = ((res.ok && res.text ? await res.text() : "") || "").split("\n").map((l) => l.trimEnd());
        while (lines.length && !lines[lines.length - 1]) lines.pop();
        text = lines.slice(-QUOTE_LINES).join("\n");
      }
      if (!live) return;
      if (!text.trim()) return setFallback(true);
      take({ machine, pane, text });
    })().catch(() => live && setFallback(true));
    return () => void (live = false);
  }, []);
  return fallback ? <QuotePicker close={close} take={take} /> : null;
}

/** Whether the slot is too narrow for a sidebar beside a thread. */
function useNarrow(box: { current: HTMLElement | null }): boolean {
  const [narrow, setNarrow] = useState(false);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const fit = () => setNarrow(el.clientWidth > 0 && el.clientWidth < NARROW_PX);
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return narrow;
}

function TeamChatBlock({ client, id, s, shown }: { client: Client; id: PaneId; s: TeamChatState | null; shown: boolean }) {
  const session = getControlSession();
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  useSubscribe((fn) => chat.subscribe(fn));
  const owned = useControlState(client);
  const box = useRef<HTMLDivElement>(null);
  const narrow = useNarrow(box);
  // Where this view is. The layout entry says where it started; moving is
  // this view's (a block on another screen, or someone else's, keeps its own).
  const [moved, setMoved] = useState<ChatRoute | null>(null);
  const pinned = s?.pinned && s.room ? s.room : undefined;
  const route = pinned ? { room: pinned, msg: moved?.room === pinned ? moved.msg : undefined } : (moved ?? routeOf(s));
  const save = useRef<number | undefined>(undefined);
  const go = useMemo(
    () => (r: ChatRoute) => {
      setMoved(r);
      clearTimeout(save.current);
      save.current = window.setTimeout(() => {
        void client.request("POST", `/api/blocks/${id}/call/route`, { room: r.room ?? null, view: r.view ?? null, q: r.q ?? null }).catch(() => {});
      }, SAVE_MS);
    },
    [client, id],
  );
  useEffect(() => {
    const entry = { client, id, go };
    mounted.add(entry);
    return () => {
      mounted.delete(entry);
      clearTimeout(save.current);
    };
  }, [client, id, go]);
  const pin = useRef(pinned);
  pin.current = pinned;
  const api = useMemo(() => hostApi(client, id, go, () => pin.current), [client, id, go]);

  const signedIn = session?.phase === "ready";
  let body;
  if (!s) body = <p class="chat-none">…</p>;
  else if (chat.on === true || (signedIn && chat.on === false)) {
    body = (
      <PlatformApiContext.Provider value={api}>
        <ChatShown.Provider value={shown}>
          <ChatApp route={route} fit={{ narrow, pinned: !!pinned, scoped: true }} />
        </ChatShown.Provider>
      </PlatformApiContext.Provider>
    );
  } else if (signedIn) body = <p class="chat-none" data-chat-reading>Reading chat from Arugula control…</p>;
  else body = <NeedsControl client={client} joined={owned?.state === "joined"} guest={!!client.state?.roles} />;
  return (
    <div class="chat-block" ref={box} data-chat-block={id} data-chat-pinned-to={pinned} data-chat-narrow={narrow ? "" : undefined}>
      {body}
    </div>
  );
}

/** This page can't chat as you: it isn't signed in to Arugula control. */
function NeedsControl({ client, joined, guest }: { client: Client; joined: boolean; guest: boolean }) {
  return (
    <div class="chat-block-needs" data-chat-needs-control>
      <h3>Team chat needs Arugula control</h3>
      {guest ? (
        <p>Chat is your team's, on Arugula control, and you read it as yourself. Open this machine through Control, signed in, to chat here.</p>
      ) : joined ? (
        <p>This machine has joined Arugula control, but this page isn't signed in to it. Open the machine through Control and chat is here as you.</p>
      ) : (
        <>
          <p>Channels, direct messages and threads are held by Arugula control, and you read them as yourself. This machine hasn't joined one.</p>
          <button class="primary" data-chat-join onClick={() => openGettingStarted("cloud", client)}>
            Join Arugula control…
          </button>
        </>
      )}
    </div>
  );
}

// ---- the tab

/** What a tab's chat blocks say is unread: each room once. */
export function teamChatUnread(client: Client, panes: PaneId[]): { n: number; mention: boolean } {
  let all: { n: number; mention: boolean } | null = null;
  const rooms = new Set<string>();
  let n = 0;
  let mention = false;
  for (const p of panes) {
    if (client.info(p)?.type !== "team_chat") continue;
    const s = client.blocks.get(p)?.state as TeamChatState | null | undefined;
    if (s?.pinned && s.room) {
      const r = chat.room(s.room);
      if (r && !rooms.has(r.id)) {
        rooms.add(r.id);
        n += r.unread;
        mention ||= !!r.mentions;
      }
    } else all ??= chat.unread();
  }
  // A block of the whole of chat counts every room, the pinned ones' too.
  return all ?? { n, mention };
}

/** A tab's unread and mention badge for its chat blocks. */
export function ChatTabBadge({ client, panes }: { client: Client; panes: PaneId[] }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const { n, mention } = teamChatUnread(client, panes);
  if (!n) return null;
  return (
    <span class={mention ? "chat-count mention" : "chat-count"} data-chat-tab-badge title={mention ? `${n} unread in chat, you're mentioned` : `${n} unread in chat`}>
      {mention ? `@${n}` : n}
    </span>
  );
}

// ---- the view

registerBlock("team_chat", (client: Client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-team-chat";
  let state: TeamChatState | null = null;
  let visible = true;
  const draw = () => render(<TeamChatBlock client={client} id={id} s={state} shown={visible} />, host);
  draw();
  const pinnedTo = () => (state?.pinned && state.room ? state.room : undefined);
  return {
    host,
    setVisible(v) {
      host.style.display = v ? "" : "none";
      if (v !== visible) {
        visible = v;
        draw();
      }
    },
    update(s) {
      state = s as TeamChatState;
      draw();
    },
    title() {
      const room = pinnedTo() ? chat.room(pinnedTo()!) : undefined;
      return room ? roomLabel(room, shellPlatformApi(client)) : "Chat";
    },
    text: () => host.textContent ?? "",
    focus: () => host.querySelector<HTMLElement>("textarea, input, button")?.focus(),
    dispose() {
      render(null, host);
      host.remove();
    },
  };
});
