// The column beside #agents or a board's channel (Arugula #1522): who's in
// it (its people, the chief of staff, and each lead or the board's lead),
// and who a post reaches, in plain words. It takes Details' place: these
// rooms' people are the project's, so there's nothing to manage here.

import { useEffect } from "preact/hooks";
import type { ChatStore } from "../chat-store";
import { useSubscribe } from "../ui/hooks";
import type { ChatRoom } from "../proto";
import { Avatar } from "../kit/avatar";
import { ChevronIcon } from "../ui/icons";
import { usePlatformApi } from "../platform-api";
import { agentName } from "../agent-name";
import { isAgentsRoom } from "./agent-rooms";
import { useProjectBoards } from "./project-boards";

function Who({ name, role, agent }: { name: string; role: string; agent?: boolean }) {
  return (
    <li class="chat-about-who">
      <Avatar name={name} agent={agent} />
      <span class="chat-about-name">{name}</span>
      <span class="m-meta">{role}</span>
    </li>
  );
}

function Reach({ rows }: { rows: [string, string][] }) {
  return (
    <dl class="chat-about-reach" data-reach-rules>
      {rows.map(([who, what]) => (
        <div key={who}>
          <dt>{who}</dt>
          <dd>{what}</dd>
        </div>
      ))}
    </dl>
  );
}

export function AgentAbout({ store, room }: { store: ChatStore; room: ChatRoom }) {
  const { nav } = usePlatformApi();
  useSubscribe((fn) => store.subscribe(fn));
  // Its people, though a thread has the room's column.
  useEffect(() => void store.detailOf(room.id).catch(() => {}), [room.id]);
  const boards = useProjectBoards(room.project ?? undefined) ?? [];
  const people = store.people(room.id);
  const agents = isAgentsRoom(room);
  const board = agents ? undefined : boards.find((b) => b.id === room.target);
  const leads = agents ? boards.filter((b) => !b.closed_at) : board ? [board] : [];
  const href = board && room.project && nav.boardHref?.(room.project, board.id);
  return (
    <aside class="chat-details chat-about-agents" aria-label={`About #${room.name}`} data-agents-about={agents ? "agents" : "board"}>
      <div class="chat-details-body">
        <h3>In #{room.name}</h3>
        <ul class="chat-about-list">
          {people.map((p) => (
            <Who key={p.account} name={p.name} role="person" />
          ))}
          {agents && <Who name={agentName({ role: "chief" })} role="yours" agent />}
          {leads.map((b) => (
            <Who key={b.id} name={b.title} role={b.lead ? "lead" : "no lead yet"} agent />
          ))}
        </ul>
        <h3>Who a post reaches</h3>
        {agents ? (
          <Reach
            rows={[
              ["You, or the chief of staff", "every lead, no tag needed"],
              ["A tag of yours", "wakes only those it names"],
              ["A lead", "only the leads it tags"],
              ["@here", "the active ones"],
              ["@<board>", "that board's lead"],
              ["@cos", "the chief of staff"],
            ]}
          />
        ) : (
          <Reach
            rows={[
              ["You", "the lead"],
              ["@here", "its active workers"],
              ["@channel", "the lead and every worker"],
              ["@<board>", "another board's lead"],
            ]}
          />
        )}
        {href && (
          <p>
            <a
              class="chat-go"
              href={href}
              data-agents-open-board
              onClick={(e) => {
                if (!nav.openBoard || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
                e.preventDefault();
                nav.openBoard(room.project!, board.id);
              }}
            >
              Open the board <ChevronIcon dir="right" />
            </a>
          </p>
        )}
      </div>
    </aside>
  );
}
