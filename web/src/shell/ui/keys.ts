// The shell's keys (M107, #1022), in the page's one table (shared/keys.ts,
// which says how chords are written): the panes' and the desktop app's.
// Go to… (⌘K) is the frame's. Each takes its key only while the workspace is up
// (`here`), so the app's other pages list none of them.
//
// Panes (M110): Ctrl+Shift+Enter toggles the tab's focus on the active
// pane, Ctrl+Shift+E opens + Pane (the pane toolbar's), and
// Ctrl+Alt+Shift+arrows grow (→ ↓) or shrink (← ↑) the active pane, and
// Ctrl+Shift+L goes to the next pane that needs you. Each
// takes its key only when it can act (a tab with more than one pane, a
// split that way), so otherwise the terminal still gets it. The resize
// chord is one no shell, editor or browser binds; GNOME's default
// "move window to workspace" takes it before the page sees it, so there
// it doesn't work. Ctrl+Shift+N can't be one: Chrome keeps it for
// incognito.

import { useLayoutEffect } from "preact/hooks";
import { addBindings, both, keysChanged } from "../../shared/keys";
import type { Client } from "../client";
import { desktopPlatform, openInNewWindow } from "../../shared/desktop";
import { swarmRoute } from "../swarm/route";
import { agentsRoute } from "./agents-route";
import { docsRoute } from "./docs-route";
import { nextNeedingYou } from "./goto-source";
import { paneIds } from "../client";
import { layoutRefusal, resizeActive, toggleFocus, type Way } from "./panes/layout";
import { canPopOut, popOut } from "./panes/pop-out";
import { addPane } from "./shell/add-pane";

/** The workspace (`App`) on the page, while it's there: its client and
 * whether it's laid out for a phone. */
let workspace: { client: Client; phone: boolean } | null = null;
/** The shown machine's client, for the desktop app's keys, which work
 * without the workspace (main.tsx swaps it when the host changes). */
const workspaceClient = () => workspace?.client ?? null;
let appClient: () => Client | null = workspaceClient;

const ctrlOrCmd = (code: string) => [`Ctrl+Shift+${code}`, `Meta+Shift+${code}`];
const mac = desktopPlatform() === "macos";
/** The desktop app's Cmd chords. `when` checks the app's platform, not the
 * browser's OS, so they're listed for both; `here` keeps their hints off
 * every other page (M113). */
const appKey = (code: string) => ({ group: "app" as const, keys: both(`Meta+${code}`), here: () => mac });
/** Only while the workspace is up. */
const inWorkspace = () => !!workspace;

addBindings([
  {
    id: "pane.jump",
    group: "panes",
    chip: "go to pane",
    here: inWorkspace,
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
      // A focused tab shows the pane asked for instead.
      const tab = c.tabOfPane(pane);
      if (tab && tab.focus !== null && tab.focus !== pane) c.intent({ op: "focus", tab: tab.id, pane });
      c.setActive(pane);
      c.viewOf(pane)?.focus();
    },
  },
  {
    id: "pane.focus",
    group: "panes",
    chip: "focus",
    here: inWorkspace,
    label: "Focus pane / restore layout",
    keys: { mac: ctrlOrCmd("Enter"), other: ["Ctrl+Shift+Enter"] },
    when: () => focusTarget() !== null,
    run: () => toggleFocus(workspace!.client, focusTarget()!),
  },
  {
    id: "pane.pop-out",
    group: "panes",
    here: inWorkspace,
    label: "Pop the pane out into its own window",
    keys: { mac: ctrlOrCmd("KeyO"), other: ["Ctrl+Shift+KeyO"] },
    when: () => {
      const w = workspace;
      const pane = focusTarget() && w ? w.client.active(w.client.tabView()!.id) : undefined;
      return pane !== undefined && canPopOut(w!.client, pane, w!.phone);
    },
    run: () => {
      const c = workspace!.client;
      popOut(c, c.active(c.tabView()!.id)!);
    },
  },
  {
    id: "pane.add",
    group: "panes",
    here: inWorkspace,
    label: "Add a pane",
    keys: { mac: ctrlOrCmd("KeyE"), other: ["Ctrl+Shift+KeyE"] },
    when: () => addPane.available && focusTarget(true) !== null,
    run: () => addPane.toggle(),
  },
  {
    id: "pane.next-needs-you",
    group: "panes",
    here: inWorkspace,
    label: "Go to the next pane that needs you",
    keys: { mac: ctrlOrCmd("KeyL"), other: ["Ctrl+Shift+KeyL"] },
    // Taken only when a pane needs you, and not from under a dialog.
    when: () => !!workspace?.client.state && !document.querySelector(".prompt-backdrop") && nextNeedingYou(workspace.client) !== undefined,
    run: () => {
      const c = workspace!.client;
      const pane = nextNeedingYou(c)!;
      c.setActive(pane);
      requestAnimationFrame(() => c.viewOf(pane)?.focus());
    },
  },
  {
    id: "pane.resize",
    group: "panes",
    here: inWorkspace,
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
      // A chat composer's 📎 when focus is in one (the thread's, else the
      // room's of the chat block), else the agent composer's when one has
      // focus or is the active pane, else the active terminal's Attach file….
      const c = appClient()!;
      const chat = document.activeElement?.closest(".thread-compose") ?? document.activeElement?.closest("[data-chat-block]")?.querySelector(".thread-compose");
      const chatAttach = chat?.querySelector<HTMLButtonElement>("[data-attach-button]");
      if (chatAttach) return chatAttach.click();
      const pane = c.active();
      const el = pane === undefined ? null : document.querySelector(`[data-pane="${pane}"]`);
      const composer = document.activeElement?.closest(".agent-composer") ?? el?.querySelector(".agent-composer");
      const attach = composer?.querySelector<HTMLButtonElement>("button.attach");
      if (attach) attach.click();
      else if (pane !== undefined && c.panes.has(pane) && c.mayType(pane)) void c.attachFiles(pane);
    },
  },
]);

/** The pane Ctrl/Cmd+Shift+N goes to: the Nth of the shown tab's, in
 * reading order, when the workspace shows panes and has that many. */
function jumpTarget(e: KeyboardEvent): number | null {
  const w = workspace;
  if (!w || w.phone || !w.client.state || swarmRoute() || agentsRoute() || docsRoute()) return null;
  // Not from under a dialog (Go to…, the picker, a prompt).
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
  if (!w || w.phone || !w.client.state || swarmRoute() || agentsRoute() || docsRoute()) return null;
  // Not from under a dialog.
  if (document.querySelector(".prompt-backdrop")) return null;
  const tab = w.client.tabView();
  if (!tab || layoutRefusal(w.client, tab)) return null;
  if (!one && tab.focus === null && paneIds(tab).length < 2) return null;
  return tab;
}

/** Take the keys for the page; `client` is read on each press (main.tsx
 * swaps it when the host changes). The answer gives them back: the app
 * (Arugula #989) holds them only while a machine's page or Swarm is shown,
 * so its other pages keep only their own keys. */
export function installKeys(client: () => Client): () => void {
  appClient = client;
  return () => {
    if (appClient === client) appClient = workspaceClient;
  };
}

/** The workspace's keys (pane jumps, focus, + Pane) while it's up. */
export function useWorkspaceKeys(client: Client, phone: boolean) {
  useLayoutEffect(() => {
    const me = { client, phone };
    workspace = me;
    keysChanged();
    return () => {
      if (workspace !== me) return;
      workspace = null;
      keysChanged();
    };
  }, [client, phone]);
}
