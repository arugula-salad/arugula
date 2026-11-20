// The Overview's talk panel (Arugula #1270): beside the field on a
// computer, Needs you on its top, then a strip of conversation tabs and the
// open one, which holds its own box to write in. The field opens a tab
// (a board's thread from its row, a session's peek from its row) and never
// goes anywhere; × closes one.
// A tab's kind says what it shows (`TAB_KINDS`): a board's thread, or a
// session's peek (./overview-peek.tsx, #1271); the chief of staff's room
// (#1275) adds its own. The chief of staff's tab is pinned first, with no
// ×, once it has a room; until then its slot is empty (`pinned` null). The
// tabs a reader opened are kept for the browser tab, per project, as the
// field's open boards are.

import type { ComponentChildren, JSX } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { ChatRoom } from "../shared/proto";
import { boards } from "../shared/boards/store";
import { chat } from "../shared/chat-store";
import { RoomBody } from "../shared/chat/room";
import { EmptyLine } from "../shared/ui/block-states";
import { useSubscribe } from "../shared/ui/hooks";
import { CloseIcon, IconButton } from "../shared/ui/icons";
import { PeekTab } from "./overview-peek";

/** A conversation in the panel: what kind it is, what it's on (a board's
 * id for a thread), and what its tab says. */
export interface TalkTab {
  kind: TalkKind;
  id: string;
  title: string;
}

/** What a kind of tab draws and how it says it has something new. */
interface TabKind {
  Body: (p: { tab: TalkTab; phone: boolean }) => JSX.Element;
  /** Something in it the reader hasn't seen: the tab's dot. */
  unread(tab: TalkTab): boolean;
}

/** The kinds of tab. */
export const TAB_KINDS = {
  /** A board's thread: the room on the board, as chat draws it. */
  thread: {
    Body: ({ tab, phone }) => <ThreadTab board={tab.id} title={tab.title} phone={phone} />,
    unread: (tab) => (objectRoom(tab.id)?.unread ?? 0) > 0,
  },
  /** The reader's staff room with their own agents (#1275), the chief of
   * staff's: pinned first, by its room's id. */
  staff: {
    Body: ({ tab, phone }) => <StaffTab room={tab.id} title={tab.title} phone={phone} />,
    unread: (tab) => (chat.rooms.find((r) => r.id === tab.id)?.unread ?? 0) > 0,
  },
  /** A session's peek (#1271): its last lines, live while open, and a box to write to it. */
  peek: {
    Body: ({ tab }) => <PeekTab id={tab.id} />,
    unread: () => false,
  },
} satisfies Record<string, TabKind>;
export type TalkKind = keyof typeof TAB_KINDS;

/** The most tabs besides the pinned one; opening another drops the oldest. */
export const TABS_MAX = 5;

const keyOf = (t: TalkTab) => `${t.kind}:${t.id}`;
const objectRoom = (target: string) => chat.rooms.find((r) => r.kind === "object" && r.target === target);

export interface Talk {
  tabs: TalkTab[];
  /** The open tab's key, if any. */
  active: string | null;
  /** Open a tab (or bring back the one already open on the same thing). */
  open(t: TalkTab): void;
  close(key: string): void;
  pick(key: string): void;
}

/** The panel's tabs on this project's Overview, kept for the browser tab. */
export function useTalk(project: string): Talk {
  const store = `arugula.ov-talk:${project}`;
  const [s, setS] = useState<{ tabs: TalkTab[]; active: string | null }>(() => {
    try {
      const v = JSON.parse(sessionStorage.getItem(store) ?? "null") as { tabs: TalkTab[]; active: string | null } | null;
      // A kind this page no longer has is dropped.
      if (v) return { tabs: v.tabs.filter((t) => t.kind in TAB_KINDS), active: v.active };
    } catch {
      // Unreadable: start empty.
    }
    return { tabs: [], active: null };
  });
  const put = (change: (was: typeof s) => typeof s) =>
    setS((was) => {
      const next = change(was);
      try {
        sessionStorage.setItem(store, JSON.stringify(next));
      } catch {
        // Not kept: it holds for this page.
      }
      return next;
    });
  return {
    ...s,
    open: (t) =>
      put(({ tabs }) => {
        const k = keyOf(t);
        if (tabs.some((x) => keyOf(x) === k)) return { tabs: tabs.map((x) => (keyOf(x) === k ? t : x)), active: k };
        return { tabs: [...tabs, t].slice(-TABS_MAX), active: k };
      }),
    close: (k) =>
      put(({ tabs, active }) => {
        const i = tabs.findIndex((x) => keyOf(x) === k);
        const left = tabs.filter((x) => keyOf(x) !== k);
        // Closing the open tab opens its neighbour.
        const next = active !== k ? active : left.length ? keyOf(left[Math.min(i, left.length - 1)]) : null;
        return { tabs: left, active: next };
      }),
    pick: (k) => put((was) => ({ ...was, active: k })),
  };
}

