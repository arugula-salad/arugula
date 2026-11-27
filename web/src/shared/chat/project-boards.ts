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

/** The project's boards, null until they're read. `fresh`: read them again
 * (the Agents section, as Chat opens). */
export function useProjectBoards(project: string | undefined, fresh = false): ProjectBoard[] | null {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((n) => n + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  useEffect(() => {
    if (project && (fresh || !read.has(project))) void ask(project);
  }, [project]);
  return project ? (read.get(project) ?? null) : null;
}

/** A board's title, from those read. */
export const boardTitle = (project: string | null | undefined, board: string): string | undefined => (project ? read.get(project)?.find((b) => b.id === board)?.title : undefined);
