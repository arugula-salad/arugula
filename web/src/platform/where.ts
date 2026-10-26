// Which project the bar is in (UI audit #159): the route's, when the route
// names one (a direct message's is its team's, under `/p/:project/dm/…`
// since UI audit #169); a deploy's, once its page has read it. Pages that
// are no project's (/, /inbox, /settings) are in none: the bar says "All
// projects".

import type { Route } from "./paths";

let deploy: { id: string; project: string } | null = null;
const listeners = new Set<() => void>();

export const where = {
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  },
  /** A deploy's page has read which project it's in. */
  deployIn(id: string, project: string) {
    if (deploy?.id === id && deploy.project === project) return;
    deploy = { id, project };
    listeners.forEach((fn) => fn());
  },
};

/** The project the bar shows for `r`, if any. */
export function barProject(r: Route): string | undefined {
  if ("project" in r && r.project) return r.project;
  if (r.page === "deploy") return deploy?.id === r.deploy ? deploy.project : undefined;
  return undefined;
}
