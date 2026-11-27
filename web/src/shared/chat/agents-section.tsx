// Chat's Agents section (Arugula #1522): after Channels and Direct
// messages, collapsing as they do. The reader's chief of staff first, then
// #agents (the project's work across boards: deploys, merge order, holds),
// then one # row per open board, its lead's dot as a DM shows presence;
// closed boards fold under "archived". A board's channel that nobody has
// opened yet is made on its first click. Which rooms these are is
// ./agent-rooms.

import { useState } from "preact/hooks";
import { both, useBindings } from "../keys";
import type { ChatStore } from "../chat-store";
import type { ChatRoom } from "../proto";
import { usePlatformApi } from "../platform-api";
import { Row } from "../kit/row";
import { Count } from "../ui/state-chip";
import { ChevronIcon } from "../ui/icons";
import { agentName } from "../agent-name";
import { counted, bold } from "./unread";
import { agentRooms, type AgentRooms, type BoardChannel } from "./agent-rooms";
import { toggleGroup } from "./sections";
import { useProjectBoards } from "./project-boards";

export const AGENTS_GROUP = "agents";

/** When `j` or `k` last opened a room: that room doesn't take the keys'
 * focus into its box to write in, so the next press moves on. */
let movedAt = 0;
export const movedByKey = () => Date.now() - movedAt < 1500;

/** The section's rooms, for the keys that move through the rooms. */
export function useAgentRooms(store: ChatStore, project: string | undefined): AgentRooms | null {
  const list = useProjectBoards(project, true);
  return project ? agentRooms(store.rooms, list ?? [], project) : null;
}

/** The rooms the section shows, in its order (for Alt+↑/↓). */
export const agentOrder = (a: AgentRooms | null): ChatRoom[] => (a ? [a.staff, a.agents, ...a.open.map((c) => c.room)].filter((r): r is ChatRoom => !!r) : []);

