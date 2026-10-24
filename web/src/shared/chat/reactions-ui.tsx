// Emoji reactions (#104), the parts of a message that show and make them:
// the chips under it, the menu the toolbar and a long-press open (the quick
// set, then the picker for the rest), and the long-press itself. The
// quick set, tooltips and the optimistic update are in ./reactions.ts.

import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { chat } from "../chat-store";
import type { ChatMessage, ChatRoom } from "../proto";
import { EmojiPicker } from "./emoji-picker";
import { QUICK, tip } from "./reactions";

/** Whether `room` takes reactions on `m` (an archive's messages and a
 * read-only room's do not). */
export function mayReact(room: ChatRoom, m: ChatMessage): boolean {
  return room.writable && !m.imported && m.deleted === null;
}

/** Add `emoji` to a message, or take mine back if it's there. */
function toggle(room: ChatRoom, m: ChatMessage, emoji: string, setError: (e: string | null) => void) {
  const on = !m.reactions?.some((c) => c.emoji === emoji && c.mine);
  chat.react(room.id, m.seq, emoji, on).catch((e: Error) => setError(e.message));
}

/** The chips under a message: emoji and count, mine highlighted, the
 * tooltip saying who. Clicking one adds mine or takes it back; the last
 * button opens the menu for another. */
export function ReactionChips({
  room,
  m,
  open,
  setError,
}: {
  room: ChatRoom;
  m: ChatMessage;
  open: () => void;
  setError: (e: string | null) => void;
}) {
  const reactions = m.reactions ?? [];
  if (!reactions.length) return null;
  const may = mayReact(room, m);
  return (
    <div class="reactions" role="group" aria-label="Reactions">
      {reactions.map((c) => (
        <button
          type="button"
          key={c.emoji}
          class={c.mine ? "reaction mine" : "reaction"}
          title={tip(c, chat.me)}
          aria-label={`${c.count} reacted with ${c.emoji}${c.mine ? ", including you" : ""}`}
          aria-pressed={c.mine}
          disabled={!may}
          data-reaction={c.emoji}
          onClick={() => toggle(room, m, c.emoji, setError)}
        >
          <span class="reaction-emoji">{c.emoji}</span>
          <span class="reaction-count">{c.count}</span>
        </button>
      ))}
      {may && (
        <button type="button" class="reaction add" title="Add a reaction" aria-label="Add a reaction" data-react-button onClick={open}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
            <circle cx="8" cy="8" r="6.2" />
            <path d="M5.4 9.6c.6 1.1 1.5 1.6 2.6 1.6s2-.5 2.6-1.6" stroke-linecap="round" />
            <path d="M5.9 6.2v.6M10.1 6.2v.6" stroke-linecap="round" stroke-width="1.8" />
          </svg>
        </button>
      )}
    </div>
  );
}

/** Closes a popover on Escape and on a press outside it, but not on `keep`:
 * the buttons that toggle it themselves, and the picker, which closes itself. */
export function useDismiss(root: { current: HTMLElement | null }, close: () => void, keep: string) {
  useEffect(() => {
    const away = (e: PointerEvent) => {
      const t = e.target as Element | null;
      if (!root.current?.contains(t) && !t?.closest(`${keep}, [data-emoji-picker]`)) close();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && close();
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", key);
    };
  }, [close]);
}

/** The quick set under the toolbar (a sheet along the bottom on a phone),
 * and "more…" for the picker. Closes after one is chosen, on Escape and on
 * a press outside. As the long-press sheet it carries the message's other
 * items (`children`) under the emoji, and without `react` only those. */
export function ReactMenu({
  room,
  m,
  close,
  setError,
  react = true,
  children,
}: {
  room: ChatRoom;
  m: ChatMessage;
  close: () => void;
  setError: (e: string | null) => void;
  react?: boolean;
  children?: ComponentChildren;
}) {
  const [more, setMore] = useState(false);
  // The picker opens below the menu unless the room under it is short (the
  // last message: it would grow the list's scroll area) and above has more.
  const [above, setAbove] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const menu = root.current;
    const picker = menu?.querySelector<HTMLElement>(".emoji-picker");
    if (!menu || !picker || matchMedia("(max-width: 600px)").matches) return;
    const m = menu.getBoundingClientRect();
    const list = menu.closest(".thread-list")?.getBoundingClientRect();
    const h = picker.offsetHeight + 4;
    const below = Math.min(list?.bottom ?? innerHeight, innerHeight) - m.bottom;
    const over = m.top - Math.max(list?.top ?? 0, 0);
    setAbove(below < h && over > below);
  }, [more]);
  useDismiss(root, close, "[data-react-button]");
  const choose = (emoji: string) => {
    toggle(room, m, emoji, setError);
    close();
  };
  return (
    <div class={children ? "react-menu with-items" : "react-menu"} ref={root} role="menu" aria-label={children ? "Message" : "React"} data-react-menu>
      {react &&
        QUICK.map((e) => {
          const mine = !!m.reactions?.some((c) => c.emoji === e && c.mine);
          return (
            <button type="button" role="menuitem" key={e} class={mine ? "mine" : ""} aria-pressed={mine} aria-label={`React with ${e}`} data-quick={e} onClick={() => choose(e)}>
              {e}
            </button>
          );
        })}
      {react && (
        <button type="button" role="menuitem" class="more" aria-label="More emoji" aria-expanded={more} data-react-more onClick={() => setMore((v) => !v)}>
          …
        </button>
      )}
      {more && <EmojiPicker pick={choose} close={close} above={above} />}
      {children && <div class="react-menu-items">{children}</div>}
    </div>
  );
}

/** How long a finger rests before it is a long-press, and how far it may
 * drift (scrolling) before it isn't. */
const HOLD_MS = 450;
const DRIFT_PX = 10;

/** Handlers that call `open` when a touch rests on the element: the menu a
 * phone has no hover toolbar for. Android's own long-press menu is the
 * `contextmenu` event, which opens it too; iOS has neither, hence the timer. */
export function longPress(open: () => void) {
  let timer = 0;
  let from = { x: 0, y: 0 };
  const stop = () => {
    window.clearTimeout(timer);
    timer = 0;
  };
  return {
    onPointerDown(e: PointerEvent) {
      if (e.pointerType !== "touch" || (e.target as Element).closest("a, button, textarea, input, pre")) return;
      from = { x: e.clientX, y: e.clientY };
      stop();
      timer = window.setTimeout(() => {
        timer = 0;
        open();
      }, HOLD_MS);
    },
    onPointerMove(e: PointerEvent) {
      if (timer && Math.hypot(e.clientX - from.x, e.clientY - from.y) > DRIFT_PX) stop();
    },
    onPointerUp: stop,
    onPointerCancel: stop,
    onContextMenu(e: MouseEvent) {
      if (!matchMedia("(pointer: coarse)").matches || (e.target as Element).closest("a, textarea, input")) return;
      e.preventDefault();
      stop();
      open();
    },
  };
}
