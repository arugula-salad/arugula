// A channel's topic and pinned messages (#119), the parts that show and
// change them: the header's topic line (and its About card), the 📌 count
// that opens the pins, and the dim centered line a topic change, a pin
// or a session's PR (M259) leaves in the room. Plain parts are in ./pins.

import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { chat, type ChatStore } from "../chat-store";
import type { ChatMessage, ChatRoom } from "../proto";
import { askText } from "../ui/menu";
import { usePlatformApi } from "../platform-api";
import { authorLabel } from "./agents";
import { handled } from "./escape";
import { mayPin, preview, systemText } from "./pins";
import { Time } from "../ui/time";

/** Ask for a channel's new topic. */
export async function editTopic(store: ChatStore, room: ChatRoom, toast: (s: string) => void) {
  const v = await askText(`Topic for #${room.name}`, room.topic, "What is this channel for?");
  if (v === null || v.trim() === room.topic) return;
  store.setTopic(room.id, { topic: v.trim() }).catch((e: Error) => toast(e.message));
}

/** Ask for a channel's new description. */
export async function editDescription(store: ChatStore, room: ChatRoom, toast: (s: string) => void) {
  const v = await askText(`Description of #${room.name}`, room.description, "A longer note on what belongs here");
  if (v === null || v.trim() === room.description) return;
  store.setTopic(room.id, { description: v.trim() }).catch((e: Error) => toast(e.message));
}

/** A button and the card it opens under it, closed by Escape or a press
 * outside. On a phone the card spans the screen's width. */
function Popover({
  label,
  title,
  button,
  children,
  hook,
  onOpen,
}: {
  label: string;
  title: string;
  button: ComponentChildren;
  children: (close: () => void) => ComponentChildren;
  hook: string;
  onOpen?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => !root.current?.contains(e.target as Node) && setOpen(false);
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".prompt")) return;
      handled(e);
      setOpen(false);
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <span class="chat-pop-wrap" ref={root}>
      <button
        class={open ? "chat-info on" : "chat-info"}
        title={title}
        aria-label={label}
        aria-expanded={open}
        {...{ [hook]: "" }}
        onClick={() => {
          if (!open) onOpen?.();
          setOpen(!open);
        }}
      >
        {button}
      </button>
      {open && (
        <div class="chat-pop" role="dialog" aria-label={label} {...{ [`${hook}-card`]: "" }}>
          {children(() => setOpen(false))}
        </div>
      )}
    </span>
  );
}

/** The header's second line for a channel with a topic: the topic itself,
 * which a member who may change it clicks to rewrite. Where nobody wrote
 * one, `fallback` (the computed line), and a link to add one. */
export function TopicLine({ room, fallback, toast }: { room: ChatRoom; fallback: string; toast: (s: string) => void }) {
  const edit = () => void editTopic(chat, room, toast);
  const sets = room.kind === "channel" && room.topic_writable;
  if (room.kind === "channel" && room.topic) {
    return sets ? (
      <button class="chat-topic" title={`${room.topic}\n\nChange the topic`} data-chat-topic data-topic-set onClick={edit}>
        {room.topic}
      </button>
    ) : (
      <span title={room.topic} data-chat-topic data-topic-set>
        {room.topic}
      </span>
    );
  }
  return (
    <span data-chat-topic>
      {fallback}
      {sets && (
        <>
          {" · "}
          <button class="chat-topic add" data-topic-add onClick={edit}>
            Add a topic
          </button>
        </>
      )}
    </span>
  );
}

/** The About card: a channel's topic and description, with the buttons to
 * change them for whoever may. */
export function AboutButton({ room, toast }: { room: ChatRoom; toast: (s: string) => void }) {
  if (room.kind !== "channel" || (!room.description && !room.topic_writable)) return null;
  return (
    <Popover label="About this channel" title="About this channel" hook="data-chat-about" button="ⓘ">
      {(close) => (
        <>
          <strong>#{room.name}</strong>
          <p class={room.topic ? "" : "dim"}>{room.topic || "No topic yet."}</p>
          {room.description && <p class="chat-pop-text">{room.description}</p>}
          {room.topic_writable && (
            <div class="chat-pop-buttons">
              <button
                data-about-topic
                onClick={() => {
                  close();
                  void editTopic(chat, room, toast);
                }}
              >
                {room.topic ? "Change topic" : "Set topic"}
              </button>
              <button
                data-about-description
                onClick={() => {
                  close();
                  void editDescription(chat, room, toast);
                }}
              >
                {room.description ? "Change description" : "Add description"}
              </button>
            </div>
          )}
        </>
      )}
    </Popover>
  );
}