export function AgentsSection({ store, rooms, picked, projectName }: { store: ChatStore; rooms: AgentRooms; picked: string | undefined; projectName: string }) {
  const api = usePlatformApi();
  const { nav } = api;
  const shut = store.sidebar.collapsed.includes(AGENTS_GROUP);
  const [archived, setArchived] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const open = (r: ChatRoom) => nav.openRoom(r.id);
  // A board's channel is made the first time someone opens it.
  const openChannel = (c: BoardChannel) => {
    if (c.room) return open(c.room);
    store.openObject(c.board.id).then(open, (e: Error) => setError(`Couldn't open #${c.board.title}: ${e.message}`));
  };
  // The prototype's keys (#1522): j and k move through these rows; g a is
  // #agents.
  const steps: (() => void)[] = [
    ...(rooms.staff ? [() => open(rooms.staff!)] : []),
    ...(rooms.agents ? [() => open(rooms.agents!)] : []),
    ...rooms.open.map((c) => () => openChannel(c)),
  ];
  const ids = [rooms.staff?.id, rooms.agents?.id, ...rooms.open.map((c) => c.room?.id ?? c.board.id)];
  const at = ids.findIndex((id) => id !== undefined && id === picked);
  const step = (d: 1 | -1) => {
    if (!steps.length) return;
    const next = at < 0 ? (d > 0 ? 0 : steps.length - 1) : Math.min(steps.length - 1, Math.max(0, at + d));
    if (next === at) return;
    movedAt = Date.now();
    steps[next]!();
  };
  useBindings(
    [
      { id: "chat.agents-next", label: "The next agents' room", chip: "move", keys: both("key:j"), hint: { mac: "j k", other: "j k" }, run: () => step(1) },
      { id: "chat.agents-prev", label: "The agents' room before", keys: both("key:k"), run: () => step(-1) },
      ...(rooms.agents ? [{ id: "chat.agents", label: "#agents", group: "go" as const, chip: "#agents", keys: both("key:g key:a"), run: () => open(rooms.agents!) }] : []),
    ],
    [picked, ids.join(), !!rooms.agents],
  );
  const linkOf = (r: ChatRoom | undefined) => (r ? nav.roomHref(r.id) : undefined);
  const unread = [rooms.staff, rooms.agents, ...rooms.open.map((c) => c.room)].reduce((n, r) => n + (r?.unread ?? 0), 0);
  const channel = (c: BoardChannel, closed = false) => {
    const r = c.room;
    const n = r ? counted(r) : 0;
    const lead = !!c.board.lead;
    return (
      <Row
        key={c.board.id}
        kind={r && r.id === picked ? "selected" : "plain"}
        bold={!!r && bold(r)}
        href={linkOf(r) ?? ""}
        data-agents-board={c.board.id}
        data-chat-room={r?.id}
        data-quiet={closed ? "" : undefined}
        lead={<span class="chat-sigil">#</span>}
        title={c.board.title}
        meta={[
          n > 0 && <Count for={r?.mentions ? "mention" : "you"}>{n}</Count>,
          closed ? (
            "closed"
          ) : (
            <span class={`chat-lead-dot presence-dot presence-${lead ? "active" : "offline"}`} data-board-lead={lead ? "" : undefined} title={lead ? "It has a lead" : "No lead yet"} />
          ),
        ].filter(Boolean)}
        line={{ title: `#${c.board.title}: its lead, its workers and its people` }}
        onClick={(e) => {
          if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
          e.preventDefault();
          openChannel(c);
        }}
      />
    );
  };
  // Nothing of the agents' yet (no chief of staff, #agents or board): no section.
  if (!rooms.staff && !rooms.agents && !rooms.open.length && !rooms.closed.length) return null;
  return (
    <section data-chat-group={AGENTS_GROUP}>
      <h2 class="chat-section chat-channels-head">
        <button aria-expanded={!shut} onClick={() => toggleGroup(store, AGENTS_GROUP, api.toast)} title={shut ? "Show them" : "Hide them"}>
          <span class="chat-caret" aria-hidden="true">
            <ChevronIcon open={!shut} />
          </span>
          Agents
          {shut && unread > 0 && <Count for="you">{unread}</Count>}
        </button>
      </h2>
      {!shut && (
        <>
          {rooms.staff && (
            <Row
              kind={rooms.staff.id === picked ? "selected" : "plain"}
              bold={bold(rooms.staff)}
              href={nav.roomHref(rooms.staff.id)}
              data-chat-room={rooms.staff.id}
              data-agents-staff
              lead={
                <span class="chat-sigil" aria-hidden="true">
                  ✦
                </span>
              }
              title={`${agentName({ role: "chief" })} · ${projectName}`}
              meta={counted(rooms.staff) > 0 ? [<Count for="you">{counted(rooms.staff)}</Count>] : undefined}
              line={{ title: "Only you and your chief of staff" }}
              onClick={(e) => {
                if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
                e.preventDefault();
                open(rooms.staff!);
              }}
            />
          )}
          {rooms.agents && (
            <Row
              kind={rooms.agents.id === picked ? "selected" : "plain"}
              bold={bold(rooms.agents)}
              href={nav.roomHref(rooms.agents.id)}
              data-chat-room={rooms.agents.id}
              data-agents-room
              lead={<span class="chat-sigil">#</span>}
              title="agents"
              meta={counted(rooms.agents) > 0 ? [<Count for={rooms.agents.mentions ? "mention" : "you"}>{counted(rooms.agents)}</Count>] : undefined}
              line={{ title: "#agents: work across boards, such as deploys, merge order and holds" }}
              onClick={(e) => {
                if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
                e.preventDefault();
                open(rooms.agents!);
              }}
            />
          )}
          {rooms.open.map((c) => channel(c))}
          {rooms.closed.length > 0 && (
            <Row
              data-agents-archived={rooms.closed.length}
              open={archived}
              lead={
                <span class="chat-sigil" aria-hidden="true">
                  <ChevronIcon open={archived} />
                </span>
              }
              title={`archived · ${rooms.closed.length}`}
              onClick={() => setArchived((a) => !a)}
            >
              {rooms.closed.map((c) => channel(c, true))}
            </Row>
          )}
          {error && (
            <p class="chat-none control-error" role="alert">
              {error}
            </p>
          )}
        </>
      )}
    </section>
  );
}
