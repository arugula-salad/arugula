// The page's keyboard shortcuts, in one table (M107, #1022): each binding's
// id, its chords per OS, when it applies and what it does. One capture-phase
// listener on the window reads the table, so a chord is the page's before
// the terminal sees it; a key no binding takes goes on to xterm untouched.
// Menus, tooltips and the palette read their hints from here (`keyHint`).
//
// Chords are written `Mod+Mod+Key`: the modifiers are Ctrl, Meta (⌘ on a
// Mac), Shift and Alt, matched exactly; `*+Key` takes any modifiers. Key is
// an `event.code` (`KeyP`, `Digit1`, `Escape`), so the keyboard's layout
// doesn't matter, or `key:x` for what the key types (`event.key`, any case).
//
// Panes (M110): Ctrl+Shift+Enter toggles the tab's focus on the active
// pane, Ctrl+Shift+E opens + Pane (the new workspace's), and
// Ctrl+Alt+Shift+arrows grow (→ ↓) or shrink (← ↑) the active pane. Each
// takes its key only when it can act (a tab with more than one pane, a
// split that way), so otherwise the terminal still gets it. The resize
// chord is one no shell, editor or browser binds; GNOME's default
// "move window to workspace" takes it before the page sees it, so there
// it doesn't work. Ctrl+Shift+N can't be one: Chrome keeps it for
// incognito.

import { useLayoutEffect } from "preact/hooks";
import type { Client } from "../client";
import { cloudPanes } from "../cloud";
import { desktopPlatform, openInNewWindow } from "../desktop";
import { swarmRoute } from "../swarm/route";
import { agentsRoute } from "./agents-route";
import { togglePalette } from "./palette";
import { openPicker } from "./picker";
import { isChatOpen } from "./places";
import { paneIds } from "../client";
import { layoutRefusal, resizeActive, toggleFocus, type Way } from "./panes/layout";
import { addPane } from "./shell/add-pane";

export type Os = "mac" | "other";

export interface Binding {
  id: string;
  /** What it does, for lists of shortcuts. */
  label: string;
  /** Its chords on each OS; the first is the one hints show. */
  keys: Record<Os, string[]>;
  /** The hint, when the first chord doesn't say it (a range of keys). */
  hint?: Partial<Record<Os, string>>;
  /** Whether it exists on this page at all (the desktop app's Cmd keys
   * only in the app on macOS): without it, no hint shows it. */
  here?: () => boolean;
  /** Whether it applies now; when it doesn't, the key goes on. */
  when?: (e: KeyboardEvent) => boolean;
  run: (e: KeyboardEvent) => void;
}

export const os: Os = /Mac|iPhone|iPad/.test(navigator.platform) ? "mac" : "other";

/** The workspace (`App`) on the page, while it's there: its client and
 * whether it's laid out for a phone. */
let workspace: { client: Client; phone: boolean } | null = null;
/** The shown machine's client, for the desktop app's keys, which work
 * without the workspace (main.tsx swaps it when the host changes). */
let appClient: () => Client | null = () => workspace?.client ?? null;

const ctrlOrCmd = (code: string) => [`Ctrl+Shift+${code}`, `Meta+Shift+${code}`];
const mac = desktopPlatform() === "macos";
/** The desktop app's Cmd chords. `when` checks the app's platform, not the
 * browser's OS, so they're listed for both; `here` keeps their hints off
 * every other page (M113). */
const appKey = (code: string) => ({ keys: { mac: [`Meta+${code}`], other: [`Meta+${code}`] }, here: () => mac });

