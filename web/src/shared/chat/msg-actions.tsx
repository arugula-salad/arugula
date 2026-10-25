// What a message offers (#121): the ⋯ menu under its hover toolbar, and the
// same items in the long-press sheet on a phone. Some of them belong to
// features that are built elsewhere (threads #106, saved items #116, mark
// unread, pins #119): each is a hook here, `messageHooks`, that the feature
// sets when it lands. Until then its item is not shown, so nothing in the
// menu does nothing.

import { useRef } from "preact/hooks";
import type { ChatMessage, ChatRoom } from "../proto";
import { useDismiss } from "./reactions-ui";

/** What a hook is given. */
export interface MessageCtx {
  room: ChatRoom;
  m: ChatMessage;
}

/** A feature's hold on a message's tools. */
export interface MessageHook {
  run(c: MessageCtx): void;
  /** The item's words when they depend on the message ("Remove from saved"). */
  label?(c: MessageCtx): string;
  /** Whether it is on for this message (saved, pinned): shown pressed. */
  on?(c: MessageCtx): boolean;
  /** Whether it applies to this message at all (a pin needs a room's right). */
  when?(c: MessageCtx): boolean;
}

/** Set by the feature that owns the action: `thread` and `save` sit on the
 * toolbar itself, `unread`, the reminders and `pin` in the ⋯ menu. */
export const messageHooks: {
  thread?: MessageHook;
  save?: MessageHook;
  unread?: MessageHook;
  remindHour?: MessageHook;
  remindTomorrow?: MessageHook;
  pin?: MessageHook;
} = {};

/** The hook for `id`, if it is set and applies here. */
export function hookFor(id: keyof typeof messageHooks, c: MessageCtx): MessageHook | undefined {
  const h = messageHooks[id];
  return h && (!h.when || h.when(c)) ? h : undefined;
}

/** One entry of the ⋯ menu and the sheet. */
export interface MsgItem {
  id: string;
  label: string;
  icon: string;
  run(): void;
  danger?: boolean;
  on?: boolean;
  /** The test selector this item has always had (`data-msg-link`). */
  data: string;
}

/** The ⋯ menu's items for a message, in order. `edit` and `remove` are given
 * only to those who may; `quote` only where the room takes a message. */
export function itemsOf(
  c: MessageCtx,
  o: { quote?: () => void; copy: () => void; copied: boolean; edit?: () => void; remove?: () => void },
): MsgItem[] {
  const out: MsgItem[] = [];
  if (o.quote) out.push({ id: "quote", label: "Quote in reply", icon: "❝", run: o.quote, data: "data-msg-reply" });
  out.push({ id: "link", label: o.copied ? "Copied" : "Copy link", icon: o.copied ? "✓" : "🔗", run: o.copy, data: "data-msg-link" });
  const unread = hookFor("unread", c);
  if (unread) out.push({ id: "unread", label: unread.label?.(c) ?? "Mark unread", icon: "◐", run: () => unread.run(c), data: "data-msg-unread" });
  const hour = hookFor("remindHour", c);
  if (hour) out.push({ id: "remind-hour", label: "Remind me in 1 hour", icon: "⏰", run: () => hour.run(c), data: "data-msg-remind-hour" });
  const tomorrow = hookFor("remindTomorrow", c);
  if (tomorrow) out.push({ id: "remind-tomorrow", label: "Remind me tomorrow", icon: "⏰", run: () => tomorrow.run(c), data: "data-msg-remind-tomorrow" });
  const pin = hookFor("pin", c);
  if (pin) out.push({ id: "pin", label: pin.label?.(c) ?? "Pin to the room", icon: "📌", run: () => pin.run(c), on: pin.on?.(c), data: "data-msg-pin" });
  if (o.edit) out.push({ id: "edit", label: "Edit message", icon: "✎", run: o.edit, data: "data-msg-edit" });
  if (o.remove) out.push({ id: "delete", label: "Delete message", icon: "🗑", run: o.remove, danger: true, data: "data-msg-delete" });
  return out;
}

/** The items as a list of buttons; `done` runs after any is chosen. */
export function MsgItems({ items, done }: { items: MsgItem[]; done: () => void }) {
  return (
    <>
      {items.map((it) => (
        <button
          type="button"
          role="menuitem"
          key={it.id}
          class={`msg-item${it.danger ? " danger" : ""}`}
          aria-pressed={it.on}
          {...{ [it.data]: "" }}
          onClick={() => {
            // "Copy link" shows its tick in place: the menu stays for a beat.
            if (it.id !== "link") done();
            it.run();
            if (it.id === "link") setTimeout(done, 700);
          }}
        >
          <span class="msg-item-icon" aria-hidden="true">
            {it.icon}
          </span>
          {it.label}
        </button>
      ))}
    </>
  );
}

/** The ⋯ menu, under the toolbar. Closes on a choice, Escape or a press
 * outside. */
export function MsgMenu({ items, close }: { items: MsgItem[]; close: () => void }) {
  const root = useRef<HTMLDivElement>(null);
  useDismiss(root, close, "[data-msg-more]");
  return (
    <div class="msg-menu" ref={root} role="menu" aria-label="More actions" data-msg-menu>
      <MsgItems items={items} done={close} />
    </div>
  );
}
