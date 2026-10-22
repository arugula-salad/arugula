// The platform page's links (M150): the path each room and project is at,
// with each id's current name for its slug.

import { chat } from "../shared/chat-store";
import { cloudWorkspaces } from "../shared/cloud";
import { issues } from "../shared/issues/store";
import { runs } from "./runs";
import { current, pathOf, type Route } from "./router";
import { session } from "./session";

/** The project a room belongs to: its team's, else the reader's own. */
export function projectOfRoom(room: string): string | undefined {
  const r = chat.room(room);
  return (r?.team ? session.ofTeam(r.team)?.id : undefined) ?? routeProject() ?? session.personal()?.id;
}

/** The project the route is in, if it's in one. */
export function routeProject(): string | undefined {
  const r = current();
  return "project" in r ? r.project : undefined;
}

/** The path a room is at, at message `msg`. */
export function roomPath(room: string, msg?: number): string {
  if (room.startsWith("dm_")) return pathOf({ page: "dm", room, msg }, names);
  // An issue's thread (M196) is on its issue's page.
  const r = chat.room(room);
  if (r?.kind === "object" && r.project && r.target?.startsWith("iss_")) return pathOf({ page: "issue", project: r.project, issue: r.target, msg }, names);
  // And a run's (M215) on its run's.
  if (r?.kind === "object" && r.project && r.target?.startsWith("run_")) return pathOf({ page: "run", project: r.project, run: r.target, msg }, names);
  const project = projectOfRoom(room);
  return project ? pathOf({ page: "chat", project, room, msg }, names) : "/";
}

/** Each id's name now, for its slug. */
export function names(id: string): string | undefined {
  if (id.startsWith("prj_")) return session.project(id)?.name;
  if (id.startsWith("cw_")) return cloudWorkspaces.find(id)?.name;
  if (id.startsWith("iss_")) return issues.known(id)?.title;
  const r = chat.room(id);
  return r?.kind === "channel" ? r.name : undefined;
}

/** Where an id-only address (`/chat/:room`, `/workspaces/:id`…) belongs,
 * once the page knows enough to say: null until then. A room needs chat's
 * rooms (its team says its project); Cloud is the reader's own project's. */
export function resolve(r: Extract<Route, { page: "find" }>): string | null {
  const own = session.personal()?.id;
  if (!own) return null;
  if (r.what === "chat") {
    if (r.room) {
      if (r.room.startsWith("dm_")) return pathOf({ page: "dm", room: r.room, ...(r.msg !== undefined ? { msg: r.msg } : {}) }, names);
      // Its team's project, once chat has said what's in it.
      if (chat.on === null) return null;
      return roomPath(r.room, r.msg);
    }
    const team = session.projects.find((p) => p.team)?.id;
    return pathOf({ page: "chat", project: team ?? own, ...(r.view ? { view: r.view, q: r.q } : {}) }, names);
  }
  if (r.what === "past") return pathOf({ page: "past", project: own, id: r.id }, names);
  if (r.what === "issue") {
    // Its project, once the page has read it (the page asks).
    const i = issues.known(r.issue);
    return i ? pathOf({ page: "issue", project: i.project, issue: i.id, ...(r.msg !== undefined ? { msg: r.msg } : {}) }, names) : null;
  }
  if (r.what === "run") {
    // Its project, once the page has read it (the page asks).
    const x = runs.known(r.run);
    return x ? pathOf({ page: "run", project: x.project, run: x.id, ...(r.msg !== undefined ? { msg: r.msg } : {}) }, names) : null;
  }
  return pathOf(r.workspace ? { page: "workspace", project: own, workspace: r.workspace, ...(r.tab ? { tab: r.tab } : {}) } : { page: "workspaces", project: own }, names);
}
