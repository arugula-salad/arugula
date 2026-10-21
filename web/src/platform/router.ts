// The platform page's router (M150): the route the page is at, kept
// current, and going somewhere without a reload. What the paths are is
// ./paths.

import { useEffect, useState } from "preact/hooks";
import { parse, type Route } from "./paths";

export * from "./paths";

const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
addEventListener("popstate", changed);

/** Hear every change of route (the page's own and Back's). */
export function onRoute(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** The route the page is at. */
export const current = (): Route => parse(location.pathname, location.search);

/** Go to `path`: a new history entry, or this one replaced (typing in
 * search, or putting the slug right). */
export function go(path: string, replace = false) {
  if (path === location.pathname + location.search) return changed();
  if (replace) history.replaceState(null, "", path);
  else history.pushState(null, "", path);
  changed();
}

/** The route, kept current. */
export function useRoute(): Route {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((n) => n + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  return current();
}

/** Follow a link inside the page without reloading it; a modified click
 * (a new tab, a download) is the browser's. */
export function follow(e: MouseEvent, path: string) {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  e.preventDefault();
  go(path);
}
