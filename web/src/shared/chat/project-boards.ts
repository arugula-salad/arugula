// A project's boards, for Chat's Agents section, a board channel's About
// and the tags its composer and messages know (Arugula #1522): read once
// per project and page, shared by every part that asks, and read again
// when a project's Chat opens.

import { useEffect, useState } from "preact/hooks";
import type { ProjectBoard } from "../proto";
import { boards as boardStore } from "../boards/store";

const read = new Map<string, ProjectBoard[]>();
const asking = new Map<string, Promise<void>>();
const listeners = new Set<() => void>();

function ask(project: string): Promise<void> {
  let p = asking.get(project);
  if (!p) {
    p = boardStore.list(project).then(
      (l) => void read.set(project, l.boards),
      () => void (read.has(project) || read.set(project, [])),
    );
    p.finally(() => {
      asking.delete(project);
      listeners.forEach((fn) => fn());
    });
    asking.set(project, p);
  }
  return p;
}

/** Draw again whenever a project's boards are read. */
function useRedraw() {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((n) => n + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
}

/** The project's boards, null until they're read. `fresh`: read them again
 * (the Agents section, as Chat opens). */
export function useProjectBoards(project: string | undefined, fresh = false): ProjectBoard[] | null {
  useRedraw();
  useEffect(() => {
    if (project && (fresh || !read.has(project))) void ask(project);
  }, [project]);
  return project ? (read.get(project) ?? null) : null;
}

/** The boards of every project in `projects` (a room's `tag_projects`,
 * Arugula #1582), as each is read: what its `@` offers. */
export function useProjectsBoards(projects: readonly string[]): ProjectBoard[] {
  useRedraw();
  const key = projects.join(" ");
  useEffect(() => {
    for (const p of projects) if (!read.has(p)) void ask(p);
  }, [key]);
  return projects.flatMap((p) => read.get(p) ?? []);
}

/** Every board read so far, in any project: whose lead an agent in chat
 * may be, for its name (../agent-name.ts `chatAgentName`). Redraws as more
 * are read. */
export function useLedBoards(): ProjectBoard[] {
  useRedraw();
  return [...read.values()].flat();
}

/** A board's title, from those read: `project`'s, else any project's (a
 * team channel tags its team's projects' boards, Arugula #1582). */
export const boardTitle = (project: string | null | undefined, board: string): string | undefined =>
  (project ? read.get(project) : undefined)?.find((b) => b.id === board)?.title ?? [...read.values()].flat().find((b) => b.id === board)?.title;
