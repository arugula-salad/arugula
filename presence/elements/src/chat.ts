// A room's text messages drawn into an element (<arugula-chat>, and
// connect()'s chat): the list, newest at the bottom and kept there while
// you're at the bottom, "earlier" to page back, and an input that sends
// with Enter (Shift+Enter for a new line). Plain DOM, no framework.

import type { Message } from "@arugula/presence";
import type { Room } from "./room.ts";

export interface ChatOptions {
  /** The input's placeholder. */
  placeholder?: string;
}

function time(at: number): string {
  return new Date(at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

export function mountChat(room: Room, el: Element | ShadowRoot, o: ChatOptions = {}): () => void {
  const box = document.createElement("div");
  box.className = "arugula-chat";
  box.setAttribute("part", "chat");
  const list = document.createElement("div");
  list.className = "messages";
  list.setAttribute("role", "log");
  list.setAttribute("aria-live", "polite");
  const earlier = document.createElement("button");
  earlier.type = "button";
  earlier.className = "earlier";
  earlier.textContent = "Earlier messages";
  earlier.addEventListener("click", () => void room.loadEarlier());
  const form = document.createElement("form");
  form.className = "say";
  const input = document.createElement("textarea");
  input.rows = 1;
  input.placeholder = o.placeholder ?? "Say something";
  input.setAttribute("aria-label", "Message");
  const err = document.createElement("span");
  err.className = "error";
  form.append(input, err);
  box.append(list, form);
  el.append(box);

  const send = async () => {
    const text = input.value.trim();
    if (!text) return;
    try {
      await room.say(text);
      input.value = "";
      err.textContent = "";
    } catch (e) {
      err.textContent = (e as Error).message;
    }
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  });
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    void send();
  });

  const shown = new Map<number, HTMLElement>();
  const row = (m: Message) => {
    const r = document.createElement("div");
    r.className = m.sub === room.me ? "msg mine" : "msg";
    const head = document.createElement("span");
    head.className = "who";
    head.textContent = m.sub === room.me ? "You" : m.name;
    const at = document.createElement("time");
    at.textContent = time(m.at);
    head.append(" ", at);
    const body = document.createElement("span");
    body.className = "text";
    body.textContent = m.text;
    r.append(head, body);
    return r;
  };
  const render = () => {
    const atBottom = list.scrollHeight - list.scrollTop - list.clientHeight < 24;
    const msgs = room.messages;
    const keep = new Set(msgs.map((m) => m.id));
    for (const [id, r] of shown) if (!keep.has(id)) (r.remove(), shown.delete(id));
    let before: Element | null = null;
    for (let i = msgs.length - 1; i >= 0; i--) {
      const m = msgs[i];
      let r = shown.get(m.id);
      if (!r) {
        r = row(m);
        shown.set(m.id, r);
      }
      if (r.nextElementSibling !== before || r.parentElement !== list) list.insertBefore(r, before);
      before = r;
    }
    if (room.client.hasEarlier && msgs.length) list.prepend(earlier);
    else earlier.remove();
    if (atBottom) list.scrollTop = list.scrollHeight;
  };
  const unsub = room.client.subscribe(render);
  render();
  list.scrollTop = list.scrollHeight;
  return () => {
    unsub();
    box.remove();
  };
}
