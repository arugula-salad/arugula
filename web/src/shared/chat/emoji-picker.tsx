// The emoji picker (#105): a search box, the categories and the ones used
// lately (per device, `recentEmoji`). The composer opens it beside its
// text; the reactions (#104) open the same one and take what `pick` hands
// them. It sits above its anchor on a desktop and fills the width of a phone
// from the bottom, where a thumb reaches. The table behind it loads when the
// picker opens (`./emoji`).

import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { findEmoji, recentEmoji, rememberEmoji, useEmoji } from "./emoji";
import { handled } from "./escape";

/** `pick`: the character chosen (already remembered as recent). `close`:
 * Escape, a click outside, or the same button again is the caller's too. */
export function EmojiPicker({ pick, close, above }: { pick: (char: string) => void; close: () => void; above?: boolean }) {
  const table = useEmoji(true);
  const [query, setQuery] = useState("");
  const [cat, setCat] = useState<number | "recent">(() => (recentEmoji().length ? "recent" : 0));
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  // Read once per opening: what's picked now shows up the next time.
  const recent = useMemo(recentEmoji, []);
  // On a phone the keyboard would cover the grid the moment the search box
  // took focus: leave it for the person to tap.
  useEffect(() => {
    if (matchMedia("(hover: hover)").matches) search.current?.focus();
  }, []);
  useEffect(() => {
    const away = (e: PointerEvent) => {
      const t = e.target as Element | null;
      // The button that opens it toggles it itself.
      if (!root.current?.contains(t) && !t?.closest("[data-emoji-button]")) close();
    };
    document.addEventListener("pointerdown", away);
    return () => document.removeEventListener("pointerdown", away);
  }, [close]);

  const shown: { char: string; name: string }[] = useMemo(() => {
    if (!table) return [];
    if (query.trim()) return findEmoji(table, query.trim(), 120).map((e) => ({ char: e.char, name: e.names[0] }));
    if (cat === "recent") {
      const by = new Map(table.list.map((e) => [e.char, e.names[0]]));
      return recent.map((char) => ({ char, name: by.get(char) ?? "" }));
    }
    return table.list.filter((e) => e.cat === cat).map((e) => ({ char: e.char, name: e.names[0] }));
  }, [table, query, cat, recent]);

  const choose = (char: string) => {
    rememberEmoji(char);
    pick(char);
  };

  return (
    <div
      class={above ? "emoji-picker above" : "emoji-picker"}
      ref={root}
      role="dialog"
      aria-label="Emoji"
      data-emoji-picker
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          handled(e);
          close();
        }
      }}
    >
      <input
        ref={search}
        type="search"
        class="emoji-search"
        placeholder="Search emoji"
        aria-label="Search emoji"
        value={query}
        onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
      />
      {!query.trim() && table && (
        <div class="emoji-cats" role="tablist">
          {recent.length > 0 && (
            <button type="button" role="tab" aria-selected={cat === "recent"} title="Recently used" aria-label="Recently used" onClick={() => setCat("recent")}>
              🕘
            </button>
          )}
          {table.cats.map((c, i) => (
            <button type="button" role="tab" key={c} aria-selected={cat === i} title={c} aria-label={c} onClick={() => setCat(i)}>
              {table.list.find((e) => e.cat === i)?.char}
            </button>
          ))}
        </div>
      )}
      <div class="emoji-grid" role="listbox" aria-label={query.trim() ? "Matches" : cat === "recent" ? "Recently used" : table?.cats[cat] ?? "Emoji"}>
        {!table && <p class="emoji-note">Loading…</p>}
        {table && !shown.length && <p class="emoji-note">{query.trim() ? "No emoji by that name." : "Nothing used yet."}</p>}
        {shown.map((e) => (
          <button type="button" role="option" key={e.char} title={e.name && `:${e.name}:`} aria-label={e.name || e.char} data-emoji={e.char} onClick={() => choose(e.char)}>
            {e.char}
          </button>
        ))}
      </div>
    </div>
  );
}
