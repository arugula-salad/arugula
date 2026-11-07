// Emoji reactions (#104), the parts of a message that show and make them:
// the chips under it, the menu the toolbar and a long-press open (the quick
// set, then the picker for the rest), and the long-press itself. The
// quick set, tooltips and the optimistic update are in ./reactions.ts.

import { useEffect, useRef, useState } from "preact/hooks";
import { chat } from "../chat-store";
import type { ChatMessage, ChatRoom } from "../proto";
import { escapeTaken, pressedShut } from "../ui/menu";
import { EmojiPicker } from "./emoji-picker";
import { handled } from "./escape";
import { MachineIcon, ReactIcon } from "../ui/icons";
import { peopleOf, QUICK, tip } from "./reactions";

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
      {reactions.map((c) => {
        const agents = c.agents ?? 0;
        const people = peopleOf(c);
        return (
          <button
            type="button"
            key={c.emoji}
            class={c.mine ? "reaction mine" : "reaction"}
            title={tip(c, chat.me)}
            aria-label={`${people ? `${people} ${people === 1 ? "person" : "people"}` : ""}${people && agents ? " and " : ""}${agents ? `${agents} ${agents === 1 ? "agent" : "agents"}` : ""} reacted with ${c.emoji}${c.mine ? ", including you" : ""}`}
            aria-pressed={c.mine}
            disabled={!may}
            data-reaction={c.emoji}
            onClick={() => toggle(room, m, c.emoji, setError)}
          >
            <span class="reaction-emoji">{c.emoji}</span>
            {people > 0 && <span class="reaction-count">{people}</span>}
            {agents > 0 && (
              <>
                {people > 0 && <span class="reaction-sep" aria-hidden="true" />}
                <span class="reaction-agents" data-reaction-agents={agents}>
                  <MachineIcon size={12} />
                  {agents}
                </span>
              </>
            )}
          </button>
        );
      })}
      {may && (
        <button type="button" class="reaction add" title="Add a reaction" aria-label="Add a reaction" data-react-button onClick={open}>
          <ReactIcon />
        </button>
      )}
    </div>
  );
}

/** Closes a popover on Escape and on a press outside it, but not on `keep`:
 * the buttons that toggle it themselves, and the picker, which closes itself
 * (and takes its own Escape). */
export function useDismiss(root: { current: HTMLElement | null }, close: () => void, keep: string) {
  useEffect(() => {
    const away = (e: PointerEvent) => {
      const t = e.target as Element | null;
      if (!root.current?.contains(t) && !t?.closest(`${keep}, [data-emoji-picker]`)) close();
    };
    const key = (e: KeyboardEvent) => {
      // The picker's Escape shuts only the picker.
      if (e.key !== "Escape" || escapeTaken(e)) return;
      handled(e);
      close();
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", key);
    };
  }, [close]);
}

/** The quick set and "more…" for the picker, as buttons: each adds the
 * emoji (or takes mine back) and calls `close`. They sit in the toolbar's
 * menu, and at the top of the long-press menu (`ReactRow`). */
function ReactButtons({ room, m, close, setError, onMore }: { room: ChatRoom; m: ChatMessage; close: () => void; setError: (e: string | null) => void; onMore?: () => void }) {
  const [more, setMore] = useState(false);
  const moreButton = useRef<HTMLButtonElement>(null);
  const choose = (emoji: string) => {
    toggle(room, m, emoji, setError);
    close();
  };
  return (
    <>
      {QUICK.map((e) => {
        const mine = !!m.reactions?.some((c) => c.emoji === e && c.mine);
        return (
          <button type="button" role="menuitem" key={e} class={mine ? "mine" : ""} aria-pressed={mine} aria-label={`React with ${e}`} data-quick={e} onClick={() => choose(e)}>
            {e}
          </button>
        );
      })}
      <button
        type="button"
        role="menuitem"
        class="more"
        aria-label="More emoji"
        aria-expanded={more}
        data-react-more
        ref={moreButton}
        onClick={() => (onMore ? onMore() : pressedShut(moreButton.current) || setMore((v) => !v))}
      >
        …
      </button>
      {more && <EmojiPicker from={moreButton} pick={choose} close={() => setMore(false)} />}
    </>
  );
}

/** The quick set under the toolbar. Closes after one is chosen, on Escape
 * and on a press outside. */
export function ReactMenu({ room, m, close, setError }: { room: ChatRoom; m: ChatMessage; close: () => void; setError: (e: string | null) => void }) {
  const root = useRef<HTMLDivElement>(null);
  useDismiss(root, close, "[data-react-button]");
  return (
    <div class="react-menu" ref={root} role="menu" aria-label="React" data-react-menu>
      <ReactButtons room={room} m={m} close={close} setError={setError} />
    </div>
  );
}

/** The picker for a message's reactions, from the long-press menu's "more…":
 * that menu has shut by now, so it hangs from the message. */
export function ReactPicker({ room, m, from, close, setError }: { room: ChatRoom; m: ChatMessage; from: { current: HTMLElement | null }; close: () => void; setError: (e: string | null) => void }) {
  return (
    <EmojiPicker
      from={from}
      pick={(emoji) => {
        toggle(room, m, emoji, setError);
        close();
      }}
      close={close}
    />
  );
}

/** The same row, at the top of the message menu a long-press opens (its
 * `head`): the menu's items follow it. */
export function ReactRow({ room, m, close, setError, more }: { room: ChatRoom; m: ChatMessage; close: () => void; setError: (e: string | null) => void; more: () => void }) {
  return (
    <div class="react-menu react-row" role="group" aria-label="React" data-react-menu>
      <ReactButtons room={room} m={m} close={close} setError={setError} onMore={more} />
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
