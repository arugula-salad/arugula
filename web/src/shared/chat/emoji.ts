// Emoji for chat (#105): the shortcode table, loaded on first use, and what
// the composer and the renderer ask of it. The table is gemoji's (MIT) in
// `emoji-data.ts`, its own chunk: a room whose messages hold no `:code:` and
// whose composer never types `:` or opens the picker never fetches it.
// Nothing is fetched from anywhere else.

import { useEffect, useState } from "preact/hooks";

export interface Emoji {
  char: string;
  /** Shortcodes, the first the usual one. */
  names: string[];
  /** The words that find it besides its names. */
  words: string;
  /** Index into `cats`. */
  cat: number;
}

export interface EmojiTable {
  cats: string[];
  list: Emoji[];
  byName: Map<string, string>;
}

let ready: EmojiTable | null = null;
let loading: Promise<EmojiTable> | null = null;
const waiting = new Set<() => void>();

export function loadEmoji(): Promise<EmojiTable> {
  loading ??= import("./emoji-data").then((m) => {
    const cats: string[] = [];
    const list: Emoji[] = [];
    const byName = new Map<string, string>();
    for (const line of m.default.split("\n")) {
      if (line.startsWith("# ")) {
        cats.push(line.slice(2));
        continue;
      }
      const [char, names = "", ...words] = line.split(" ");
      if (!char || !cats.length) continue;
      const e = { char, names: names.split(","), words: words.join(" "), cat: cats.length - 1 };
      list.push(e);
      for (const n of e.names) byName.set(n, char);
    }
    ready = { cats, list, byName };
    for (const f of waiting) f();
    return ready;
  });
  // A failed load (offline) can be tried again the next time it's wanted.
  loading.catch(() => {
    loading = null;
  });
  return loading;
}

/** The table once it's loaded; asking for it (`want`) starts the load. */
export function useEmoji(want: boolean): EmojiTable | null {
  const [, again] = useState(0);
  useEffect(() => {
    if (ready || !want) return;
    const f = () => again((n) => n + 1);
    waiting.add(f);
    loadEmoji().catch(() => {});
    return () => void waiting.delete(f);
  }, [want]);
  return ready;
}

/** Whether a text might hold a shortcode: worth loading the table for. */
export const SHORTCODE = /:[a-z0-9_+-]+:/i;
const SHORTCODES = /:([a-z0-9_+-]+):/gi;

/** `text` with the shortcodes the table knows turned into emoji; the others
 * stay as typed. */
export function withEmoji(text: string, t: EmojiTable): string {
  return text.replace(SHORTCODES, (s, name: string) => t.byName.get(name.toLowerCase()) ?? s);
}

// Up to three emoji and nothing else: a keycap or a digit is not one.
const PICTO = String.raw`(?:\p{Extended_Pictographic}(?:️|\p{Emoji_Modifier}|‍\p{Extended_Pictographic})*|\p{Regional_Indicator}{2})`;
const ONLY = new RegExp(`^(?:${PICTO}){1,3}$`, "u");

/** Whether a message is only emoji (up to three, shortcodes counted), which
 * draw large. Without the table a shortcode isn't known yet: not yet. */
export function onlyEmoji(text: string, t: EmojiTable | null): boolean {
  let s = text.trim();
  if (!s || s.length > 120) return false;
  if (t) s = withEmoji(s, t);
  return ONLY.test(s.replace(/\s+/g, ""));
}

/** Emoji matching what's typed after a `:` or in the picker's search:
 * names that start with it first, then names and words that contain it. */
export function findEmoji(t: EmojiTable, query: string, max: number): Emoji[] {
  const q = query.toLowerCase().replace(/^:/, "");
  if (!q) return [];
  const start: Emoji[] = [];
  const within: Emoji[] = [];
  for (const e of t.list) {
    if (e.names.some((n) => n.startsWith(q))) start.push(e);
    else if (e.names.some((n) => n.includes(q)) || e.words.split(" ").some((w) => w.startsWith(q))) within.push(e);
  }
  // As Slack does: what you use, then what most people use, then the table's
  // order (`:ta` is 🎉 before 🍊).
  const recent = recentEmoji();
  const rank = (e: Emoji) => {
    const r = recent.indexOf(e.char);
    if (r >= 0) return r;
    const c = COMMON.indexOf(e.char);
    return c >= 0 ? RECENT_MAX + c : Infinity;
  };
  const byRank = (a: Emoji, b: Emoji) => rank(a) - rank(b);
  return [...start.sort(byRank), ...within.sort(byRank)].slice(0, max);
}

/** What most people react and write with, first among equal matches. */
const COMMON = ["👍", "❤️", "😂", "🎉", "👀", "✅", "🙏", "🔥", "😄", "😅", "🤔", "👋", "🚀", "💯", "😢", "😍", "✨", "🙌", "👏", "😭", "🥲", "😎", "🤝", "⚠️", "❌"];

// ---- recently used, per device

const RECENT_KEY = "chat.emoji.recent";
const RECENT_MAX = 24;

export function recentEmoji(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]") as unknown;
    return Array.isArray(v) ? v.filter((c): c is string => typeof c === "string").slice(0, RECENT_MAX) : [];
  } catch {
    return [];
  }
}

/** Puts `char` first among the recent ones. */
export function rememberEmoji(char: string) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify([char, ...recentEmoji().filter((c) => c !== char)].slice(0, RECENT_MAX)));
  } catch {
    // kept for this page only: the picker simply has no memory here
  }
}