/** The panel: `needs` (Needs you) on top, then the tabs, `pinned` first. */
export function TalkPanel({ talk, pinned, needs, phone }: { talk: Talk; pinned: TalkTab | null; needs?: ComponentChildren; phone: boolean }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const tabs = pinned ? [pinned, ...talk.tabs.filter((t) => keyOf(t) !== keyOf(pinned))] : talk.tabs;
  const open = tabs.find((t) => keyOf(t) === talk.active) ?? (pinned && !talk.active ? pinned : undefined);
  const Body = open && TAB_KINDS[open.kind].Body;
  const fit = useFitWindow(!!open);
  return (
    <aside ref={fit} class={`ov-panel${open ? " ov-panel-talking" : ""}`} data-ov-panel>
      {needs}
      <section class="ov-talk" aria-label="Talk" data-ov-talk>
        {tabs.length > 0 && (
          <div class="ov-tabs" role="tablist" data-ov-tabs>
            {tabs.map((t) => {
              const k = keyOf(t);
              const on = open !== undefined && keyOf(open) === k;
              const dot = !on && TAB_KINDS[t.kind].unread(t);
              return (
                <div key={k} class={`ov-tab${on ? " ov-tab-on" : ""}`} data-ov-tab={k} data-ov-tab-unread={dot ? "" : undefined}>
                  <button type="button" role="tab" aria-selected={on} aria-controls={on ? "ov-talk-open" : undefined} title={t.title} onClick={() => talk.pick(k)}>
                    {dot && <i class="ov-tab-dot" aria-label="Unread" />}
                    <span>{t.title}</span>
                  </button>
                  {t !== pinned && (
                    <IconButton class="ov-tab-close" label={`Close ${t.title}`} data-ov-tab-close onClick={() => talk.close(k)}>
                      <CloseIcon size="xs" />
                    </IconButton>
                  )}
                </div>
              );
            })}
          </div>
        )}
        {open && Body ? (
          <div class="ov-talk-open" id="ov-talk-open" role="tabpanel" data-ov-talk-open={keyOf(open)}>
            <Body key={keyOf(open)} tab={open} phone={phone} />
          </div>
        ) : (
          !phone && <EmptyLine data-ov-talk-none>A board's thread, or a session from its row, opens here.</EmptyLine>
        )}
      </section>
    </aside>
  );
}

/** While `on`, a panel held beside the field (sticky) reaches from where
 * its top is to the window's foot, as the page scrolls, so the box to write
 * in is always in sight; docked under the field, it keeps its CSS height. */
function useFitWindow(on: boolean) {
  const ref = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (!on) return void el.style.removeProperty("height");
    const size = () => {
      if (getComputedStyle(el).position !== "sticky") return void el.style.removeProperty("height");
      el.style.height = `${Math.max(320, Math.round(window.innerHeight - Math.max(0, el.getBoundingClientRect().top) - 12))}px`;
    };
    size();
    // The page scrolls in the frame's main, not the window: listen in the capture phase.
    document.addEventListener("scroll", size, { capture: true, passive: true });
    window.addEventListener("resize", size);
    return () => {
      document.removeEventListener("scroll", size, { capture: true });
      window.removeEventListener("resize", size);
    };
  }, [on]);
  return ref;
}

/** The reader's staff room in `project` (#1275), once it exists: the chief
 * of staff's session (or the reader) makes it, and its tab is pinned first. */
export function staffTab(project: string): TalkTab | null {
  const r = chat.rooms.find((x) => x.staff && x.project === project);
  return r ? { kind: "staff", id: r.id, title: "Chief of staff" } : null;
}

/** The staff room: its messages, which its lead session hears, and the box. */
function StaffTab({ room, title, phone }: { room: string; title: string; phone: boolean }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const r = chat.rooms.find((x) => x.id === room);
  if (!r) return <p class="dim ov-talk-note">Loading the room…</p>;
  const why = r.lead
    ? `What you write here goes to ${r.lead.session} on ${r.lead.machine_name || "its machine"}. Only you and your agents read it.`
    : "No session leads this room yet, so what you write waits here. Only you and your agents read it.";
  return (
    <div class="chat-thread ov-talk-room" data-ov-staff={r.id}>
      <RoomBody key={r.id} room={r} phone={phone} embedded name={title} empty={{ title: "Nothing said to your chief of staff yet.", why }} />
    </div>
  );
}

/** A board's thread: its room, its messages and the box to write in. */
function ThreadTab({ board, title, phone }: { board: string; title: string; phone: boolean }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const [room, setRoom] = useState<ChatRoom | null>(null);
  const [error, setError] = useState<string | null>(null);
  const on = chat.on;
  useEffect(() => {
    if (!on) return;
    let live = true;
    boards.thread(board).then(
      (r) => live && setRoom(r),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [board, on]);
  if (on === false) return <p class="dim ov-talk-note">Threads come with chat, which is off for you. A team owner turns it on for the team under Teams.</p>;
  if (!room) return <p class="dim ov-talk-note">{error ? `The thread didn't load: ${error}` : "Loading the thread…"}</p>;
  return (
    <div class="chat-thread ov-talk-room" data-ov-thread={room.id}>
      <RoomBody key={room.id} room={room} phone={phone} embedded name={title} empty={{ title: "Nothing said in this board's thread yet.", why: "Everyone in the project reads it. Write @name to notify someone." }} />
    </div>
  );
}
