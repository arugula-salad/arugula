// What Boards, Docs and the Inbox are scoped to (Arugula #881): the current
// project's (./current-project), unless the address says `?all=1` (./paths).
// Without a current project, or one that isn't the reader's, they list
// everything, as they always did.

import type { Route } from "./paths.ts";

/** The pages that open outside a project, and so are scoped to the current one. */
export function scopable(r: Route): boolean {
  return r.page === "inbox" || r.page === "all-boards" || (r.page === "docs" && !r.project);
}

/** The project `r` is scoped to: the current one, when it's the reader's
 * (`known`) and the address doesn't ask for All. */
export function scopeOf(r: Route, current: string | null, known: (id: string) => boolean): string | undefined {
  if ("all" in r && r.all) return undefined;
  return current && known(current) ? current : undefined;
}

/** The inbox's address for a scope: `/inbox?project=<id>` as the platform reads it, or all of it. */
export function inboxPath(project: string | undefined): string {
  return project ? `/api/inbox?project=${encodeURIComponent(project)}` : "/api/inbox";
}
