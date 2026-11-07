// What a message offers (#121): the ⋯ menu under its hover toolbar, and the
// same items in the long-press menu on a phone (`openMenu`: a sheet there). Some of them belong to
// features that are built elsewhere (threads #106, saved items #116, mark
// unread, pins #119): each is a hook here, `messageHooks`, that the feature
// sets when it lands. Until then its item is not shown, so nothing in the
// menu does nothing.

import type { ComponentChildren } from "preact";
import type { ChatMessage, ChatRoom } from "../proto";
import { type MenuItem, menuBelow, openMenu } from "../ui/menu";
import { ClockIcon, EditIcon, LinkIcon, PinIcon, QuoteIcon, TrashIcon, UnreadIcon } from "../ui/icons";

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

/** The ⋯ menu's items for a message, in order. `edit` and `remove` are given
 * only to those who may, below a rule; `quote` only where the room takes a
 * message. `before` are items the long-press sheet puts first (the toolbar
 * has those as buttons). Each keeps the test selector it has always had. */
export function itemsOf(
  c: MessageCtx,
  o: { quote?: () => void; copy: () => void; edit?: () => void; remove?: () => void },
): MenuItem[] {
  const out: MenuItem[] = [];
  if (o.quote) out.push({ label: "Quote in reply", icon: <QuoteIcon />, run: o.quote, attrs: { "data-msg-reply": "" } });
  out.push({ label: "Copy link", icon: <LinkIcon size={16} />, run: o.copy, done: "Copied", attrs: { "data-msg-link": "" } });
  const unread = hookFor("unread", c);
  if (unread) out.push({ label: unread.label?.(c) ?? "Mark unread", icon: <UnreadIcon />, run: () => unread.run(c), attrs: { "data-msg-unread": "" } });
  const hour = hookFor("remindHour", c);
  if (hour) out.push({ label: "Remind me in 1 hour", icon: <ClockIcon />, run: () => hour.run(c), attrs: { "data-msg-remind-hour": "" } });
  const tomorrow = hookFor("remindTomorrow", c);
  if (tomorrow) out.push({ label: "Remind me tomorrow", icon: <ClockIcon />, run: () => tomorrow.run(c), attrs: { "data-msg-remind-tomorrow": "" } });
  const pin = hookFor("pin", c);
  if (pin) {
    const on = pin.on?.(c);
    out.push({ label: pin.label?.(c) ?? "Pin to the room", icon: <PinIcon />, run: () => pin.run(c), attrs: { "data-msg-pin": "", ...(on === undefined ? {} : { "aria-pressed": on }) } });
  }
  if (o.edit || o.remove) out.push("separator");
  if (o.edit) out.push({ label: "Edit message", icon: <EditIcon />, run: o.edit, attrs: { "data-msg-edit": "" } });
  if (o.remove) out.push({ label: "Delete message", icon: <TrashIcon />, run: o.remove, danger: true, attrs: { "data-msg-delete": "" } });
  return out;
}

/** Hang the ⋯ menu under its button, or (no button: a long-press) at the
 * message's corner, with the emoji row above the items if given. Drawn in
 * the message itself (`MenuHost`). */
export function openMsgMenu(items: MenuItem[], at: HTMLElement, o: { head?: ComponentChildren; button?: boolean; onClose: () => void }) {
  const r = at.getBoundingClientRect();
  const e = o.button ? menuBelow(at) : { clientX: r.right - 16, clientY: r.top + 16, preventDefault() {} };
  return openMenu(e, items, { from: o.button ? at : null, end: true, label: "More actions", attrs: { "data-msg-menu": "" }, head: o.head, onClose: o.onClose });
}
