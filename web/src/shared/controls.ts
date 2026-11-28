// The controls (Arugula #1534): every key the page's table binds, by its
// binding's id, with its default chords, where it applies and its verb.
// A binding takes its keys from here (`control(id)`), so a key goes in only
// through this list; scripts/keys-defaults.test.mjs rejects defaults that
// clash. Aaron's scheme (the keys Doc): a small core (j k ↵ esc l a / s ?,
// ⌘K and the `g` places) and one verb per letter, so `a` answers wherever
// there's something to answer. A control with no chords is unbound by
// default; the Controls page (to come) binds it.

import { both, type Binding, type Context, type Group, type Os } from "./keys.ts";

/** Where a control applies: one page, the agent panel (on any page that
 * has one), the machine's workspace, the desktop app, or everywhere. A
 * page's key beats an everywhere one; two in one place may not share one. */
export type Where = "everywhere" | "panel" | "needs" | "board" | "docs" | "chat" | "workspace" | "app";

export interface Control {
  label: string;
  where: Where;
  /** What it does, in a word: two controls on one plain key share a verb,
   * and two with one verb share a key (Answer is `a` on Needs and a board). */
  verb?: string;
  group?: Group;
  /** The first hour's keys, at the top of the `?` card. */
  core?: boolean;
  chip?: string;
  keys: Record<Os, string[]>;
  hint?: Partial<Record<Os, string>>;
}

const none = both();
const inApp = !!(globalThis as { __arugulaApp?: unknown }).__arugulaApp;
const jk = { mac: "j k", other: "j k" };
const range = (n: string) => ({ mac: n, other: n });
const pane = (code: string) => both(`Ctrl+Shift+${code}`);
const app = (code: string) => both(`Meta+${code}`);
const arrows = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].map((k) => `Ctrl+Shift+Alt+${k}`);
const digits = (mods: string) => [1, 2, 3, 4, 5, 6, 7, 8, 9].map((n) => `${mods}+Digit${n}`);
const go = (label: string, key: string): Control => ({ label, where: "everywhere", group: "go", core: true, keys: both(`key:g key:${key}`) });