const table: Binding[] = [
  {
    id: "palette",
    label: "Command palette",
    // A Mac's Cmd too, for the desktop app (its menu leaves Cmd chords to
    // the page) and Safari. Again closes it.
    keys: { mac: ctrlOrCmd("KeyP"), other: ctrlOrCmd("KeyP") },
    when: () => !!workspace,
    run: () => togglePalette(workspace!.client, workspace!.phone),
  },
  {
    id: "picker",
    label: "Go to directory",
    keys: { mac: ["Ctrl+Shift+key:g"], other: ["Ctrl+Shift+key:g"] },
    when: () => !!workspace,
    run: () => openPicker(workspace!.client, workspace!.client.active(), workspace!.phone),
  },
  {
    id: "pane.jump",
    label: "Go to pane 1–9",
    // By code, so Shift's symbols and the layout don't matter. On a Mac,
    // Cmd+Shift+3, 4 and 5 are the system's screenshots: Ctrl+Shift works
    // for all nine.
    keys: {
      mac: [1, 2, 3, 4, 5, 6, 7, 8, 9].flatMap((n) => ctrlOrCmd(`Digit${n}`)),
      other: [1, 2, 3, 4, 5, 6, 7, 8, 9].flatMap((n) => ctrlOrCmd(`Digit${n}`)),
    },
    hint: { mac: "Ctrl+Shift+1–9", other: "Ctrl+Shift+1–9" },
    when: (e) => jumpTarget(e) !== null,
    run: (e) => {
      const c = workspace!.client;
      const pane = jumpTarget(e)!;
      // Off a cloud run's details, back to the panes (as a tab's click).
      cloudPanes.local();
      // A focused tab shows the pane asked for instead.
      const tab = c.tabOfPane(pane);
      if (tab && tab.focus !== null && tab.focus !== pane) c.intent({ op: "focus", tab: tab.id, pane });
      c.setActive(pane);
      c.viewOf(pane)?.focus();
    },
  },
  {
    id: "pane.focus",
    label: "Focus pane / restore layout",
    keys: { mac: ctrlOrCmd("Enter"), other: ["Ctrl+Shift+Enter"] },
    when: () => focusTarget() !== null,
    run: () => toggleFocus(workspace!.client, focusTarget()!),
  },
  {
    id: "pane.add",
    label: "Add a pane",
    keys: { mac: ctrlOrCmd("KeyE"), other: ["Ctrl+Shift+KeyE"] },
    when: () => addPane.available && focusTarget(true) !== null,
    run: () => addPane.toggle(),
  },
  {
    id: "pane.resize",
    label: "Grow or shrink the pane",
    keys: {
      mac: ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].map((k) => `Ctrl+Shift+Alt+${k}`),
      other: ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].map((k) => `Ctrl+Shift+Alt+${k}`),
    },
    hint: { mac: "⌃⌥⇧ arrows", other: "Ctrl+Alt+Shift+arrows" },
    // Taken only when it resized something.
    when: (e) => focusTarget() !== null && resizeActive(workspace!.client, e.code.slice("Arrow".length).toLowerCase() as Way),
    run: () => {},
  },
  // The desktop app on macOS (desktop.ts): its menus leave these to the
  // page. Cmd-W closes the pane (not the window), Cmd-T opens a tab, Cmd-N
  // a window, and Cmd-U attaches files (M70).
  {
    id: "desktop.close",
    label: "Close pane",
    ...appKey("KeyW"),
    when: () => mac && !!appClient(),
    run: () => {
      const c = appClient()!;
      const pane = c.active();
      if (pane !== undefined) c.intent({ op: "close_pane", pane });
    },
  },
  {
    id: "desktop.tab",
    label: "New tab",
    ...appKey("KeyT"),
    when: () => mac && !!appClient(),
    run: () => {
      const c = appClient()!;
      if (c.session !== null) c.intent({ op: "new_tab", session: c.session, from_pane: c.active() ?? null });
    },
  },
  {
    id: "desktop.window",
    label: "New window",
    ...appKey("KeyN"),
    when: () => mac && !!appClient(),
    run: () => openInNewWindow(),
  },
  {
    id: "desktop.attach",
    label: "Attach file",
    ...appKey("KeyU"),
    when: () => mac && !!appClient(),
    run: () => {
      // The agent composer's 📎 when one has focus or is the active pane,
      // else the active terminal's Attach file….
      const c = appClient()!;
      const pane = c.active();
      const el = pane === undefined ? null : document.querySelector(`[data-pane="${pane}"]`);
      const composer = document.activeElement?.closest(".agent-composer") ?? el?.querySelector(".agent-composer");
      const attach = composer?.querySelector<HTMLButtonElement>("button.attach");
      if (attach) attach.click();
      else if (pane !== undefined && c.panes.has(pane) && c.mayType(pane)) void c.attachFiles(pane);
    },
  },
];

/** The pane Ctrl/Cmd+Shift+N goes to: the Nth of the shown tab's, in
 * reading order, when the workspace shows panes and has that many. */
function jumpTarget(e: KeyboardEvent): number | null {
  const w = workspace;
  if (!w || w.phone || !w.client.state || isChatOpen() || swarmRoute() || agentsRoute()) return null;
  // Not from under a dialog (the palette, the picker, a prompt).
  if (document.querySelector(".prompt-backdrop")) return null;
  const n = Number(e.code.slice("Digit".length));
  // In reading order, also the panes a focused tab doesn't show.
  const tab = w.client.tabView();
  return tab ? (paneIds(tab)[n - 1] ?? null) : null;
}

