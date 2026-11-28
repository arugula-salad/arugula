// The shell's keys (M107, #1022), in the page's one table (shared/keys.ts,
// which says how chords are written): the panes' and the desktop app's.
// Go to… (⌘K) is the frame's. Each takes its key only while the workspace is up
// (`here`), so the app's other pages list none of them.
//
// Panes (M110): Ctrl+Shift+Z toggles the tab's focus on the active
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
import { control } from "../../shared/controls";
import { addBindings, keysChanged, typingIn } from "../../shared/keys";
import type { Client } from "../client";
import { desktopPlatform, openInNewWindow } from "../../shared/desktop";
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

const mac = desktopPlatform() === "macos";
/** The desktop app's Cmd chords. `when` checks the app's platform, not the
 * browser's OS, so they're listed for both; `here` keeps their hints off
 * every other page (M113). */
const appKey = (id: "desktop.close" | "desktop.tab" | "desktop.window" | "desktop.attach") => ({ ...control(id), here: () => mac });
/** Only while the workspace is up. */
const inWorkspace = () => !!workspace;

addBindings([
  {
    // By code, so Shift's symbols and the layout don't matter.
    ...control("pane.jump"),
    here: inWorkspace,
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
    ...control("pane.focus"),
    here: inWorkspace,
    // A field's own Ctrl+Shift+Z is its redo; a terminal's goes to the pane.
    when: () => focusTarget() !== null && !(typingIn(document.activeElement) && !document.activeElement?.closest(".xterm")),
    run: () => toggleFocus(workspace!.client, focusTarget()!),
  },
  {
    ...control("pane.pop-out"),
    here: inWorkspace,
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
    ...control("pane.add"),
    here: inWorkspace,
    when: () => addPane.available && focusTarget(true) !== null,
    run: () => addPane.toggle(),
  },
  {
    ...control("pane.next-needs-you"),
    here: inWorkspace,
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
    ...control("pane.resize"),
    here: inWorkspace,
    // Taken only when it resized something.
    when: (e) => focusTarget() !== null && resizeActive(workspace!.client, e.code.slice("Arrow".length).toLowerCase() as Way),
    run: () => {},
  },
  // The desktop app on macOS (desktop.ts): its menus leave these to the
  // page. Cmd-W closes the pane (not the window), Cmd-T opens a tab, Cmd-N
  // a window, and Cmd-U attaches files (M70).
  {
    ...appKey("desktop.close"),
    when: () => mac && !!appClient(),
    run: () => {
      const c = appClient()!;
      const pane = c.active();
      if (pane !== undefined) c.intent({ op: "close_pane", pane });
    },
  },
  {
    ...appKey("desktop.tab"),
    when: () => mac && !!appClient(),
    run: () => {
      const c = appClient()!;
      if (c.session !== null) c.intent({ op: "new_tab", session: c.session, from_pane: c.active() ?? null });
    },
  },
  {
    ...appKey("desktop.window"),
    when: () => mac && !!appClient(),
    run: () => openInNewWindow(),
  },
  {
    ...appKey("desktop.attach"),
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
  if (!w || w.phone || !w.client.state) return null;
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
  if (!w || w.phone || !w.client.state) return null;
  // Not from under a dialog.
  if (document.querySelector(".prompt-backdrop")) return null;
  const tab = w.client.tabView();
  if (!tab || layoutRefusal(w.client, tab)) return null;
  if (!one && tab.focus === null && paneIds(tab).length < 2) return null;
  return tab;
}

/** Take the keys for the page; `client` is read on each press (main.tsx
 * swaps it when the host changes). The answer gives them back: the app
 * (Arugula #989) holds them only while a machine's page is shown,
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
