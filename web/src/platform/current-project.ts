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
