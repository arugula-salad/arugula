// The project the machine view shows (Arugula #1164): the app's machine
// page is in a project, and its sessions, tabs and panes, Panes, Swarm and
// Machines show only that project's. A session of another project isn't
// there at all. The daemon's own page names none, and shows everything.
// A session placed in no project is its machine owner's Default (#1161,
// decision 2): yours, on your own machine, in your Default.

import type { ProjectRef } from "../shared/proto.gen";

export interface ShownProject {
  id: string;
  name: string;
  /** It's the reader's Default, where a session placed nowhere shows. */
  personal: boolean;
  /** Go to its Machines: where a machine with nothing of it sends you. */
  machines?: () => void;
}

let shown: ShownProject | null = null;
const listeners = new Set<() => void>();

/** The view shows `project` from now on, or everything (`null`). */
export function showProject(project: ShownProject | null) {
  const same = shown?.id === project?.id && shown?.name === project?.name && shown?.personal === project?.personal;
  shown = project;
  if (!same) listeners.forEach((fn) => fn());
}

/** The project shown, or null for everything. */
export function shownProject(): ShownProject | null {
  return shown;
}

export function onShownProject(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** Whether a session in `project` shows, on a machine that's `yours` or not. */
export function inShown(project: ProjectRef | null | undefined, yours: boolean, at: ShownProject | null = shown): boolean {
  if (!at) return true;
  if (project) return project.id === at.id;
  return yours && at.personal;
}

/** The project a session in `project` shows in: its own, else (placed
 * nowhere, on your machine) your Default; null when it's none of yours. */
export function projectOf(project: ProjectRef | null | undefined, yours: boolean, personal: string | undefined): string | null {
  return project?.id ?? (yours ? (personal ?? null) : null);
}

/** Where the last session shown in each project is kept, per viewer and
 * machine: `arugula.last-session:<viewer>:<machine>`, a map of project to
 * session id. In this browser only; without storage, none is kept. */
const lastKey = (viewer: string, machine: string) => `arugula.last-session:${viewer}:${machine}`;

function lastMap(key: string): Record<string, number> {
  try {
    const v = JSON.parse(localStorage.getItem(key) ?? "{}") as unknown;
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, number>) : {};
  } catch {
    return {};
  }
}

/** The session `viewer` last had shown in `project` on `machine`, if kept. */
export function lastSession(viewer: string, machine: string, project: string): number | null {
  const id = lastMap(lastKey(viewer, machine))[project];
  return typeof id === "number" ? id : null;
}

/** Keep `session` as the one `viewer` last had shown in `project` on `machine`. */
export function keepLastSession(viewer: string, machine: string, project: string, session: number) {
  const key = lastKey(viewer, machine);
  const map = lastMap(key);
  if (map[project] === session) return;
  map[project] = session;
  try {
    localStorage.setItem(key, JSON.stringify(map));
  } catch {
    // No storage: switching back lands on the project's first session.
  }
}
