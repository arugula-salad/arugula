// A dialog on control's page: the trust screens' (web/src/shell/trust/)
// and the platform's settings (web/src/platform/settings/) alike, so both
// sides draw it the same (M149).
//
// UI audit #150: every dialog behaves the same way. Focus moves into it and
// stays there (Tab wraps), Escape closes it before the terminal under it can
// hear the key, and focus goes back to what opened it. Dialogs drawn by hand
// (Permission rules, Appearance, Getting started…) use `useModal` for this.
//
// A pane modal (a big tip, shell/hints) is a dialog over one pane, not the
// window: `useModal`'s `covers` names the pane. The pane under it is inert
// (no clicks, keys or focus reach it, a frame's neither), focus moves in
// when it was on that pane, and Escape and Tab are its own only while the
// focus is in it. The rest of the workspace stays usable: focus may leave
// for another pane, and nothing pulls it back.

import { useLayoutEffect, useRef } from "preact/hooks";
import { escapeTaken } from "./menu";

interface Open {
  el: () => HTMLElement | null;
  close: () => (() => void) | undefined;
  /** A pane modal's: the pane it covers. */
  covers?: () => HTMLElement | null;
}

/** The dialogs up, latest (the one on top) last. */
const stack: Open[] = [];

/** The dialog over the window on top that's drawn. */
function top(): Open | undefined {
  for (let i = stack.length - 1; i >= 0; i--) if (!stack[i].covers && stack[i].el()?.isConnected) return stack[i];
  return undefined;
}

/** The pane modal drawn that has `at`, in it or in the pane it covers. */
function paneModalAt(at: Element | null): Open | undefined {
  if (!at) return undefined;
  for (let i = stack.length - 1; i >= 0; i--) {
    const o = stack[i];
    const el = o.el();
    if (o.covers && el?.isConnected && (el.contains(at) || o.covers()?.contains(at))) return o;
  }
  return undefined;
}

const watchers = new Set<() => void>();

/** A dialog is up. What opens by itself (Getting started's greeting)
 * waits until it's answered: one dialog at a time, never one under
 * another (Arugula #280). */
export const dialogUp = (): boolean => top() !== undefined;

/** Call `fn` when a dialog opens or closes; returns the unsubscribe. */
export function subscribeDialogs(fn: () => void): () => void {
  watchers.add(fn);
  return () => void watchers.delete(fn);
}

const dialogsChanged = () => watchers.forEach((fn) => fn());

const FOCUSABLE = 'button, [href], input, select, textarea, summary, [tabindex]:not([tabindex="-1"])';

function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (x) => !(x as HTMLButtonElement).disabled && x.getClientRects().length > 0 && !x.closest("[inert]"),
  );
}

/** Where a dialog takes the focus: what asks for it, else the dialog. */
function focusInto(root: HTMLElement) {
  if (root.contains(document.activeElement)) return;
  const want = root.querySelector<HTMLElement>("[autofocus], [data-autofocus]");
  (want ?? root).focus({ preventScroll: true });
}

function onKey(e: KeyboardEvent) {
  // A pane modal's keys are its own while the focus is in it; a dialog
  // over the window's always are.
  const up = paneModalAt(document.activeElement) ?? top();
  const root = up?.el();
  if (!up || !root) return;
  if (e.key === "Escape") {
    // A menu open over the dialog takes its own Escape (menu.tsx).
    if (e.isComposing || escapeTaken(e)) return;
    e.preventDefault();
    e.stopPropagation();
    up.close()?.();
    return;
  }
  if (e.key !== "Tab") return;
  const list = focusables(root);
  const at = document.activeElement as HTMLElement | null;
  if (!list.length) {
    e.preventDefault();
    root.focus({ preventScroll: true });
    return;
  }
  const first = list[0];
  const last = list[list.length - 1];
  if (!at || !root.contains(at)) {
    e.preventDefault();
    (e.shiftKey ? last : first).focus();
  } else if (e.shiftKey && (at === first || at === root)) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && at === last) {
    e.preventDefault();
    first.focus();
  }
}

/** Focus that wanders out (the terminal taking it back) comes back in. A
 * menu or a prompt over the dialog keeps what it takes. */
