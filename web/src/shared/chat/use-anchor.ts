// One scroll model for a message list (#965), shared by the room and the
// thread panel. The view is either at the bottom, or anchored to a message:
// the first one whose bottom is below the list's top edge, and where its top
// sits from that edge. Every change of layout (the list or a row resizing,
// rows coming or going, the keyboard, which resizes the list) goes
// through `restore`: at the bottom, scroll to it; else put the anchor message
// back where it was. Nothing else writes `scrollTop` but what the person
// asks for (a linked message, "Jump to newest"). The browser's own scroll
// anchoring is off on the list (style.css): two things correcting is the
// jump this replaces.
//
// Where it was is kept per device, per room (or thread), so it opens there.

import { useEffect, useLayoutEffect, useRef } from "preact/hooks";

/** How near the bottom still counts as at it, for following new messages. */
export const NEAR_BOTTOM = 80;
/** How long a scroll waits before it's written to local storage. */
const SAVE_AFTER = 250;

export type Anchor = { bottom: true } | { seq: number; offset: number };

const AT_BOTTOM: Anchor = { bottom: true };

/** What was kept for `key`; nothing, or anything unreadable, is null. */
export function savedAnchor(key: string): Anchor | null {
  try {
    const v = JSON.parse(localStorage.getItem(key) ?? "null");
    if (v?.bottom === true) return AT_BOTTOM;
    if (Number.isInteger(v?.seq) && Number.isFinite(v?.offset)) return { seq: v.seq, offset: v.offset };
  } catch {}
  return null;
}

function keep(key: string, a: Anchor) {
  try {
    localStorage.setItem(key, JSON.stringify(a));
  } catch {}
}

export interface AnchorView {
  /** Whether the view is at the bottom. */
  bottom(): boolean;
  /** Whether the list has been placed (opened) yet: until then it isn't moved. */
  ready(): boolean;
  /** Open at `a` (null: the bottom), and keep it from now on. */
  open(a: Anchor | null): void;
  /** Go to the bottom and stay there (my own message, "Jump to newest"). */
  toBottom(): void;
  /** Take where the view is now as what to keep (after a scroll the person did). */
  capture(): void;
  /** Put the view where the model says. */
  restore(): void;
  /** The scroll handler for the list. */
  onScroll(): void;
}

/** `key` is the room's (or thread's) storage key; `onBottom` hears when the
 * view gets to or leaves the bottom. */
export function useAnchor(list: { current: HTMLElement | null }, key: string, onBottom: (bottom: boolean) => void): AnchorView {
  const s = useRef({
    mode: AT_BOTTOM as Anchor,
    ready: false,
    timer: undefined as ReturnType<typeof setTimeout> | undefined,
    key,
    onBottom,
    view: undefined as AnchorView | undefined,
  }).current;
  s.onBottom = onBottom;

  const save = () => {
    clearTimeout(s.timer);
    s.timer = undefined;
    if (s.ready) keep(s.key, s.mode);
  };
  const set = (mode: Anchor) => {
    const was = "bottom" in s.mode;
    s.mode = mode;
    if (was !== "bottom" in mode) s.onBottom("bottom" in mode);
  };
  const rows = (el: HTMLElement) => el.querySelectorAll<HTMLElement>("[data-msg]");

  s.view ??= {
    bottom: () => "bottom" in s.mode,
    ready: () => s.ready,
    open(a) {
      s.ready = true;
      set(a ?? AT_BOTTOM);
      s.view!.restore();
      // Placed by what was kept: what's there now is the truth to keep.
      s.view!.capture();
    },
    toBottom() {
      s.ready = true;
      set(AT_BOTTOM);
      s.view!.restore();
    },
    capture() {
      const el = list.current;
      if (!el || !s.ready) return;
      if (el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM) return set(AT_BOTTOM);
      // The first row whose bottom is below the top edge (rows are in order).
      const all = rows(el);
      const edge = el.getBoundingClientRect().top;
      let lo = 0;
      let hi = all.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (all[mid].getBoundingClientRect().bottom > edge) hi = mid;
        else lo = mid + 1;
      }
      const at = all[lo];
      const seq = Number(at?.dataset.msg);
      if (at && Number.isInteger(seq)) set({ seq, offset: at.getBoundingClientRect().top - edge });
    },
    restore() {
      const el = list.current;
      if (!el || !s.ready) return;
      if ("bottom" in s.mode) {
        el.scrollTop = el.scrollHeight;
      } else {
        const at = el.querySelector<HTMLElement>(`[data-msg="${s.mode.seq}"]`);
        // Not there (yet, or gone): leave the view be.
        if (at) el.scrollTop += at.getBoundingClientRect().top - el.getBoundingClientRect().top - s.mode.offset;
      }
    },
    onScroll() {
      const el = list.current;
      if (!el || !s.ready) return;
      s.view!.capture();
      clearTimeout(s.timer);
      s.timer = setTimeout(save, SAVE_AFTER);
    },
  };
  const view = s.view;

  // A new room (or thread): not placed until the caller says where.
  useLayoutEffect(() => {
    s.key = key;
    s.ready = false;
    s.mode = AT_BOTTOM;
    onBottom(true);
    return save;
  }, [key]);

  useEffect(() => {
    const el = list.current;
    if (!el) return;
    const again = () => view.restore();
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(again);
    const watch = (n: Node) => n instanceof Element && ro?.observe(n, { box: "border-box" });
    ro?.observe(el, { box: "border-box" });
    el.childNodes.forEach(watch);
    // Rows come and go (older ones above, new ones below): watch each that's there;
    // a new one is told of its size once, which puts the view back.
    const mo = new MutationObserver((changes) => {
      for (const c of changes) {
        c.addedNodes.forEach(watch);
        c.removedNodes.forEach((n) => n instanceof Element && ro?.unobserve(n));
      }
    });
    mo.observe(el, { childList: true });
    // Closing the app or the tab: what's kept is the last scroll.
    const leave = () => document.visibilityState === "hidden" && save();
    document.addEventListener("visibilitychange", leave);
    addEventListener("pagehide", save);
    return () => {
      ro?.disconnect();
      mo.disconnect();
      document.removeEventListener("visibilitychange", leave);
      removeEventListener("pagehide", save);
    };
  }, []);

  return view;
}
