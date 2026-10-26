// A message list that stays at its newest while what's in it grows (UI
// audit #156): link previews, pictures and agents' cards load after the
// list has scrolled to the bottom, and left it short of the newest with
// "Jump to newest" up. While the list is at the bottom (`pinned()`), any
// change in its rows' sizes, or its own, scrolls it back there before
// the frame is drawn.

import { useEffect } from "preact/hooks";

export function useStickToBottom(list: { current: HTMLElement | null }, pinned: () => boolean) {
  useEffect(() => {
    const el = list.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    let h = el.scrollHeight;
    const ro = new ResizeObserver(() => {
      const now = el.scrollHeight;
      if (now !== h && pinned()) el.scrollTop = now;
      h = now;
    });
    const watch = (n: Node) => n instanceof Element && ro.observe(n);
    ro.observe(el);
    el.childNodes.forEach(watch);
    // Rows come and go: watch each one that's there.
    const mo = new MutationObserver((changes) => {
      for (const c of changes) {
        c.addedNodes.forEach(watch);
        c.removedNodes.forEach((n) => n instanceof Element && ro.unobserve(n));
      }
    });
    mo.observe(el, { childList: true });
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  }, []);
}
