// Emoji reactions (#104), the plain parts: the quick set, a chip's tooltip,
// and a message with a reaction added or taken back before the server has
// answered. Plain functions, for ./reactions-ui.tsx, the store, and their tests
// (scripts/chat-reactions.test.mjs).

import type { ChatMessage, ChatReaction, ChatReactor } from "../proto";

/** What the message's tools and a long-press offer first. */
export const QUICK = ["👍", "❤️", "😂", "🎉", "👀", "✅"];

/** One who reacted, as the tooltip names them: "You", "Sam", or "Jake's
 * agent on geek" for an agent reacting for its owner. */
export function reactor(r: ChatReactor, me: string): string {
  const name = r.name || "someone";
  if (r.agent) return `${r.account === me ? "Your" : `${name}'s`} agent on ${r.agent.machine_name || "a machine that left"}`;
  return r.account === me ? "You" : name;
}

/** "You, Sam and 2 more reacted with 👍": yours first, the rest as they came. */
export function tip(c: ChatReaction, me: string): string {
  const who = [...c.who].sort((a, b) => Number(b.account === me && !b.agent) - Number(a.account === me && !a.agent));
  const names = who.map((w) => reactor(w, me));
  const more = c.count - names.length;
  if (more > 0) names.push(`${more} more`);
  const list = names.length < 2 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
  return `${list} reacted with ${c.emoji}`;
}

/** `m` with `emoji` added or taken back for the reader (`me`, their account;
 * `by`, the agent reacting for them, if it is one), as the server will have
 * it: the page shows this at once and the server's answer replaces it. */
export function reacted(m: ChatMessage, emoji: string, on: boolean, me: string, by?: string): ChatMessage {
  const had = m.reactions ?? [];
  const mine = (w: ChatReactor) => w.account === me && (w.agent?.machine ?? "") === (by ?? "");
  const out: ChatReaction[] = [];
  let found = false;
  for (const c of had) {
    if (c.emoji !== emoji) {
      out.push(c);
      continue;
    }
    found = true;
    if (on === c.mine) {
      out.push(c);
    } else if (on) {
      out.push({ ...c, count: c.count + 1, mine: true, who: [...c.who, { account: me, name: "", agent: null }] });
    } else if (c.count > 1) {
      out.push({ ...c, count: c.count - 1, mine: false, who: c.who.filter((w) => !mine(w)) });
    }
  }
  if (on && !found) out.push({ emoji, count: 1, mine: true, who: [{ account: me, name: "", agent: null }] });
  return { ...m, reactions: out };
}
