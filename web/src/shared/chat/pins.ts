// A channel's topic and pinned messages (#119), the plain parts: what a
// system line says, a pin's one-line preview, and whether the reader may
// write the topic. ./pins-ui draws them.

import type { ChatMessage, ChatRoom } from "../proto";

/** What a system line says after its author's name: "set the topic: …",
 * "pinned a message". `target` is the pinned message's number, if any. */
export function systemText(m: ChatMessage): { before: string; target: number | null; after: string } {
  const s = m.system;
  const none = { target: null, after: "" };
  switch (s?.kind) {
    case "topic":
      return m.text ? { before: `set the topic: ${m.text}`, ...none } : { before: "cleared the topic", ...none };
    case "description":
      return { before: "changed the description", ...none };
    case "pin":
      return { before: "pinned", target: s.target, after: "" };
    case "unpin":
      return { before: "unpinned", target: s.target, after: "" };
    case "session_pr":
      return { before: "opened this PR from an agent session", ...none };
    default:
      return { before: s?.kind ?? "", ...none };
  }
}

/** A pinned message's text on one line, cut at `max` characters. */
export function preview(text: string, max = 140): string {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > max ? `${one.slice(0, max - 1).trimEnd()}…` : one;
}

/** Whether `m` can be pinned from its menu: said, not deleted, not an
 * archive's, in a room the reader writes in. */
export function mayPin(room: ChatRoom, m: ChatMessage): boolean {
  return room.writable && !m.imported && !m.system && m.deleted === null;
}
