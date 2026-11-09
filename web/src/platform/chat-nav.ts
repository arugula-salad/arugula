// Back goes up one level in chat (Arugula #817): thread → room → list → out.
// The URL is the one truth: the level is read off the route (`chatLevel`),
// never kept beside it, so Back and the swipe (#816) render what the address
// says. History is at most list, room, thread deep: down a level pushes, a
// step sideways replaces.

import { names } from "./links";
import { current, go, onRoute, parse, pathOf, type Route } from "./router";

export type ChatLevel = "list" | "room" | "thread";

const RANK: Record<ChatLevel, number> = { list: 0, room: 1, thread: 2 };

/** The level a route is at; null when it isn't in chat. */
export function chatLevel(r: Route = current()): ChatLevel | null {
  if (r.page !== "chat" && r.page !== "dm") return null;
  if (!r.room) return "list";
  return r.thread !== undefined ? "thread" : "room";
}

/** The path one level up, or null from the list (Back's own: out of chat). */
export function chatUp(r: Route = current()): string | null {
  if ((r.page !== "chat" && r.page !== "dm") || !r.room) return null;
  const level = chatLevel(r);
  if (level === "thread") return pathOf({ ...r, thread: undefined, msg: undefined }, names);
  return pathOf({ page: "chat", project: r.project }, names);
}

const routeOf = (path: string) => {
  const u = new URL(path, location.origin);
  return parse(u.pathname, u.search);
};
const mark = (r: Route) => ({ chat: chatLevel(r) });
const marked = () => (history.state as { chat?: ChatLevel } | null)?.chat;

/** Open `path` inside chat, by the levels it moves between: down pushes,
 * sideways replaces, and up (a thread closing to its room) pops the entry
 * the thread pushed. */
export function chatGo(path: string) {
  const to = routeOf(path);
  const from = chatLevel();
  const into = chatLevel(to);
  const state = mark(to);
  if (from === null || into === null || RANK[into] > RANK[from]) return go(path, false, state);
  if (RANK[into] < RANK[from]) {
    const here = current();
    const sameRoom = "room" in here && "room" in to && here.room === to.room;
    // Back to the room the thread was pushed from; another room replaces.
    if (sameRoom && marked() === "thread" && chatUp(here) === path) return history.back();
  }
  go(path, true, state);
}

/** Arrive at `path` from outside (a notification, a pasted link, the page
 * load): a room or thread with no entries under it gets the list and the
 * room inserted, so Back climbs. An entry already marked is left alone, so
 * a reload doesn't stack them again. */
export function arrive(path: string, hash = "") {
  const to = routeOf(path);
  const level = chatLevel(to);
  if (level === null || level === "list" || marked()) return go(path, true, level === null ? null : mark(to));
  if (to.page !== "chat" && to.page !== "dm") return;
  history.replaceState(mark({ page: "chat", project: to.project }), "", pathOf({ page: "chat", project: to.project }, names));
  const room = pathOf({ ...to, thread: undefined, msg: level === "thread" ? undefined : to.msg }, names);
  if (level === "thread") history.pushState(mark({ ...to, thread: undefined }), "", room);
  // The hash stays on the page's own entry: a sign-in's #code is read off it
  // after this runs (signin.ts).
  history.pushState(mark(to), "", path + hash);
  go(path); // renders; same path, so no new entry
}

/** The page's first route, if it's a room or thread arrived at cold. */
export function arriveHere() {
  const level = chatLevel();
  if (level && level !== "list" && !marked()) arrive(location.pathname + location.search, location.hash);
}

// Swipe left (#816) goes back down: into the room last open in a project's
// list, or the thread last open in a room. Set
// whenever the route (the page's own, Back's, a cold load) is at a room or a
// thread. Keyed by ids, not paths: a path's slugs change as names load.
const lastChild = new Map<string, string>();
const parentKey = (project: string, room?: string) => (room ? `${project}/${room}` : project);

function note(r: Route = current()) {
  const level = chatLevel(r);
  if ((r.page === "chat" || r.page === "dm") && (level === "room" || level === "thread")) {
    lastChild.set(parentKey(r.project, level === "thread" ? r.room : undefined), pathOf({ ...r, msg: level === "room" ? undefined : r.msg }, names));
  }
}
note();
onRoute(() => note());

/** Where a swipe right goes from here: one level up (what Back does), or from
 * the list out to the projects at `/`. Null where chat isn't. */
export function swipeUp(r: Route = current()): string | null {
  const level = chatLevel(r);
  if (level === null) return null;
  if (level === "list") return r.page === "chat" && r.view ? pathOf({ page: "chat", project: r.project }, names) : "/";
  return chatUp(r);
}

/** Where a swipe left goes: the room or thread last open one level down, if any. */
export function swipeDown(r: Route = current()): string | null {
  if (r.page !== "chat" && r.page !== "dm") return null;
  const level = chatLevel(r);
  if (level === "list") return r.page === "chat" && !r.view ? lastChild.get(parentKey(r.project)) ?? null : null;
  if (level === "room") return lastChild.get(parentKey(r.project, r.room)) ?? null;
  return null;
}

/** Commit a swipe's target: `/` is a plain push, the rest are chat's levels. */
export function swipeGo(path: string) {
  if (path === "/") return go("/");
  chatGo(path);
}
