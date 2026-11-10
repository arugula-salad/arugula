// The emoji picker (#105): a search box, the categories and the ones used
// lately (per device, `recentEmoji`). The composer opens it beside its
// text; the reactions (#104) open the same one and take what `pick` hands
// them. It is a `Popover` (shared/ui/menu.tsx): under its button, or over it
// when there's no room, and a sheet along the bottom of a phone, where a
// thumb reaches. The table behind it loads when the picker opens (`./emoji`).

import { useMemo, useState } from "preact/hooks";
import { Popover } from "../ui/menu";
import { findEmoji, recentEmoji, rememberEmoji, useEmoji } from "./emoji";

/** `from`: the button that opens it. `pick`: the character chosen (already
 * remembered as recent). `close`: it shut by Escape or a press elsewhere; the
 * same button again is the caller's too (`pressedShut`). */
export function EmojiPicker({ from, pick, close }: { from: { current: HTMLElement | null }; pick: (char: string) => void; close: () => void }) {
  return (
    <Popover from={from} label="Emoji" attrs={{ "data-emoji-picker": true }} onClose={close}>
      <EmojiPanel pick={pick} />
    </Popover>
  );
}

function EmojiPanel({ pick }: { pick: (char: string) => void }) {
  const table = useEmoji(true);
  const [query, setQuery] = useState("");
  const [cat, setCat] = useState<number | "recent">(() => (recentEmoji().length ? "recent" : 0));
  // Read once per opening: what's picked now shows up the next time.
  const recent = useMemo(recentEmoji, []);
  // On a phone the keyboard would cover the grid the moment the search box
  // took focus: leave it for the person to tap.
  const hover = matchMedia("(hover: hover)").matches;

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
    <div class="emoji-picker">
      <input
        type="search"
        data-autofocus={hover || undefined}
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
