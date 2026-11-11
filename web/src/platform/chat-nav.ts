// Back goes up one level in chat (Arugula #817): thread → room → list → out.
// The URL is the one truth: the level is read off the route (`chatLevel`),
// never kept beside it, so Back and the swipe (#816) render what the address
// says. History is at most list, room, thread deep: down a level pushes, a
// step sideways replaces.

import { arrival, leaveChat } from "./current-project";
import { names } from "./links";
import { cameFrom, current, go, onRoute, parse, pathOf, type Route } from "./router";
import { session } from "./session";

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
// Each chat entry's `history.state` says its level and which entry sits
// directly under it (`below`), so going up can pop it when that is the target
// (one Back press from the list leaves chat) and replace when it isn't.
// `below` is a route's ids, not its path: slugs change as names load.
type Mark = { chat: ChatLevel | null; below?: string };
const keyOf = (r: Route) => (r.page === "chat" || r.page === "dm" ? `${r.project}/${r.room ?? ""}/${r.thread ?? ""}` : "");
const mark = (r: Route, below?: string): Mark => (below ? { chat: chatLevel(r), below } : { chat: chatLevel(r) });
const stateNow = () => history.state as Partial<Mark> | null;
const marked = () => stateNow()?.chat;

/** Open `path` inside chat, by the levels it moves between: down pushes, a
 * step sideways replaces, and up pops the entry under this one when that is
 * `path`, else replaces. */
export function chatGo(path: string) {
  const to = routeOf(path);
  const from = chatLevel();
  const into = chatLevel(to);
  if (from === null || into === null) return go(path, false, mark(to));
  if (RANK[into] > RANK[from]) return go(path, false, mark(to, keyOf(current())));
  const up = chatUp(to);
  const below = stateNow()?.below;
  if (RANK[into] < RANK[from] && below && below === keyOf(to)) return history.back();
  // The entry under this one stays under it only while it is still the level above `to`.
  go(path, true, mark(to, up && below === keyOf(routeOf(up)) ? below : undefined));
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
  const list: Route = { page: "chat", project: to.project };
  history.replaceState(mark(list), "", pathOf(list, names));
  const room = pathOf({ ...to, thread: undefined, msg: level === "thread" ? undefined : to.msg }, names);
  if (level === "thread") history.pushState(mark({ ...to, thread: undefined }, keyOf(list)), "", room);
  // The hash stays on the page's own entry: a sign-in's #code is read off it
  // after this runs (signin.ts).
  history.pushState(mark(to, level === "thread" ? keyOf({ ...to, thread: undefined }) : keyOf(list)), "", path + hash);
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

/** Where leaving chat from `project` goes (Arugula #882): a project's home,
 * never `/`. `back`: the page before is ours and in this project, so it's a
 * step back in history instead of a new entry. */
export function chatOut(project: string): { path: string; back: boolean } {
  const from = cameFrom();
  const before = !!from && from.page !== "chat" && from.page !== "dm" && "project" in from && from.project === project;
  const out = leaveChat(project, arrival(), (id) => !!session.project(id), before);
  return { path: pathOf({ page: "project", project: out.project }, names), back: out.back };
}

/** Leave chat by its close button or Back's own: see `chatOut`. */
export function leaveChatTo(project: string) {
  const out = chatOut(project);
  if (out.back) history.back();
  else go(out.path);
}

/** Where a swipe right goes from here: one level up (what Back does), or from
 * the list out to the project's home. Null where chat isn't. */
export function swipeUp(r: Route = current()): string | null {
  const level = chatLevel(r);
  if (level === null || (r.page !== "chat" && r.page !== "dm")) return null;
  if (level === "list") return r.page === "chat" && r.view ? pathOf({ page: "chat", project: r.project }, names) : chatOut(r.project).path;
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

/** Commit a swipe's target: out of chat is `leaveChatTo`'s, the rest are chat's levels. */
export function swipeGo(path: string) {
  const r = current();
  if ((r.page === "chat" || r.page === "dm") && path === chatOut(r.project).path) return leaveChatTo(r.project);
  chatGo(path);
}
