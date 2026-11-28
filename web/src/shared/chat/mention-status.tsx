// Who a message to agents got to (#502, Arugula #1560): one small icon in
// its head, after the time, in the tone of the worst state, with a count
// when more than one agent got it. A hover (a tap on a phone) opens a card
// like a reaction's: each agent, by machine or by session of one
// (`@machine/session`, #875), and its state in plain words. Control says
// each when the machine's daemon answers, and again when its agent replies.
// A board channel's lead gets every post there by leading it, which control
// keeps no status of: its row is drawn from the board. And a thread's agents
// that have read this far (#646) read it.

import { useRef, useState } from "preact/hooks";
import type { ChatMentionStatus, ChatMessage, ChatRoom, ChatThreadAgent, ProjectBoard } from "../proto";
import { chatAgentName, type LedBoard } from "../agent-name";
import { usePlatformApi } from "../platform-api";
import { CheckIcon, DotIcon, MachineIcon } from "../ui/icons";
import { Popover, pressedShut } from "../ui/menu";
import { Chip } from "../ui/state-chip";
import { isAgentRoom, isBoardChannel } from "./agent-rooms";
import { machineHandle, sessionSlug } from "./agents";
import { typesMachine } from "./mention-typed";
import { useLedBoards, useProjectBoards } from "./project-boards";

/** A row: a status, or a board channel's lead, which got the post. */
type Row = Omit<ChatMentionStatus, "state"> & { state: ChatMentionStatus["state"] | "reached" };

/** A state's tone, in no colour of its own (Arugula #1560): got it (read,
 * answered, reached) quiet, on its way a dim pulse, and what waits or didn't
 * get there amber. */
export function mentionTone(state: Row["state"]): "quiet" | "pending" | "warn" {
  switch (state) {
    case "read":
    case "answered":
    case "reached":
      return "quiet";
    case "sending":
    case "delivered":
    case "working":
      return "pending";
    default:
      return "warn";
  }
}

/** A warning that routing, not the poster, may have made (#1167): where
 * the machine went to no session it named, or wasn't there. Outside the
 * agents' rooms one for a machine the post didn't type isn't shown. A
 * session silenced or a turn ended is the agent's own state, shown even
 * when the To row chose it. */
const routed = (state: Row["state"]) => ["fell_back", "waiting", "queued", "no_such_session", "offline", "chain", "not_granted", "mentions_off"].includes(state);

/** A row's state, in plain words. */
export function mentionWords(s: Row): string {
  switch (s.state) {
    case "reached":
      return "reached";
    case "sending":
      return "sending…";
    case "delivered":
      // #643: one that took it at once is `working`, so this one has it
      // waiting: for its next tool call (#644), or an older daemon's turn end.
      return s.session ? "reads it at its next step" : "delivered";
    case "fell_back":
      return `went to ${s.session ?? "another session"}: no session called ${s.asked ?? "that"}`;
    case "waiting":
      return "waiting: no session took it";
    case "queued":
      return "held: no hooks";
    case "silenced":
      return "not delivered: its owner silenced this session";
    case "no_such_session":
      return `no session named ${s.asked ?? "that"}`;
    case "working":
      return "working…";
    case "read":
      return "read";
    case "dropped":
      return `dropped: ${s.why === "stopped" ? "the session was stopped" : "the session ended mid-turn"}`;
    case "offline":
      return "offline or didn't answer";
    case "mentions_off":
      return "not delivered: its owner turned agent mentions off";
    case "answered":
      return "answered ↗";
    case "chain":
      return "not delivered: a chain of agent replies; a person's message restarts it";
    case "not_granted":
      return "not granted: its agents can't read this room; its owner was asked";
  }
}

/** Whom a row is: the session that has it (#1290), else the one the post
 * named, by what it is (../agent-name.ts `chatAgentName`, Arugula #1582),
 * else the machine. `boards`: whose lead it may be. */
export function mentionWho(s: Row, boards: readonly LedBoard[] = []): string {
  const session = s.session ?? s.for_session;
  return session ? chatAgentName({ machine: s.machine, machine_name: s.machine_name, session }, boards) : s.machine_name || "the machine";
}

/** The icon after a message's time: nothing for one that went to no agent.
 * Outside the agents' rooms, a warning for a machine the poster didn't type
 * (routing put it there) isn't shown (#1167); in them the bus chose whom to
 * tell, so each is. `insert` writes into the composer this message is
 * answered in. */
