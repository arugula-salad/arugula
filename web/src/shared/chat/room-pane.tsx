// A chat room as a pane (Arugula #1522): #agents or a board's channel
// beside a board's lanes, as its place layout keeps it (a `room` pane, its
// `ref` the room's id). The pane is Chat's own room, pinned: its messages,
// threads in place, tags and composer. On a board: "+ pane" offers #agents
// and the board's channel, and `g a` opens #agents beside it.

import { useRef } from "preact/hooks";
import { chat } from "../chat-store";
import type { LayoutNewPane } from "../proto";
import { control } from "../controls";
import { useBinding } from "../keys";
import { useSubscribe } from "../ui/hooks";
import type { MenuItem } from "../ui/menu";
import { ChatApp } from "./app";
import { isAgentsRoom } from "./agent-rooms";

/** Whether a pane is a room: its kind, or (from a control that keeps no
 * `room` kind yet) a room's id for its ref. */
export const isRoomPane = (p: { kind: string; ref: string }) => p.kind === "room" || /^(ob|ch)_[0-9a-f]{8,32}$/.test(p.ref);

/** The room, drawn as a pane: one room, no sidebar, the narrow layout. */
export function RoomPane({ room }: { room: string }) {
  return (
    <div class="chat-room-pane" data-room-pane={room}>
      <ChatApp route={{ room }} fit={{ pinned: true, narrow: true, scoped: true }} />
    </div>
  );
}

const pane = (room: string, title: string): LayoutNewPane => ({ kind: "room", ref: room, title, machine: null, state: null });

/** On a board, "+ pane"'s rooms: #agents (once the project has it) and the
 * board's channel. `show` opens a pane on the board. */
export function roomPaneItems(project: string, board: string, title: string, show: (p: LayoutNewPane) => void): MenuItem[] {
  if (!chat.on) return [];
  const agents = chat.rooms.find((r) => isAgentsRoom(r, project));
  // The board's channel is made the first time someone opens it.
  const channel = () => void chat.openObject(board).then((r) => show(pane(r.id, `#${title}`)), () => {});
  return [
    ...(agents ? [{ label: "#agents", run: () => show(pane(agents.id, "#agents")), attrs: { "data-board-add-pane": "agents" } }] : []),
    { label: `#${title}`, run: channel, attrs: { "data-board-add-pane": "channel" } },
  ];
}

/** On a board, `g a`: #agents beside it. Draws nothing. */
export function AgentsPaneKey({ project, show }: { project: string; show: (p: LayoutNewPane) => void }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const agents = chat.on ? chat.rooms.find((r) => isAgentsRoom(r, project)) : undefined;
  const latest = useRef(show);
  latest.current = show;
  useBinding(agents ? { ...control("board.agents-pane"), run: () => latest.current(pane(agents.id, "#agents")) } : null, [agents?.id]);
  return null;
}