function onFocusIn(e: FocusEvent) {
  const t = e.target as HTMLElement | null;
  // Into a pane a pane modal covers: the modal takes it.
  const over = paneModalAt(t);
  const mine = over?.el();
  if (mine && t && !mine.contains(t)) return focusInto(mine);
  const root = top()?.el();
  if (!root || !t || root.contains(t) || t.closest?.(".menu, .prompt-backdrop")) return;
  focusInto(root);
}

let installed = false;
function install() {
  if (installed) return;
  installed = true;
  // Capture, on the window: before the terminal's textarea hears the key.
  window.addEventListener("keydown", onKey, true);
  document.addEventListener("focusin", onFocusIn, true);
}

/** Make `ref` a modal dialog while `open`: it takes the focus and keeps it,
 * Escape calls `close` (and never reaches what's under it, even when there
 * is no `close`), and the focus goes back where it was when it shuts.
 *
 * With `covers`, it's a pane modal over that pane alone (above): the pane
 * is inert while it's up, and the focus moves in only from that pane (or
 * from nowhere), and back to it after. */
export function useModal(ref: { current: HTMLElement | null }, close: (() => void) | undefined, open = true, covers?: () => HTMLElement | null) {
  const latest = useRef(close);
  latest.current = close;
  useLayoutEffect(() => {
    if (!open) return;
    install();
    // A menu's item gives the focus back to what opened the menu before it
    // runs (menu.tsx), so this is the account button, a pane, the palette's.
    const opener = document.activeElement as HTMLElement | null;
    const me: Open = { el: () => ref.current, close: () => latest.current, covers };
    const pane = covers?.() ?? null;
    // A pane modal takes the focus from its pane (a terminal, a composer,
    // a frame) or from nowhere, never from another pane or a dialog.
    const take = !covers || !opener || opener === document.body || (pane?.contains(opener) ?? false);
    stack.push(me);
    if (pane) pane.inert = true;
    const root = ref.current;
    if (root) {
      if (!root.hasAttribute("tabindex")) root.tabIndex = -1;
      if (take) focusInto(root);
    }
    dialogsChanged();
    return () => {
      const i = stack.indexOf(me);
      if (i !== -1) stack.splice(i, 1);
      if (pane) pane.inert = false;
      dialogsChanged();
      // Only when the focus was ours: a dialog shut because another opened
      // leaves the new one's focus be.
      const at = document.activeElement;
      const ours = !at || at === document.body || !at.isConnected || (root?.contains(at) ?? false);
      const under = top()?.el();
      if (!ours) return;
      if (covers) {
        // Back to what had the focus in the pane (the caller gives a pane
        // that had none its own).
        if (take && opener?.isConnected && opener !== document.body) opener.focus({ preventScroll: true });
        return;
      }
      if (under && !under.contains(opener)) focusInto(under);
      else if (opener?.isConnected && opener !== document.body) opener.focus({ preventScroll: true });
    };
  }, [open]);
}

/** The corner ×, pinned where it can always be seen: Modal's, and the
 * dialogs drawn by hand (Permission rules, Developer settings, Usage and
 * crash reports: UI audit #158). */
export function CloseX({ close }: { close: () => void }) {
  return (
    <button class="modal-close" aria-label="Close dialog" title="Close" onClick={close}>
      ×
    </button>
  );
}

/** A control dialog: as tall as the window allows, scrolling inside, with
 * its close button pinned where it can always be seen. Its name is its
 * `label`, else its heading. */
export function Modal({ children, close, label, class: cls }: { children: preact.ComponentChildren; close?: () => void; label?: string; class?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useModal(ref, close);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || label) return;
    const h = el.querySelector("h2");
    if (h) {
      if (!h.id) h.id = `modal-${Math.random().toString(36).slice(2, 10)}`;
      el.setAttribute("aria-labelledby", h.id);
    }
  });
  return (
    <div class="prompt-backdrop" onClick={(e) => e.target === e.currentTarget && close?.()}>
      <div ref={ref} class={cls ? `prompt control-prompt ${cls}` : "prompt control-prompt"} role="dialog" aria-modal="true" aria-label={label}>
        {close ? <CloseX close={close} /> : null}
        {children}
      </div>
    </div>
  );
}
