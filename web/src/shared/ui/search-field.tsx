// One search field (#1100, Doc "Form controls: one kit", item 4): the
// glass, the box you type in, and the `/` key, as Docs' list had it. A
// page's main search answers `/` and shows the key; Esc clears it, and
// on an empty field leaves it.

import type { RefObject, TargetedKeyboardEvent } from "preact";
import { useEffect, useRef } from "preact/hooks";
import { SearchIcon } from "./icons";
import "./search-field.css";

const inField = (t: EventTarget | null) => !!(t as Element | null)?.closest?.("input, textarea, select, [contenteditable]");
const covered = () => !!document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"]');

/** The fields that answer `/`, the newest last. One listener for them all:
 * the newest one on screen takes the key, so a page has one `/`. */
const slashers: RefObject<HTMLInputElement | null>[] = [];

const slash = (e: KeyboardEvent) => {
  if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented || inField(e.target) || covered()) return;
  for (let i = slashers.length - 1; i >= 0; i--) {
    const box = slashers[i].current;
    if (!box || box.getClientRects().length === 0) continue;
    e.preventDefault();
    box.focus();
    return;
  }
};

export function SearchField({
  value,
  onValue,
  label,
  placeholder = label,
  slash: answers = false,
  small = false,
  inputRef,
  class: cls,
  onKeyDown,
  ...input
}: {
  /** Its data- attributes, and the ARIA of a list it drives, go on the input. */
  [attr: `data-${string}` | `aria-${string}`]: string | boolean | undefined;

  value: string;
  onValue: (v: string) => void;
  /** What it searches, for a screen reader; the placeholder too unless one is given. */
  label: string;
  placeholder?: string;
  /** The page's main search: `/` from anywhere that isn't a field focuses it, and the key shows in the field. */
  slash?: boolean;
  /** A bar's height (shared/fields.css's --field-h-bar). */
  small?: boolean;
  inputRef?: RefObject<HTMLInputElement | null>;
  /** On the box, for its place in the layout. */
  class?: string;
  /** First: Esc is its unless this prevents the default. */
  onKeyDown?: (e: TargetedKeyboardEvent<HTMLInputElement>) => void;
}) {
  const own = useRef<HTMLInputElement>(null);
  const box = inputRef ?? own;
  useEffect(() => {
    if (!answers) return;
    slashers.push(box);
    if (slashers.length === 1) window.addEventListener("keydown", slash);
    return () => {
      slashers.splice(slashers.indexOf(box), 1);
      if (slashers.length === 0) window.removeEventListener("keydown", slash);
    };
  }, [answers]);
  return (
    <label class={["search-field", small && "small", cls].filter(Boolean).join(" ")}>
      <SearchIcon size="sm" />
      <input
        {...input}
        autoComplete="off"
        ref={box}
        type="search"
        placeholder={placeholder}
        aria-label={label}
        value={value}
        onInput={(e) => onValue(e.currentTarget.value)}
        onKeyDown={(e) => {
          onKeyDown?.(e);
          if (e.key !== "Escape" || e.defaultPrevented) return;
          e.preventDefault();
          if (value) onValue("");
          else e.currentTarget.blur();
        }}
      />
      {answers && <kbd aria-hidden="true">/</kbd>}
    </label>
  );
}
