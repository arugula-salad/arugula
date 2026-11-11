// How a person arranges the chat sidebar (#120): starred rooms, sections of
// their own, what's collapsed, how each group is sorted. The platform keeps
// one document per account (`ChatSidebar`); these are the plain functions
// that change a copy of it and put rooms in order, for ./rooms and the
// store, and their tests (scripts/chat-sidebar.test.mjs).
//
// A room is in one place at most: starred, or in a section, or neither
// (where its kind puts it). Every change here keeps that, as the platform
// does on the way in.

import type { ChatRoom, ChatSection, ChatSidebar, ChatSort } from "../proto";

/** The key of the Starred group, in `collapsed` and `sorts`. */
export const STARRED = "starred";

export const emptySidebar = (): ChatSidebar => ({ starred: [], sections: [], collapsed: [], sorts: {} });

/** Where `room` is placed: `starred`, a section's id, or null. */
export function placeOf(s: ChatSidebar, room: string): string | null {
  if (s.starred.includes(room)) return STARRED;
  return s.sections.find((x) => x.rooms.includes(room))?.id ?? null;
}

function without(s: ChatSidebar, room: string): ChatSidebar {
  return {
    ...s,
    starred: s.starred.filter((r) => r !== room),
    sections: s.sections.map((x) => (x.rooms.includes(room) ? { ...x, rooms: x.rooms.filter((r) => r !== room) } : x)),
  };
}

/** Put `room` in `to`: `starred`, a section's id, or null for where its
 * kind puts it. */
export function place(s: ChatSidebar, room: string, to: string | null): ChatSidebar {
  const out = without(s, room);
  if (to === null) return out;
  if (to === STARRED) return { ...out, starred: [...out.starred, room] };
  if (!out.sections.some((x) => x.id === to)) return s;
  return { ...out, sections: out.sections.map((x) => (x.id === to ? { ...x, rooms: [...x.rooms, room] } : x)) };
}

/** Star or unstar. Unstarring a room puts it back where its kind does. */
export const star = (s: ChatSidebar, room: string, on: boolean): ChatSidebar =>
  on ? place(s, room, STARRED) : placeOf(s, room) === STARRED ? place(s, room, null) : s;

export function addSection(s: ChatSidebar, name: string, id = `s_${Math.random().toString(16).slice(2, 10)}${Date.now().toString(16)}`): ChatSidebar {
  return { ...s, sections: [...s.sections, { id, name: name.trim(), rooms: [] }] };
}

export const renameSection = (s: ChatSidebar, id: string, name: string): ChatSidebar => ({
  ...s,
  sections: s.sections.map((x) => (x.id === id ? { ...x, name: name.trim() } : x)),
});

/** Its rooms go back where their kinds put them. */
export const removeSection = (s: ChatSidebar, id: string): ChatSidebar => ({
  ...s,
  sections: s.sections.filter((x) => x.id !== id),
  collapsed: s.collapsed.filter((k) => k !== id),
  sorts: Object.fromEntries(Object.entries(s.sorts).filter(([k]) => k !== id)),
});

export const collapse = (s: ChatSidebar, key: string, shut: boolean): ChatSidebar => ({
  ...s,
  collapsed: shut ? (s.collapsed.includes(key) ? s.collapsed : [...s.collapsed, key]) : s.collapsed.filter((k) => k !== key),
});

/** Name is the default, so it is not kept. */
export function setSort(s: ChatSidebar, key: string, sort: ChatSort): ChatSidebar {
  const { [key]: _, ...rest } = s.sorts;
  return { ...s, sorts: sort === "name" ? rest : { ...rest, [key]: sort } };
}

/** The sort a group has: its own, else `fallback` (DMs and threads default
 * to newest, channels to name). */
export const sortOf = (s: ChatSidebar, key: string, fallback: ChatSort = "name"): ChatSort => s.sorts[key] ?? fallback;

/** `rooms` in `sort` order. Archived channels go last by name, as the
 * channel list always had them. */
export function ordered(rooms: ChatRoom[], sort: ChatSort, label: (r: ChatRoom) => string): ChatRoom[] {
  const name = (a: ChatRoom, b: ChatRoom) => label(a).localeCompare(label(b));
  const cmp = sort === "recent" ? (a: ChatRoom, b: ChatRoom) => b.last - a.last || name(a, b) : name;
  return [...rooms].sort((a, b) => Number(a.archived) - Number(b.archived) || cmp(a, b));
}

/** The rooms of `ids` that `rooms` has, by id (those the reader no longer
 * sees are skipped, and come back in place if they return). */
export function present(ids: string[], rooms: ChatRoom[]): ChatRoom[] {
  const by = new Map(rooms.map((r) => [r.id, r]));
  return ids.flatMap((id) => by.get(id) ?? []);
}

export const sectionRooms = (x: ChatSection, rooms: ChatRoom[]): ChatRoom[] => present(x.rooms, rooms);

/** Every placed room's id, for taking them out of the built-in groups. */
export function placed(s: ChatSidebar): Set<string> {
  return new Set([...s.starred, ...s.sections.flatMap((x) => x.rooms)]);
}

/** Which rooms a page of one project lists (UI audit #169): a team's
 * project its team's channels and DMs, the reader's own (`team: null`)
 * the rooms of no team, their machines' threads and archives. `name` heads
 * the sidebar. A page with no scope (the shell's) lists every room. */
export interface ChatScope {
  team: string | null;
  name: string;
}

/** Whether `r` is one `scope` lists; every room is, with none. */
export const inScope = (r: ChatRoom, scope?: ChatScope): boolean => !scope || (scope.team ? r.team === scope.team : !r.team);

/** Whether a view that reads across rooms (Activity, Saved, Threads) takes
 * the room `r` (Arugula #883): the scope's rooms, and no DMs, which aren't a
 * team's (the sidebar still lists them). A room the store doesn't know isn't
 * in a scope. With no scope, every room. */
export const inView = (r: ChatRoom | undefined, scope?: ChatScope): boolean => !scope || (!!r && r.kind !== "dm" && inScope(r, scope));

/** The team a search is limited to: the scope's, unless it's every team's
 * (`all`). Personal has no team to name: it searches as before. */
export const searchTeam = (scope: ChatScope | undefined, all: boolean | undefined): string | undefined => (all ? undefined : (scope?.team ?? undefined));
