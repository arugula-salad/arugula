// The platform page's router (M150): the route the page is at, kept
// current, and going somewhere without a reload. What the paths are is
// ./paths.

import { useEffect, useState } from "preact/hooks";
import { currentProject, forgetCurrentProject, noteRoute, setCurrentProject } from "./current-project";
import { parse, type Route } from "./paths";

export * from "./paths";

/** The route the page is at. */
export const current = (): Route => parse(location.pathname, location.search);

const LAST = "arugula.platform-last-page";

/** Where this browser last was on this page, for the shell's Projects to
 * come back to (Arugula #638). Its path, never a fragment: secrets stay in
 * those. */
function remember(popped = false) {
  const route = current();
  const was = currentProject();
  // Any page under a project makes it the current one; the picker goes through here too.
  const project = "project" in route && route.project ? route.project : undefined;
  if (project) setCurrentProject(project);
  const chat = route.page === "chat" || route.page === "dm";
  noteRoute(was, { project, chat, room: chat && !!route.room }, popped);
  // Chat and account settings are the rail's own places, not the projects';
  // the design reference is no place to come back to.
  const { page } = route;
  if (page === "chat" || page === "dm" || page === "settings" || page === "owner-settings" || page === "design") return;
  // A shell button's hand-off passes through; where it lands is the place.
  if (route.page === "home" && route.to) return;
  try {
    localStorage.setItem(LAST, location.pathname + location.search);
  } catch {
    // without storage, Projects lands on the list
  }
}

/** Forget it: this page's sign-out, so the next person isn't sent there. */
export function forgetLastPage() {
  try {
    localStorage.removeItem(LAST);
  } catch {
    // nothing kept, then
  }
  forgetCurrentProject();
}

// `/?back` is the shell's Projects: on to where this browser last was, or the
// list if nowhere. Before anything reads the route.
if (location.pathname === "/" && new URLSearchParams(location.search).has("back")) {
  let last: string | null = null;
  try {
    last = localStorage.getItem(LAST);
  } catch {
    // the list, then
  }
  // A path on this origin, and nothing else ("//host" is another origin).
  history.replaceState(null, "", `${last?.startsWith("/") && !last.startsWith("//") ? last : "/"}${location.hash}`);
}
remember();

const listeners = new Set<() => void>();
// The route this page was at before its last step forward, while Back hasn't
// moved it since (then it isn't known): the entry under this one is ours.
let before: Route | null = null;
/** The page this one was opened from, if it's one of ours. */
export const cameFrom = (): Route | null => before;

const changed = (popped = false) => {
  here = location.pathname + location.search;
  remember(popped);
  listeners.forEach((fn) => fn());
};
/** Where the page is now, to come back to when leaving is refused. */
let here = location.pathname + location.search;

addEventListener("popstate", () => {
  // Back, with changes not saved and kept: the address goes back to them.
  if (holding && !holding()) {
    history.pushState(null, "", here);
    return;
  }
  before = null;
  changed(true);
});

/** What a page with changes not saved asks before it's left (Arugula
 * #897): whether to go. */
let holding: (() => boolean) | null = null;

/** While `dirty`, leaving the page asks first: a link, Back, closing the
 * tab or reloading it. */
export function useHoldLeaving(dirty: boolean, ask = "Leave without saving your changes?") {
  useEffect(() => {
    if (!dirty) return;
    const mine = () => confirm(ask);
    holding = mine;
    const unload = (e: BeforeUnloadEvent) => e.preventDefault();
    addEventListener("beforeunload", unload);
    return () => {
      if (holding === mine) holding = null;
      removeEventListener("beforeunload", unload);
    };
  }, [dirty, ask]);
}

/** Hear every change of route (the page's own and Back's). */
export function onRoute(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** Go to `path`: a new history entry, or this one replaced (typing in
 * search, or putting the slug right). `state` marks the entry (./chat-nav). */
export function go(path: string, replace = false, state: unknown = null) {
  if (path === location.pathname + location.search) return changed();
  // A slug put right stays on the page; anywhere else asks first.
  if (!replace && holding && !holding()) return;
  if (replace) history.replaceState(state, "", path);
  else {
    before = current();
    history.pushState(state, "", path);
  }
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

/** A whole table row as a link (#373): its own click opens `path`, as the
 * link inside it does. Not a click on that link or a button (they handle
 * themselves), nor one that ends a text selection. */
export function followRow(e: MouseEvent, path: string) {
  if ((e.target as Element).closest("a, button") || getSelection()?.toString()) return;
  follow(e, path);
}
