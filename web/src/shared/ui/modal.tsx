// A dialog on control's page: the trust screens' (web/src/shell/trust/)
// and the platform's settings (web/src/platform/settings/) alike, so both
// sides draw it the same (M149).
//
// UI audit #150: every dialog behaves the same way. Focus moves into it and
// stays there (Tab wraps), Escape closes it before the terminal under it can
// hear the key, and focus goes back to what opened it. Dialogs drawn by hand
// (Permission rules, Appearance, Getting started…) use `useModal` for this.

import { useLayoutEffect, useRef } from "preact/hooks";
import { escapeTaken } from "./menu";

interface Open {
  el: () => HTMLElement | null;
  close: () => (() => void) | undefined;
}

/** The dialogs up, latest (the one on top) last. */
const stack: Open[] = [];

/** The dialog on top that's drawn. */
function top(): Open | undefined {
  for (let i = stack.length - 1; i >= 0; i--) if (stack[i].el()?.isConnected) return stack[i];
  return undefined;
}

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
  const up = top();
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
  const root = top()?.el();
  const t = e.target as HTMLElement | null;
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
 * is no `close`), and the focus goes back where it was when it shuts. */
export function useModal(ref: { current: HTMLElement | null }, close: (() => void) | undefined, open = true) {
  const latest = useRef(close);
  latest.current = close;
  useLayoutEffect(() => {
    if (!open) return;
    install();
    // A menu's item gives the focus back to what opened the menu before it
    // runs (menu.tsx), so this is the account button, a pane, the palette's.
    const opener = document.activeElement as HTMLElement | null;
    const me: Open = { el: () => ref.current, close: () => latest.current };
    stack.push(me);
    const root = ref.current;
    if (root) {
      if (!root.hasAttribute("tabindex")) root.tabIndex = -1;
      focusInto(root);
    }
    return () => {
      const i = stack.indexOf(me);
      if (i !== -1) stack.splice(i, 1);
      // Only when the focus was ours: a dialog shut because another opened
      // leaves the new one's focus be.
      const at = document.activeElement;
      const ours = !at || at === document.body || !at.isConnected || (root?.contains(at) ?? false);
      const under = top()?.el();
      if (!ours) return;
      if (under && !under.contains(opener)) focusInto(under);
      else if (opener?.isConnected && opener !== document.body) opener.focus({ preventScroll: true });
    };
  }, [open]);
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
        {close ? (
          <button class="modal-close" aria-label="Close dialog" title="Close" onClick={close}>
            ×
          </button>
        ) : null}
        {children}
      </div>
    </div>
  );
}