/** The 📌 and its count, when the room has pins: opens them, newest first,
 * each with a jump to the message and (to who may write here) an unpin. */
export function PinsButton({ room, toast }: { room: ChatRoom; toast: (s: string) => void }) {
  const nav = usePlatformApi().nav;
  const pins = chat.pinsOf(room.id);
  if (!room.pins && !pins?.length) return null;
  return (
    <Popover
      label={`Pinned messages: ${room.pins}`}
      title="Pinned messages"
      hook="data-chat-pins"
      onOpen={() => void chat.loadPins(room.id)}
      button={
        <>
          <span aria-hidden="true">📌</span> <span data-pins-count>{room.pins}</span>
        </>
      }
    >
      {(close) => (
        <>
          <strong>Pinned</strong>
          {!pins && <p class="dim">Loading…</p>}
          {pins?.length === 0 && <p class="dim">Nothing is pinned.</p>}
          <ul class="chat-pins">
            {pins?.map((p) => (
              <li key={p.message.seq} data-pin={p.message.seq}>
                <div class="chat-pin-who">
                  {authorLabel(p.message)} · <Time at={p.message.at} />
                </div>
                <div class="chat-pin-text">{preview(p.message.text) || (p.message.quote ? "A quote from a pane" : "")}</div>
                <div class="chat-pop-buttons">
                  <button
                    data-pin-jump
                    onClick={() => {
                      close();
                      nav.openRoom(room.id, p.message.seq);
                    }}
                  >
                    Jump to it
                  </button>
                  {room.writable && (
                    <button data-pin-remove onClick={() => chat.pin(room.id, p.message.seq, false).catch((e: Error) => toast(e.message))}>
                      Unpin
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
    </Popover>
  );
}

/** What happened in the room, in place of a message: centered, dim. */
export function SystemLine({ room, m }: { room: ChatRoom; m: ChatMessage }) {
  const api = usePlatformApi();
  const nav = api.nav;
  if (m.deleted !== null) return null;
  // A session's PR (M259): its thread says which session it came from.
  if (m.system?.kind === "session_pr" && m.text.startsWith("ses_")) {
    const short = m.text.slice(4, 12);
    const href = room.project ? api.sessionHref?.(room.project, m.text) : undefined;
    return (
      <div class="msg-system" data-msg={m.seq} data-system="session_pr">
        <b>{authorLabel(m)}</b> opened this PR from agent session{" "}
        {href ? (
          <a href={href} data-system-session={m.text}>
            {short}
          </a>
        ) : (
          <code>{short}</code>
        )}
        <time title={new Date(m.at).toLocaleString()}>{new Date(m.at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</time>
      </div>
    );
  }
  const { before, target, after } = systemText(m);
  return (
    <div class="msg-system" data-msg={m.seq} data-system={m.system?.kind}>
      <b>{authorLabel(m)}</b> {before}
      {target !== null && (
        <>
          {" "}
          <a
            href={nav.roomLink(room.id, target)}
            data-system-jump={target}
            onClick={(e) => {
              e.preventDefault();
              nav.openRoom(room.id, target);
            }}
          >
            a message
          </a>
        </>
      )}
      {after}
      <time title={new Date(m.at).toLocaleString()}>{new Date(m.at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</time>
    </div>
  );
}

/** The message menu's pin or unpin. */
export function PinTool({ room, m, setError }: { room: ChatRoom; m: ChatMessage; setError: (e: string | null) => void }) {
  if (!mayPin(room, m)) return null;
  return (
    <button
      title={m.pinned ? "Unpin" : "Pin to the room"}
      aria-label={m.pinned ? "Unpin message" : "Pin message"}
      aria-pressed={m.pinned}
      data-msg-pin
      onClick={() => chat.pin(room.id, m.seq, !m.pinned).catch((e: Error) => setError(e.message))}
    >
      📌
    </button>
  );
}

/** A pinned message says so, under its head. */
export function PinnedMark({ m }: { m: ChatMessage }) {
  return m.pinned ? (
    <span class="thread-tag pinned-mark" title="Pinned to the room" data-pinned-mark>
      📌 pinned
    </span>
  ) : null;
}
