// The page's keys, in one table (M107, #1022; every key since Lift 3,
// #1479): each binding's id, its chords per OS, when it applies and what it
// does. One capture-phase listener on the window reads the table, so a
// chord is the page's before the terminal sees it; a key no binding takes
// goes on to xterm untouched. The footer's chips, the `?` sheet, menus and
// tooltips read their hints from here (`keyHint`), so a key is written once.
//
// Chords are written `Mod+Mod+Key`: the modifiers are Ctrl, Meta (⌘ on a
// Mac), Shift and Alt, matched exactly; `*+Key` takes any modifiers. Key is
// an `event.code` (`KeyP`, `Digit1`, `Escape`), so the keyboard's layout
// doesn't matter, or `key:x` for what the key types (`event.key`, any
// case). A plain `key:?` (a symbol) takes whatever Shift it needs. Two
// chords with a space between are a sequence: `key:g key:o` is g, then o.
//
// A plain key (no Ctrl, Meta or Alt) is never taken from a field, an
// editable or a terminal: the table checks that once, here, so a screen's
// `j` never eats the j someone types.
//
// One key runs one binding (#1535): the most specific that takes it, by its
// `context` (a dialog's, then a screen's, then the agent panel's, then one
// that's everywhere), and of two in one context, the newer. So the board's
// `l` writes to the lead without the panel's `l` running too.

import { useLayoutEffect } from "preact/hooks";

export type Os = "mac" | "other";

/** Where a binding is listed on the `?` sheet. */
export type Group = "go" | "switch" | "needs" | "screen" | "panes" | "app";

export const GROUPS: { id: Group; label: string }[] = [
  { id: "go", label: "Go to" },
  { id: "switch", label: "Switch" },
  { id: "needs", label: "Needs you" },
  { id: "screen", label: "This screen" },
  { id: "panes", label: "Panes" },
  { id: "app", label: "The app" },
];

/** How specific a binding is, least first: where two take a key, the
 * later in this list wins. */
export const CONTEXTS = ["everywhere", "panel", "screen", "dialog"] as const;
export type Context = (typeof CONTEXTS)[number];

export interface Binding {
  id: string;
  /** What it does, for lists of shortcuts. */
  label: string;
  /** Its heading on the `?` sheet (default: this screen's). */
  group?: Group;
  /** Where it applies, for which binding a shared key runs (default: this
   * screen's). */
  context?: Context;
  /** Off the `?` sheet and hints: a screen's own key the sheet doesn't list
   * yet (#1534's list will). */
  unlisted?: boolean;
  /** Its words as a footer chip ("move", "go to"); without, no chip. */
  chip?: string;
  /** Its chords on each OS; the first is the one hints show. */
  keys: Record<Os, string[]>;
  /** The hint, when the first chord doesn't say it (a range of keys). */
  hint?: Partial<Record<Os, string>>;
  /** Whether it exists on this page at all (the desktop app's Cmd keys
   * only in the app on macOS): without it, no hint or sheet shows it. */
  here?: () => boolean;
  /** Whether it applies now; when it doesn't, the key goes on. */
  when?: (e: KeyboardEvent) => boolean;
  run: (e: KeyboardEvent) => void;
}

export const os: Os = /Mac|iPhone|iPad/.test(navigator.platform) ? "mac" : "other";

/** The same chord on both OSes. */
export const both = (...keys: string[]): Record<Os, string[]> => ({ mac: keys, other: keys });

const table: Binding[] = [];
const listeners = new Set<() => void>();
/** Tell the lists of keys that what's `here` changed. */
export const keysChanged = () => listeners.forEach((fn) => fn());

/** Whether `el` takes typing: a field, an editable or a terminal. */
export function typingIn(el: Element | null): boolean {
  if (!el) return false;
  if (el.closest(".xterm, [contenteditable=''], [contenteditable='true'], [data-keys-own]")) return true;
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) return true;
  return el instanceof HTMLInputElement && !["button", "checkbox", "radio", "range", "submit", "reset", "color", "file"].includes(el.type);
}

const plain = (chord: string) => !/\b(Ctrl|Meta|Alt)\+|^\*\+/.test(chord);

function matches(chord: string, e: KeyboardEvent): boolean {
  const parts = chord.split("+");
  const key = parts.pop()!;
  const any = parts[0] === "*";
  // A symbol typed with Shift (`?`) is written without it.
  const symbol = !parts.length && key.startsWith("key:") && !/^key:[a-z0-9]$/.test(key) && key !== "key:escape";
  if (!any) {
    const has = (m: string) => parts.includes(m);
    if (e.ctrlKey !== has("Ctrl") || e.metaKey !== has("Meta") || e.altKey !== has("Alt")) return false;
    if (!symbol && e.shiftKey !== has("Shift")) return false;
  }
  return key.startsWith("key:") ? e.key.toLowerCase() === key.slice(4) : e.code === key;
}

/** The first chord of a sequence, while the page waits for its second. */
let pending: { first: string; until: number } | null = null;

const rank = (b: Binding) => CONTEXTS.indexOf(b.context ?? "screen");

/** What a key does: the one binding it runs, else the sequence it starts.
 * `wait` is a sequence's first chord, pressed just before. */
