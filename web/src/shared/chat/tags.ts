// Tags in the agents' rooms (Arugula #1522): `@channel`, `@here`, `@cos`
// and `@<board>`, resolved by id rather than by text. Who a post reached is
// its mention statuses' (./mention-status, Arugula #1560). No page state,
// so `scripts/chat-tags.test.mjs` runs it without a browser.
//
// The wire here is #1522's (its server half is Arugula #1536): a post
// sends `tags` beside its text, and a message comes back with its `tags`.
// Without them, its tags are matched by today's names.

import type { ChatMessage, ChatPost } from "../proto";

/** One tag in a post, as written (`text`, without the `@`) and as meant. A
 * board's is its id, so a rename keeps it. */
export type ChatTag = { kind: "channel" | "here" | "cos"; text: string } | { kind: "board"; board: string; text: string };

export type TaggedMessage = ChatMessage & { tags?: ChatTag[] };
export type TaggedPost = ChatPost & { tags?: ChatTag[] };

/** A board as a tag names it. */
export interface TagBoard {
  id: string;
  title: string;
  lead: boolean;
}

export type RoomTags = "agents" | "board";

/** What the `@` list offers in an agents' room, `typed` being what follows
 * the `@` so far: @channel, @here, @cos, then a board's lead each. */
export interface TagOption {
  tag: ChatTag;
  note: string;
}

export function tagOptions(room: RoomTags, boards: TagBoard[], typed: string): TagOption[] {
  const t = typed.toLowerCase();
  const all: TagOption[] = [
    { tag: { kind: "channel", text: "channel" }, note: room === "agents" ? "every agent here" : "the lead and every worker" },
    { tag: { kind: "here", text: "here" }, note: room === "agents" ? "the active ones" : "its active workers" },
    { tag: { kind: "cos", text: "cos" }, note: "your chief of staff" },
    ...boards.map((b) => ({ tag: { kind: "board" as const, board: b.id, text: b.title }, note: b.lead ? "its lead" : "no lead yet" })),
  ];
  return all.filter((o) => o.tag.text.toLowerCase().startsWith(t) || o.tag.text.toLowerCase().split(/\s+/).some((w) => w.startsWith(t)));
}

/** The tags a text has, from what the `@` list offered: each `@name` at a
 * word's start and end, the longest name first ("@A2P" isn't "@A"). */
export function tagsIn(text: string, options: TagOption[]): ChatTag[] {
  const out: ChatTag[] = [];
  const names = [...options].sort((a, b) => b.tag.text.length - a.tag.text.length);
  const lower = text.toLowerCase();
  const taken: [number, number][] = [];
  for (const o of names) {
    const want = `@${o.tag.text.toLowerCase()}`;
    for (let i = lower.indexOf(want); i >= 0; i = lower.indexOf(want, i + 1)) {
      const end = i + want.length;
      if (i > 0 && /\w/.test(text[i - 1]!)) continue;
      if (end < text.length && /\w/.test(text[end]!)) continue;
      if (taken.some(([a, b]) => i < b && end > a)) continue;
      taken.push([i, end]);
      if (!out.some((t) => same(t, o.tag))) out.push(o.tag);
    }
  }
  return out;
}

const same = (a: ChatTag, b: ChatTag) => a.kind === b.kind && (a.kind !== "board" || (b.kind === "board" && a.board === b.board));

/** A tag at the start of `from` (which starts with `@`): the message's own
 * tags first (a board's drawn with its name now), else today's names.
 * `len` is how much of `from` it is. */
export function tagAt(from: string, tags: ChatTag[], title: (board: string) => string | undefined): { len: number; kind: ChatTag["kind"]; name: string } | null {
  const lower = from.toLowerCase();
  const fits = (text: string) => lower.startsWith(`@${text.toLowerCase()}`) && !/\w/.test(from[text.length + 1] ?? "");
  const found = [...tags].sort((a, b) => b.text.length - a.text.length).find((t) => fits(t.text));
  if (!found) return null;
  return { len: found.text.length + 1, kind: found.kind, name: found.kind === "board" ? (title(found.board) ?? found.text) : found.text };
}

/** The line under the composer: who a post here reaches. */
export function composerLine(room: RoomTags, reply: boolean): string {
  if (reply) return "Replies reach whoever the post reached";
  return room === "agents" ? "Every lead gets this, no tag needed" : "Its lead gets this · @here adds active workers";
}

/** Under the `@` list: the delivery rule, in #agents. */
export const TAG_RULE = "In #agents, a post from you or the chief of staff reaches every lead. A lead's post reaches only the leads it tags.";