export function MentionStatus({ room, m, seen = [], insert }: { room: ChatRoom; m: ChatMessage; seen?: ChatThreadAgent[]; insert?: (text: string) => void }) {
  const { nav } = usePlatformApi();
  const led = useLedBoards();
  const board = isBoardChannel(room) ? room.target : null;
  const lead = useProjectBoards(board ? (room.project ?? undefined) : undefined)?.find((b) => b.id === board)?.lead;
  const [open, setOpen] = useState<"hover" | "tap" | null>(null);
  const from = useRef<HTMLButtonElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const agents = isAgentRoom(room);
  const leads = (s: Row) => !!lead && lead.machine === s.machine && sessionSlug(s.for_session || s.session || lead.session) === sessionSlug(lead.session);
  const lines: Row[] = (m.mention_status ?? []).filter((s) => agents || !routed(s.state) || !s.machine_name || typesMachine(m.text ?? "", s.machine_name));
  if (lead && !lines.some(leads) && ledBy(lead, m)) lines.unshift({ machine: lead.machine, machine_name: lead.machine_name, session: lead.session, state: "reached" });
  for (const a of seen) {
    const i = lines.findIndex((s) => s.machine === a.id && sessionSlug(s.for_session || s.session || a.name) === sessionSlug(a.name));
    const was = lines[i];
    if (!was) lines.push({ machine: a.id, machine_name: a.machine_name, session: a.name, state: "read" });
    else if (was.state === "reached" || mentionTone(was.state) === "pending") lines[i] = { ...was, state: "read" };
  }
  if (!lines.length || m.deleted !== null) return null;
  const tone = (["warn", "pending"] as const).find((t) => lines.some((s) => mentionTone(s.state) === t)) ?? "quiet";
  const later = (fn: () => void, ms: number) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(fn, ms);
  };
  const label = `Sent to ${lines.length === 1 ? mentionWho(lines[0]!, led) : `${lines.length} agents`}`;
  return (
    <span
      class="receipt"
      onPointerEnter={(e) => e.pointerType === "mouse" && later(() => setOpen((o) => o ?? "hover"), 250)}
      onPointerLeave={(e) => e.pointerType === "mouse" && later(() => setOpen((o) => (o === "hover" ? null : o)), 200)}
    >
      <button ref={from} type="button" class="receipt-icon" data-tone={tone} data-receipt={lines.length} aria-label={label} onClick={() => !pressedShut(from.current) && setOpen("tap")}>
        <MachineIcon size="xs" />
        {lines.length > 1 && lines.length}
      </button>
      {open && (
        <Popover from={from} passive={open === "hover"} label={label} attrs={{ "data-receipt-card": "" }} onClose={() => setOpen(null)}>
          <ul class="receipt-list" role="group" aria-label="Agent mentions">
            {lines.map((s) => (
              <li key={`${s.machine}/${s.for_session ?? ""}`} class="mention-line" data-mention-status={s.state} data-machine={s.machine} data-session={s.for_session} data-mention-names={(s.state === "no_such_session" && !!insert && !!s.names?.length) || undefined}>
                <span class="receipt-mark" data-tone={mentionTone(s.state)}>
                  {mentionTone(s.state) === "quiet" ? <CheckIcon size="xs" /> : <DotIcon size="xs" />}
                </span>
                <b title={s.machine_name ? `on ${s.machine_name}` : undefined}>{mentionWho(s, led)}</b>{" "}
                {s.state === "answered" && s.reply ? (
                  <button type="button" class="mention-link link" title={`Reply ${s.reply}`} onClick={() => nav.openRoom(room.id, s.reply, m.parent ?? m.seq)}>
                    {mentionWords(s)}
                  </button>
                ) : (
                  <span>{mentionWords(s)}</span>
                )}
                {/* The sessions it could have named: a pick puts `@machine/session` in the composer (#1167). */}
                {s.state === "no_such_session" &&
                  insert &&
                  s.names?.map((n) => (
                    <Chip
                      key={n}
                      data-mention-pick={n}
                      onClick={() => {
                        setOpen(null);
                        insert(`@${machineHandle(s.machine_name ?? "")}/${sessionSlug(n)} `);
                      }}
                    >
                      {n}
                    </Chip>
                  ))}
              </li>
            ))}
          </ul>
        </Popover>
      )}
    </span>
  );
}

/** Whether a board channel's lead got `m` by leading the channel: posted
 * since it took the lead, and not by the lead itself. */
function ledBy(lead: NonNullable<ProjectBoard["lead"]>, m: ChatMessage): boolean {
  return m.at >= lead.at && !(m.agent?.machine === lead.machine && sessionSlug(m.agent.session ?? "") === sessionSlug(lead.session));
}