export function resolve(list: readonly Binding[], e: KeyboardEvent, typing: boolean, wait: string | null): { run: Binding } | { starts: string } | null {
  // The most specific first; of two as specific, the newer.
  const order = list.map((b, i) => ({ b, i })).sort((x, y) => rank(y.b) - rank(x.b) || y.i - x.i);
  let starts: string | null = null;
  for (const { b } of order) {
    if (b.here && !b.here()) continue;
    for (const k of b.keys[os]) {
      if (typing && plain(k)) continue;
      const [first, second] = k.split(" ");
      if (second) {
        if (wait === first && matches(second, e) && (!b.when || b.when(e))) return { run: b };
        if (!wait && !starts && matches(first!, e)) starts = first!;
        continue;
      }
      if (!wait && matches(k, e) && (!b.when || b.when(e))) return { run: b };
    }
  }
  return starts ? { starts } : null;
}

/** A key pressed, `typing` when focus takes typing: runs its one binding.
 * Whether the page took the key (a sequence's first chord, or its second,
 * counts). */
export function press(e: KeyboardEvent, typing: boolean): boolean {
  if (e.isComposing || ["Shift", "Control", "Alt", "Meta"].includes(e.key)) return false;
  // In a field, a sequence never goes on: its next key is typed.
  const wait = !typing && pending && pending.until > e.timeStamp ? pending.first : null;
  pending = null;
  const got = resolve(table, e, typing, wait);
  if (got && "run" in got) got.run.run(e);
  else if (got) pending = { first: got.starts, until: e.timeStamp + 1500 };
  return !!got || !!wait;
}

/** Focus moved: into a field, a sequence's first chord lets go, so the
 * next key is typed, not eaten. */
export function focusMoved(typing: boolean) {
  if (typing) pending = null;
}

function dispatch(e: KeyboardEvent) {
  if (!press(e, typingIn(document.activeElement))) return;
  e.preventDefault();
  e.stopPropagation();
}

let installed = false;
function install() {
  if (installed) return;
  installed = true;
  window.addEventListener("keydown", dispatch, true);
  window.addEventListener("focusin", (e) => focusMoved(e.target instanceof Element && typingIn(e.target)), true);
  // For the e2e specs: the ids the `?` sheet must list.
  (window as unknown as { __arugulaKeys: () => string[] }).__arugulaKeys = () => bindings().map((b) => b.id);
}

/** Add bindings for as long as the page lives, or until the answer is
 * called. */
export function addBindings(list: Binding[]): () => void {
  install();
  table.push(...list);
  keysChanged();
  return () => {
    for (const b of list) {
      const i = table.indexOf(b);
      if (i !== -1) table.splice(i, 1);
    }
    keysChanged();
  };
}

/** Add bindings while something is up (a screen's keys, a dialog's
 * Escape), with the same deps as an effect. Added as soon as it's drawn,
 * before the terminal can take the key. */
export function useBindings(list: Binding[], deps: unknown[]) {
  useLayoutEffect(() => (list.length ? addBindings(list) : undefined), deps);
}

export function useBinding(b: Binding | null, deps: unknown[]) {
  useBindings(b ? [b] : [], deps);
}

/** Escape closes a dialog, before anything under it hears the key. */
export function useEscape(id: string, close: (() => void) | null, deps: unknown[] = []) {
  useBinding(close && { id, label: "Close", group: "app", context: "dialog", keys: both("*+key:escape"), run: close }, deps);
}

/** Every binding this page has, for lists of shortcuts. */
export function bindings(): readonly Binding[] {
  return table.filter((b) => !b.unlisted && (!b.here || b.here()));
}

/** Told when bindings come or go (a screen's keys). */
export function subscribeKeys(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** How a binding's chord reads, as menus, tooltips and the palette show
 * it ("Ctrl+Shift+P", "⌘U", "g o"); undefined when it has none on this OS. */
export function keyHint(id: string): string | undefined {
  const b = table.find((x) => x.id === id);
  if (!b || b.unlisted || (b.here && !b.here())) return undefined;
  return hintOf(b);
}

export function hintOf(b: Binding): string | undefined {
  return b.hint?.[os] ?? (b.keys[os][0] ? formatChord(b.keys[os][0]) : undefined);
}

/** `aria-keyshortcuts` for a hint: its names, not its glyphs. */
export const ariaKeys = (hint: string | undefined) => hint?.replace("Ctrl", "Control");

const NAMES: Record<string, string> = { escape: "esc", enter: "↵", arrowup: "↑", arrowdown: "↓", arrowleft: "←", arrowright: "→", tab: "Tab" };

export function formatChord(chord: string): string {
  if (chord.includes(" ")) return chord.split(" ").map(formatChord).join(" ");
  const parts = chord.split("+");
  const raw = parts.pop()!;
  const mods = parts.filter((m) => m !== "*");
  // A plain key as it's typed: `j`, `?`, `esc`.
  if (!mods.length && raw.startsWith("key:")) return NAMES[raw.slice(4)] ?? raw.slice(4);
  const key = raw.startsWith("key:")
    ? raw.slice(4).toUpperCase()
    : raw.replace(/^Key|^Digit/, "");
  // A Mac's Cmd chords as its menus write them.
  if (os === "mac" && mods.includes("Meta")) {
    const glyph: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Meta: "⌘" };
    return ["Ctrl", "Alt", "Shift", "Meta"].filter((m) => mods.includes(m)).map((m) => glyph[m]).join("") + key;
  }
  return [...mods, key].join("+");
}
