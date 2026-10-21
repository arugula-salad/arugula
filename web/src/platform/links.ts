// The platform page's links (M150): the path each room and project is at,
// with each id's current name for its slug.

import { chat } from "../shared/chat-store";
import { current, pathOf } from "./router";
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
  const project = projectOfRoom(room);
  return project ? pathOf({ page: "chat", project, room, msg }, names) : "/";
}

/** Each id's name now, for its slug. */
export function names(id: string): string | undefined {
  if (id.startsWith("prj_")) return session.project(id)?.name;
  if (id.startsWith("cw_")) return undefined;
  const r = chat.room(id);
  return r?.kind === "channel" ? r.name : undefined;
}