/** The shown tab, when the workspace shows panes and may change their
 * layout (with `one`, also a tab of one pane). */
function focusTarget(one = false) {
  const w = workspace;
  if (!w || w.phone || !w.client.state || isChatOpen() || swarmRoute() || agentsRoute()) return null;
  // Not from under a dialog, nor over the session overview (M111).
  if (document.querySelector(".prompt-backdrop") || location.hash.startsWith("#overview")) return null;
  const tab = w.client.tabView();
  if (!tab || layoutRefusal(w.client, tab)) return null;
  if (!one && tab.focus === null && paneIds(tab).length < 2) return null;
  return tab;
}

function matches(chord: string, e: KeyboardEvent): boolean {
  const parts = chord.split("+");
  const key = parts.pop()!;
  const any = parts[0] === "*";
  if (!any) {
    const has = (m: string) => parts.includes(m);
    if (e.ctrlKey !== has("Ctrl") || e.metaKey !== has("Meta") || e.shiftKey !== has("Shift") || e.altKey !== has("Alt")) return false;
  }
  return key.startsWith("key:") ? e.key.toLowerCase() === key.slice(4) : e.code === key;
}

function dispatch(e: KeyboardEvent) {
  let ran = false;
  // Every binding that takes the key runs, in the order they were added:
  // two dialogs open each close on Escape, as their own listeners did.
  for (const b of [...table]) {
    if (!b.keys[os].some((k) => matches(k, e)) || (b.when && !b.when(e))) continue;
    ran = true;
    b.run(e);
  }
  if (!ran) return;
  e.preventDefault();
  e.stopPropagation();
}

let installed = false;
function install() {
  if (installed) return;
  installed = true;
  window.addEventListener("keydown", dispatch, true);
}

/** Take the keys for the page; `client` is read on each press (main.tsx
 * swaps it when the host changes). */
export function installKeys(client: () => Client) {
  appClient = client;
  install();
}

/** Add a binding while something is up (a dialog's Escape), with the
 * same deps as an effect. Added as soon as it's drawn, before the
 * terminal can take the key. */
export function useBinding(b: Binding | null, deps: unknown[]) {
  useLayoutEffect(() => {
    if (!b) return;
    install();
    table.push(b);
    return () => {
      const i = table.indexOf(b);
      if (i !== -1) table.splice(i, 1);
    };
  }, deps);
}

/** Escape closes a dialog, before anything under it hears the key. */
export function useEscape(id: string, close: (() => void) | null, deps: unknown[] = []) {
  useBinding(close && { id, label: "Close", keys: { mac: ["*+key:escape"], other: ["*+key:escape"] }, run: close }, deps);
}

/** The workspace's keys (palette, picker, pane jumps) while it's up. */
export function useWorkspaceKeys(client: Client, phone: boolean) {
  useLayoutEffect(() => {
    const me = { client, phone };
    workspace = me;
    install();
    return () => {
      if (workspace === me) workspace = null;
    };
  }, [client, phone]);
}

/** Every binding, for lists of shortcuts. */
export function bindings(): readonly Binding[] {
  return table;
}

/** How a binding's chord reads, as menus, tooltips and the palette show
 * it ("Ctrl+Shift+P", "⌘U"); undefined when it has none on this OS. */
export function keyHint(id: string): string | undefined {
  const b = table.find((x) => x.id === id);
  if (!b || (b.here && !b.here())) return undefined;
  return b.hint?.[os] ?? (b.keys[os][0] ? formatChord(b.keys[os][0]) : undefined);
}

/** `aria-keyshortcuts` for a hint: its names, not its glyphs. */
export const ariaKeys = (hint: string | undefined) => hint?.replace("Ctrl", "Control");

export function formatChord(chord: string): string {
  const parts = chord.split("+");
  const raw = parts.pop()!;
  const key = raw.startsWith("key:")
    ? raw.slice(4).toUpperCase()
    : raw.replace(/^Key|^Digit/, "");
  const mods = parts.filter((m) => m !== "*");
  // A Mac's Cmd chords as its menus write them.
  if (os === "mac" && mods.includes("Meta")) {
    const glyph: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Meta: "⌘" };
    return ["Ctrl", "Alt", "Shift", "Meta"].filter((m) => mods.includes(m)).map((m) => glyph[m]).join("") + key;
  }
  return [...mods, key].join("+");
}