export const CONTROLS = {
  // Go
  goto: {
    label: "Go to…",
    where: "everywhere",
    group: "go",
    core: true,
    chip: "go to",
    // Ctrl+K leaves a terminal and a field their kill-line: Ctrl+Shift+K is the one from there.
    keys: { mac: ["Meta+KeyK", "Ctrl+Shift+KeyK"], other: ["Ctrl+KeyK", "Ctrl+Shift+KeyK"] }, // gitleaks:allow (chords, not a secret)
  },
  "go.overview": go("Overview", "o"),
  "go.inbox": go("Inbox", "i"),
  "go.chat": go("Chat", "c"),
  "go.boards": go("Boards", "b"),
  "go.docs": go("Docs", "d"),
  "go.machines": go("Machines", "m"),
  "go.agents": { ...go("#agents", "a"), verb: "agents" },
  "board.agents-pane": { label: "#agents, beside the board", where: "board", verb: "agents", group: "go", keys: both("key:g key:a") },
  "chat.agents": { label: "#agents", where: "chat", verb: "agents", group: "go", chip: "#agents", keys: both("key:g key:a") },
  back: { label: "Back a place, across projects", where: "everywhere", verb: "back", group: "switch", core: true, keys: both("key:escape") },
  // Ctrl+Tab is the browser's: only the desktop app (desktop.ts's `__arugulaApp`) gets it.
  flip: { label: "Flip to your last place in another project", where: "everywhere", group: "switch", chip: "flip back", keys: both("key:g key:;", ...(inApp ? ["Ctrl+Tab"] : [])) },

  // Help and Look: Look, light/dark and depth are unbound by default.
  keys: { label: "The controls card", where: "everywhere", group: "app", core: true, keys: both("key:?") },
  look: { label: "Look", where: "everywhere", group: "app", keys: none },
  "look.theme": { label: "Light or dark", where: "everywhere", group: "app", keys: none },
  "look.depth": { label: "Depth, a step deeper", where: "everywhere", group: "app", keys: none },

  // Needs you
  "needs-down": { label: "Next row", where: "needs", verb: "next", group: "needs", chip: "move", keys: both("key:j"), hint: jk },
  "needs-up": { label: "Previous row", where: "needs", verb: "prev", group: "needs", keys: both("key:k") },
  "needs-expand": { label: "Expand or collapse the row", where: "needs", group: "needs", keys: none },
  "needs-yes": { label: "Answer the first way", where: "needs", verb: "answer", group: "needs", chip: "answer", keys: both("key:a"), hint: range("a x") },
  "needs-no": { label: "Answer the second way", where: "needs", verb: "decline", group: "needs", keys: both("key:x") },
  "needs-done": { label: "Done, on a to-do", where: "needs", verb: "done", group: "needs", keys: both("Shift+key:d"), hint: range("D") },
  "needs-next": { label: "Next thing that needs you", where: "needs", group: "needs", keys: none },
  "needs-open": { label: "Open it where it is", where: "needs", verb: "open", group: "needs", chip: "go there", keys: both("Enter"), hint: range("↵") },

  // Docs
  "docs.next": { label: "The next Doc", where: "docs", verb: "next", chip: "move", keys: both("key:j", "ArrowDown"), hint: jk },
  "docs.prev": { label: "The Doc before", where: "docs", verb: "prev", keys: both("key:k", "ArrowUp") },
  "docs.open": { label: "Open the Doc", where: "docs", verb: "open", chip: "open", keys: both("Enter") },
  "docs.mine": { label: "Mine or all", where: "docs", verb: "mine", chip: "mine", keys: both("key:m") },

  // A board
  "board.tab": { label: "A tab of the board or the Overview", where: "board", chip: "tabs", keys: both("Digit1", "Digit2", "Digit3", "Digit4"), hint: range("1–4") },
  "board.pane.back": { label: "Back from a maximised pane", where: "board", verb: "back", group: "panes", keys: both("key:escape"), hint: range("esc") },
  "board.next": { label: "The next row", where: "board", verb: "next", chip: "move", keys: both("key:j"), hint: jk },
  "board.prev": { label: "The row before", where: "board", verb: "prev", keys: both("key:k") },
  "board.find": { label: "Filter the rows", where: "board", verb: "find", keys: both("key:/") },
  "board.add": { label: "Add a row", where: "board", verb: "new", keys: both("key:n") },
  "board.lead": { label: "Write to the lead", where: "board", verb: "answer", chip: "lead", keys: both("key:a") },

  // Chat: ⌘K is the room switcher there (Aaron's choice), over Go to….
  "chat.rooms": { label: "Jump to a room", where: "chat", group: "go", keys: both("Meta+key:k", "Ctrl+key:k") },
  "chat.as-pane": { label: "Open as a pane", where: "chat", verb: "pane", chip: "open as a pane", keys: both("key:o") },
  "chat.thread-back": { label: "Back to the room", where: "chat", verb: "back", chip: "back", keys: both("key:escape") },
  "chat.agents-next": { label: "The next agents' room", where: "chat", verb: "next", chip: "move", keys: both("key:j"), hint: jk },
  "chat.agents-prev": { label: "The agents' room before", where: "chat", verb: "prev", keys: both("key:k") },
  "chat.staff": { label: "Your chief of staff", where: "chat", verb: "staff", chip: "chief of staff", keys: both("key:s") },

  // The agent panel
  "panel-focus": { label: "Focus the agent panel", where: "panel", verb: "panel", core: true, chip: "panel", keys: both("key:l") },
  "panel-work": { label: "Focus the work beside the panel", where: "panel", keys: none },
  "panel-staff": { label: "The chief of staff, and back", where: "panel", verb: "staff", core: true, keys: both("key:s") },
  "panel-view": { label: "Transcript or terminal, in the panel", where: "panel", verb: "view", keys: both("key:v") },
  "panel-floor": { label: "The panel's transcript or terminal full screen, and back", where: "panel", verb: "floor", keys: both("key:f") },
  "panel-narrow": { label: "Narrower panel", where: "panel", verb: "narrower", keys: both("key:[") },
  "panel-wide": { label: "Wider panel", where: "panel", verb: "wider", keys: both("key:]") },
  "panel-collapse": { label: "Collapse the panel to a strip, and back", where: "panel", verb: "collapse", keys: both("key:\\") },
  // From a field too (the floor's composer), but never a terminal's.
  "panel-back": { label: "Leave the floor, or a thread in the panel", where: "panel", verb: "back", keys: both("*+key:escape"), hint: range("esc") },

  // The machine's workspace. On a Mac, Cmd+Shift+3, 4 and 5 are the
  // system's screenshots and ⌘⇧Z is redo: Ctrl+Shift on both.
  "pane.jump": { label: "Go to pane 1–9", where: "workspace", group: "panes", chip: "go to pane", keys: both(...digits("Ctrl+Shift"), ...digits("Meta+Shift")), hint: range("Ctrl+Shift+1–9") },
  "pane.focus": { label: "Focus pane / restore layout", where: "workspace", group: "panes", chip: "focus", keys: pane("KeyZ") },
  "pane.pop-out": { label: "Pop the pane out into its own window", where: "workspace", group: "panes", keys: { mac: ["Ctrl+Shift+KeyO", "Meta+Shift+KeyO"], other: ["Ctrl+Shift+KeyO"] } },
  "pane.add": { label: "Add a pane", where: "workspace", group: "panes", keys: { mac: ["Ctrl+Shift+KeyE", "Meta+Shift+KeyE"], other: ["Ctrl+Shift+KeyE"] } },
  "pane.next-needs-you": { label: "Go to the next pane that needs you", where: "workspace", group: "panes", keys: { mac: ["Ctrl+Shift+KeyL", "Meta+Shift+KeyL"], other: ["Ctrl+Shift+KeyL"] } },
  "pane.resize": { label: "Grow or shrink the pane", where: "workspace", group: "panes", keys: both(...arrows), hint: { mac: "⌃⌥⇧ arrows", other: "Ctrl+Alt+Shift+arrows" } },

  // The desktop app on macOS.
  "desktop.close": { label: "Close pane", where: "app", group: "app", keys: app("KeyW") },
  "desktop.tab": { label: "New tab", where: "app", group: "app", keys: app("KeyT") },
  "desktop.window": { label: "New window", where: "app", group: "app", keys: app("KeyN") },
  "desktop.attach": { label: "Attach file", where: "app", group: "app", keys: app("KeyU") },
} satisfies Record<string, Control>;

export type ControlId = keyof typeof CONTROLS;

const contextOf = (w: Where): Context => (w === "everywhere" ? "everywhere" : w === "panel" ? "panel" : "screen");

/** A binding's id, words, place and keys, as the list has them: the
 * binding adds what it does (`run`, `when`). */
export function control(id: ControlId): Pick<Binding, "id" | "label" | "group" | "context" | "core" | "chip" | "keys" | "hint"> {
  const c: Control = CONTROLS[id];
  return { id, label: c.label, group: c.group, context: contextOf(c.where), core: c.core, chip: c.chip, keys: c.keys, hint: c.hint };
}
