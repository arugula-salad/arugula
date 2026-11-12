// The "All projects" toggle on Boards, Docs and the Inbox (Arugula #881):
// they open in the current project, and this widens them to every project.
// It's the address (`?all=1`), so it survives a reload and can be linked. It
// shows only where there's a project to widen from.

import type { Route } from "./paths";
import { follow, pathOf } from "./router";

export function ScopeToggle({ r, scoped, all }: { r: Route & { page: "inbox" | "all-boards" | "docs" }; scoped: boolean; all: boolean }) {
  // Neither scoped nor widened: no current project, nothing to toggle.
  if (!scoped && !all) return null;
  const to = pathOf(all ? { ...r, all: undefined } : { ...r, all: true });
  return (
    <a class={all ? "secondary small on" : "secondary small"} href={to} role="button" aria-pressed={all} data-all-projects-toggle data-on={all ? "" : undefined} onClick={(e) => follow(e, to)}>
      All projects
    </a>
  );
}
