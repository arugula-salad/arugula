// Cloud's routes on the platform page (M153): the Cloud workspace views
// (../platform/cloud) open the list, a workspace, a tab or a past one
// through these, as paths under the reader's own project, where Cloud
// workspaces belong ("The project is the unit of platform sharing").

import type { CloudRoutes } from "../shared/cloud";
import { names } from "./links";
import { current, go, pathOf, type Route } from "./router";
import { session } from "./session";

/** The project Cloud's pages are under: the route's, else the reader's own. */
function project(): string | undefined {
  const r = current();
  return ("project" in r ? r.project : undefined) ?? session.personal()?.id;
}

const to = (r: Route) => go(pathOf(r, names));

export const pathRoutes: CloudRoutes = {
  workspace() {
    const r = current();
    if (r.page === "workspaces") return { workspace: null, tab: null };
    return r.page === "workspace" ? { workspace: r.workspace, tab: r.tab ?? null } : null;
  },
  past() {
    const r = current();
    return r.page === "past" ? r.id : null;
  },
  openWorkspace(workspace, tab) {
    const p = project();
    if (p) to(workspace ? { page: "workspace", project: p, workspace, ...(tab ? { tab } : {}) } : { page: "workspaces", project: p });
  },
  openPast(id) {
    const p = project();
    if (p) to({ page: "past", project: p, id });
  },
  leave() {
    const p = project();
    to(p ? { page: "project", project: p } : { page: "home" });
  },
};
