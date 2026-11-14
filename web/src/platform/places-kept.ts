// Where you were, per project (Arugula #989, decisions 4 and 5): kept in
// your account's prefs (shared/seen.ts: `/api/me/prefs` on the hub, else
// this browser's), so the phone and the computer agree.
//
// - `app.project`: the project you were last in.
// - `app.place.<project>`: the page you were last at in it.
// - `app.pane.<project>`: the machine and pane you last had open in it.
//
// Opening the app goes back to the last project's place; the switcher opens
// a project at its place; Machines on the rail opens your last pane. Chat,
// account settings and the design reference are no project's place, as
// the router's own memory has it (./router.ts).

import { pref, setPref } from "../shared/seen.ts";
import { idOf, type Route } from "./paths.ts";

export const PROJECT_KEY = "app.project";
export const placeKey = (project: string) => `app.place.${project}`;
export const paneKey = (project: string) => `app.pane.${project}`;

/** The pages that are no project's place to come back to. */
const NOT_A_PLACE = new Set<Route["page"]>(["home", "find", "missing", "chat", "dm", "settings", "owner-settings", "design", "inbox", "all-boards", "installed", "deploy", "team"]);

/** The project `r` is a place of, or null when it's none to come back to. */
export function placeIn(r: Route): string | null {
  if (NOT_A_PLACE.has(r.page) || !("project" in r) || !r.project) return null;
  return r.project;
}

/** A kept path, if it's still a page of `project`'s: one of this origin's
 * paths (never `//host`), under `/p/<project>`. */
export function keptPath(v: unknown, project: string): string | null {
  if (typeof v !== "string" || !v.startsWith("/p/") || v.startsWith("//") || v.length > 400) return null;
  const segment = v.slice(3).split(/[/?#]/)[0];
  return idOf(segment) === project ? v : null;
}

/** Writes wait this long, so a run of steps is one save. */
export const SETTLE_MS = 5000;
const pending = new Map<string, string>();
let timer: ReturnType<typeof setTimeout> | undefined;
/** A pref as it is now: what waits to be saved, else what's saved. */
const now = (k: string) => pending.get(k) ?? pref(k);

/** Where you were in `project`. */
export const placeOf = (project: string) => keptPath(now(placeKey(project)), project);
/** The pane you last had open in `project`, as its machine page's path. */
export const paneOf = (project: string) => keptPath(now(paneKey(project)), project);
/** The project you were last in. */
export function lastProject(): string | null {
  const v = now(PROJECT_KEY);
  return typeof v === "string" && idOf(v) === v ? v : null;
}

/** Save what waits now: leaving the page does. */
export function savePlaces() {
  clearTimeout(timer);
  timer = undefined;
  for (const [k, v] of pending) if (pref(k) !== v) setPref(k, v);
  pending.clear();
}

function keep(k: string, v: string) {
  pending.set(k, v);
  timer ??= setTimeout(savePlaces, SETTLE_MS);
}

/** You're at `path` (route `r`): keep it as your place in its project. */
export function notePlace(r: Route, path: string) {
  const project = placeIn(r);
  if (!project || !keptPath(path, project)) return;
  keep(PROJECT_KEY, project);
  keep(placeKey(project), path);
}

/** A machine's page shows a pane (`path`) in `project`. */
export function notePane(project: string, path: string) {
  if (!keptPath(path, project)) return;
  keep(paneKey(project), path);
}

// Leaving the page saves what's waiting.
if (typeof addEventListener === "function") addEventListener("pagehide", savePlaces);
