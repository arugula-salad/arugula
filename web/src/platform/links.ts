// The platform page's links (M150): the path each room and project is at,
// with each id's current name for its slug.

import { chat } from "../shared/chat-store";
import { boards } from "../shared/boards/store";
import { listedInChat } from "../shared/chat/unread";
import { issues } from "../shared/issues/store";
import { runs } from "./runs";
import { current, pathOf, type Route } from "./router";
import { session } from "./session";

/** The project a room belongs to: its team's; an object thread's, its
 * object's; a room of no team (a machine's thread, an archive), the
 * reader's own (UI audit #169: each project's chat lists its own rooms). A
 * room not known yet stays in the route's project. */
export function projectOfRoom(room: string): string | undefined {
  const r = chat.room(room);
  if (r?.kind === "object" && r.project) return r.project;
  if (r?.team) return session.ofTeam(r.team)?.id ?? routeProject() ?? session.personal()?.id;
  if (r) return session.personal()?.id;
  return routeProject() ?? session.personal()?.id;
}

/** The project the route is in, if it's in one. */
export function routeProject(): string | undefined {
  const r = current();
  return "project" in r ? r.project : undefined;
}

/** The path a room is at, at message `msg`; with `thread`, the thread of
 * that message beside it (#106). */
export function roomPath(room: string, msg?: number, thread?: number): string {
  // A DM is its team's project's (UI audit #169).
  if (room.startsWith("dm_")) {
    const project = projectOfRoom(room);
    return project ? pathOf({ page: "dm", project, room, msg, thread }, names) : "/";
  }
  // An issue's thread (M196) is on its issue's page.
  const r = chat.room(room);
  if (r?.kind === "object" && r.project && r.target?.startsWith("iss_")) return pathOf({ page: "issue", project: r.project, issue: r.target, msg }, names);
  // And a run's (M215) on its run's.
  if (r?.kind === "object" && r.project && r.target?.startsWith("run_")) return pathOf({ page: "run", project: r.project, run: r.target, msg }, names);
  const project = projectOfRoom(room);
  return project ? pathOf({ page: "chat", project, room, msg, thread }, names) : "/";
}

/** Where a chat route's room is when it's another project's (an old link,
 * a slip), else null (UI audit #169: each project's chat is its own). */
export function roomElsewhere(r: Route): string | null {
  if ((r.page !== "chat" && r.page !== "dm") || !r.room) return null;
  const x = chat.room(r.room);
  return x && listedInChat(x) && projectOfRoom(x.id) !== r.project ? roomPath(x.id, r.msg, r.thread) : null;
}

/** Each id's name now, for its slug. */
export function names(id: string): string | undefined {
  if (id.startsWith("prj_")) return session.project(id)?.name;
  if (id.startsWith("iss_")) return issues.known(id)?.title;
  if (id.startsWith("brd_")) return boards.title(id);
  const r = chat.room(id);
  return r?.kind === "channel" ? r.name : undefined;
}

/** Where an id-only address (`/chat/:room`, `/issues/:issue`…) belongs,
 * once the page knows enough to say: null until then. A room needs chat's
 * rooms (its team says its project). */
export function resolve(r: Extract<Route, { page: "find" }>): string | null {
  const own = session.personal()?.id;
  if (!own) return null;
  if (r.what === "chat") {
    if (r.room) {
      // Its team's project (a DM's too, UI audit #169), once chat has said
      // what's in it.
      if (chat.on === null) return null;
      return roomPath(r.room, r.msg, r.thread);
    }
    const team = session.projects.find((p) => p.team)?.id;
    return pathOf({ page: "chat", project: team ?? own, ...(r.view ? { view: r.view, q: r.q } : {}) }, names);
  }
  if (r.what === "issue") {
    // Its project, once the page has read it (the page asks).
    const i = issues.known(r.issue);
    return i ? pathOf({ page: "issue", project: i.project, issue: i.id, ...(r.msg !== undefined ? { msg: r.msg } : {}) }, names) : null;
  }
  // A run: its project, once the page has read it (the page asks).
  const x = runs.known(r.run);
  return x ? pathOf({ page: "run", project: x.project, run: x.id, ...(r.msg !== undefined ? { msg: r.msg } : {}) }, names) : null;
}
