// Agents in Chat (Arugula #1522): which rooms are the agents', and how the
// Agents section lists them. A project's own thread is #agents (an object
// thread on the project, `prj_…`); a board's is its channel (`brd_…`); the
// reader's staff room is their chief of staff. An issue's thread is its
// comments, never a room in Chat. No page state: rooms and boards in,
// rows out, so `scripts/chat-agents.test.mjs` runs it without a browser.

import type { ChatRoom, ProjectBoard } from "../proto";
import type { RoomTags } from "./tags";

/** #agents: the project's own thread. */
export const isAgentsRoom = (r: ChatRoom, project?: string): boolean => r.kind === "object" && !!r.target?.startsWith("prj_") && (!project || r.target === project);

/** A board's channel. */
export const isBoardChannel = (r: ChatRoom): boolean => r.kind === "object" && !!r.target?.startsWith("brd_");

/** The reader's chief of staff, in `project`. */
export const isStaffRoom = (r: ChatRoom, project?: string): boolean => r.kind === "dm" && r.staff && (!project || r.project === project);

/** A room of the Agents section: its threads open in place, beside its
 * About (who's in it, who a post reaches). */
export const isAgentRoom = (r: ChatRoom): boolean => isAgentsRoom(r) || isBoardChannel(r);

/** Which tags `r`'s composer offers and its messages draw (Arugula #1582). */
export const roomTags = (r: ChatRoom): RoomTags => (isAgentsRoom(r) ? "agents" : isBoardChannel(r) ? "board" : "room");

/** The projects whose boards a post in `r` may tag: control's list. From a
 * server before it, an agents' room's own project, and no other room's:
 * that server tells no lead of a tag there. */
export const tagProjects = (r: ChatRoom): string[] => (r.tag_projects?.length ? r.tag_projects : isAgentRoom(r) && r.project ? [r.project] : []);

/** A board's row: the board, and its channel once someone has opened it. */
export interface BoardChannel {
  board: ProjectBoard;
  room?: ChatRoom;
}

export interface AgentRooms {
  staff?: ChatRoom;
  agents?: ChatRoom;
  /** The open boards' channels, in the project's order. */
  open: BoardChannel[];
  /** The closed boards' channels: "archived". */
  closed: BoardChannel[];
}

/** The Agents section of `project`'s Chat, from the rooms the reader has
 * and the project's boards. */
export function agentRooms(rooms: ChatRoom[], boards: ProjectBoard[], project: string): AgentRooms {
  const channel = (b: ProjectBoard): BoardChannel => {
    const room = rooms.find((r) => isBoardChannel(r) && r.target === b.id);
    return room ? { board: b, room } : { board: b };
  };
  const mine = boards.filter((b) => b.project === project);
  return {
    staff: rooms.find((r) => isStaffRoom(r, project)),
    agents: rooms.find((r) => isAgentsRoom(r, project)),
    open: mine.filter((b) => !b.closed_at).map(channel),
    closed: mine.filter((b) => !!b.closed_at).map(channel),
  };
}
