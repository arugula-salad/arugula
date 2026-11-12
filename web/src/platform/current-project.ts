// The project a person is in (Arugula #880): set from the URL whenever a
// project's page is open, and kept in this browser, so every button that opens
// a place (the rails', the bar's, the shell's) opens it in that project
// instead of dropping them out to the global one. The one reader of the
// key; ./router writes it, and the shell's buttons (another origin) hand
// their place to the platform page as `/?to=` (./paths), which asks this.

import { pathOf, type Names } from "./paths.ts";

// The key keeps its name from before: it was the rail's Projects alone.
const KEY = "arugula.platform-active-project";

const listeners = new Set<() => void>();
// Where storage is refused, this page still has one.
let kept: string | null = null;

/** The current project's id, if there is one: not yet checked against the
 * reader's projects (./session does, once it knows them). */
export function currentProject(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return kept;
  }
}

/** A project's page is open: it's the current one. */
export function setCurrentProject(id: string) {
  if (currentProject() === id) return;
  kept = id;
  try {
    localStorage.setItem(KEY, id);
  } catch {
    // without storage, it lasts as long as the page
  }
  listeners.forEach((fn) => fn());
}

/** Forget it: it's gone or no longer the reader's, or this page's sign-out. */
export function forgetCurrentProject() {
  if (currentProject() === null) return;
  kept = null;
  try {
    localStorage.removeItem(KEY);
  } catch {
    // nothing kept, then
  }
  listeners.forEach((fn) => fn());
}

/** Hear the current project change. */
export function onCurrentProject(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** The places a button can open in the current project. */
export const ENTRIES = ["projects", "docs", "boards", "chat"] as const;
export type Entry = (typeof ENTRIES)[number];

/** Where `entry` opens for `project` (the reader's, as the session knows it),
 * or the global page when there's none. Chat is its team's; Personal has no
 * team chat, so it keeps the chat page. */
export function entryPath(entry: Entry, project: { id: string; team?: unknown } | undefined, names?: Names): string {
  if (!project) return { projects: "/", docs: "/docs", boards: "/boards", chat: "/chat" }[entry];
  const id = project.id;
  switch (entry) {
    case "projects":
      return pathOf({ page: "project", project: id }, names);
    case "docs":
      return pathOf({ page: "docs", project: id }, names);
    case "boards":
      return pathOf({ page: "boards", project: id }, names);
    case "chat":
      return project.team ? pathOf({ page: "chat", project: id }, names) : "/chat";
  }
}

// Chat's own (Arugula #882): which team's chat opens with no project named, and
// how a link into another team's room is told and undone.

const LAST_TEAM = "arugula.platform-last-team-project";

/** The most recent current project that was a team's, kept in this browser. */
export function lastTeamProject(): string | null {
  try {
    return localStorage.getItem(LAST_TEAM);
  } catch {
    return keptTeam;
  }
}
let keptTeam: string | null = null;

/** Note the current project if it's a team's: Personal has no chat, so this is
 * the team chat opens for there. */
export function rememberTeamProject(projects: readonly { id: string; team?: unknown }[]) {
  const id = currentProject();
  if (!id || !projects.some((p) => p.id === id && p.team)) return;
  keptTeam = id;
  try {
    localStorage.setItem(LAST_TEAM, id);
  } catch {
    // without storage, it lasts as long as the page
  }
}

/** The project chat opens with none named: the current one if it's a team's;
 * else (Personal, or none) the last current team's, else the first team's.
 * Undefined with no team at all. */
export function chatProject(projects: readonly { id: string; team?: unknown }[], current: string | null, last: string | null): string | undefined {
  const team = (id: string | null) => projects.find((p) => p.id === id && p.team)?.id;
  return team(current) ?? team(last) ?? projects.find((p) => p.team)?.id;
}

/** A link opened a room of another project than the one they were in. */
export type Arrival = { from: string; to: string };
let arrived: Arrival | null = null;

/** The room link that switched project, until dismissed or left. */
export const arrival = (): Arrival | null => arrived;

/** The page is at `at` and the project was `was` before it. A room opened
 * from another project is an arrival; any other place than that project's
 * chat, or Back's own step (`popped`), ends it. */
export function noteRoute(was: string | null, at: { project?: string; chat: boolean; room: boolean }, popped: boolean) {
  const before = arrived;
  if (!popped && was && at.project && was !== at.project && at.room) {
    // A stale link's second hop (the room is in a third project) keeps the first origin.
    arrived = { from: before?.to === was ? before.from : was, to: at.project };
  } else if (!(at.chat && at.project && before?.to === at.project)) {
    arrived = null;
  }
  if (arrived !== before) listeners.forEach((fn) => fn());
}

/** The banner is dismissed, or its Back was taken. */
export function dismissArrival() {
  if (!arrived) return;
  arrived = null;
  listeners.forEach((fn) => fn());
}

/** Where leaving chat from `project` goes: the project the reader came from
 * after a link across teams (while it's still theirs); else this project's,
 * by history when the page before was ours and in it. Never the global list. */
export function leaveChat(project: string, now: Arrival | null, known: (id: string) => boolean, previousInProject: boolean): { project: string; back: boolean } {
  if (now && now.to === project && known(now.from)) return { project: now.from, back: false };
  return { project, back: previousInProject };
}
