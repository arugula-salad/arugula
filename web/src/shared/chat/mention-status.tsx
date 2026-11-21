// Where an `@machine` mention got to (#502): one quiet line under the
// message per machine it names, or per session of one when it names its
// sessions (`@machine/session`). Control says it when the machine's daemon
// answers, and again when the machine's agent replies in the thread. Each
// is a status chip in the tone of its state.

import { useRef, useState } from "preact/hooks";
import type { ChatMentionStatus, ChatMessage, ChatRoom } from "../proto";
import { usePlatformApi } from "../platform-api";
import { ChevronIcon, MachineIcon } from "../ui/icons";
import { Popover, pressedShut } from "../ui/menu";
import { Chip, type Tone } from "../ui/state-chip";
import { machineHandle, sessionSlug } from "./agents";
import { typesMachine } from "./mention-typed";

/** A state's tone: working accent, read and answered ok, the ones that wait
 * or went elsewhere warn, a dropped turn danger, and what was only sent or
 * handed over is neutral. */
export function mentionTone(state: ChatMentionStatus["state"]): Tone {
  switch (state) {
    case "working":
      return "accent";
    case "read":
    case "answered":
      return "ok";
    case "dropped":
      return "danger";
    case "fell_back":
    case "waiting":
    case "queued":
    case "no_such_session":
    case "offline":
    case "chain":
    case "not_granted":
    case "mentions_off":
      return "warn";
    case "sending":
    case "delivered":
    case "silenced":
      return "muted";
  }
}

/** The line's words: what the daemon said, in the sender's terms. */
export function mentionStatusText(s: ChatMentionStatus): string {
  const who = s.machine_name || "the machine";
  // The session the post named, so two lines for one machine differ.
  const named = s.for_session ? `${who}/${s.for_session}` : who;
  const at = s.session ? `${who}/${s.session}` : named;
  switch (s.state) {
    case "sending":
      return `Sending to ${named}…`;
    case "delivered":
      // #643: a session that took it at once is `working`, so this one has
      // it waiting: for its next tool call (#644), or an older daemon's turn end.
      return s.session ? `${at}: reads this at its next step` : `Delivered to ${named}`;
    case "fell_back":
      return `${who}: went to ${s.session ?? "another session"} (no session called ${s.asked ?? "that"})`;
    case "waiting":
      return `Waiting on ${named}: no session took it`;
    case "queued":
      return `${at}: held: no hooks, waiting for ${who}`;
    case "silenced":
      return `Not delivered to ${who}${s.asked ? `/${s.asked}` : ""}: its owner silenced mentions for that session`;
    // The machine's sessions are behind the chip (#1167), not in it.
    case "no_such_session":
      return `${who}: no session named ${s.asked ?? "that"}`;
    case "working":
      return `${at}: working…`;
    case "read":
      return `${at}: read`;
    case "dropped":
      return `${at}: dropped: ${s.why === "stopped" ? "the session was stopped" : "the session ended mid-turn"}`;
    case "offline":
      return s.for_session ? `${named}: ${who} is offline or didn't answer` : `${who} is offline or didn't answer`;
    case "mentions_off":
      return `Not delivered to ${named}: its owner turned agent mentions off`;
    // The session that answered, not just its machine (#1290): a board's thread reaches its lead by none.
    case "answered":
      return `${at} answered`;
    case "chain":
      return `Not delivered to ${named}: a chain of agent replies; a person's message restarts it`;
    case "not_granted":
      return `Not granted: ${who}'s agents can't read this room; its owner was asked`;
  }
}

/** A `no_such_session` line with names to offer: the chip opens a list of
 * them, and a pick puts `@machine/session` in the composer (#1167). */
function NamesChip({ s, insert }: { s: ChatMentionStatus; insert: (text: string) => void }) {
  const [open, setOpen] = useState(false);
  const from = useRef<HTMLSpanElement | null>(null);
  const handle = machineHandle(s.machine_name ?? "");
  return (
    <>
      <span ref={from} class="mention-names">
        <Chip tone={mentionTone(s.state)} dot class="mention-link" title="Sessions on this machine" onClick={() => !pressedShut(from.current) && setOpen(true)}>
          {mentionStatusText(s)} <ChevronIcon dir="down" />
        </Chip>
      </span>
      {open && (
        <Popover from={from} label={`Sessions on ${s.machine_name || "the machine"}`} attrs={{ "data-mention-names": "" }} onClose={() => setOpen(false)}>
          <ul class="mention-names-list">
            {(s.names ?? []).map((n) => (
              <li key={n}>
                <button
                  type="button"
                  data-mention-pick={n}
                  onClick={() => {
                    setOpen(false);
                    insert(`@${handle}/${sessionSlug(n)} `);
                  }}
                >
                  {n}
                </button>
              </li>
            ))}
          </ul>
        </Popover>
      )}
    </>
  );
}

/** The lines under a message: nothing for one that mentions no machine. An
 * answer is a link to the reply it names, in the thread. A warning for a
 * machine the poster didn't type (routing put it there) isn't shown (#1167).
 * `insert` writes into the composer this message is answered in. */
export function MentionStatus({ room, m, insert }: { room: ChatRoom; m: ChatMessage; insert?: (text: string) => void }) {
  const { nav } = usePlatformApi();
  const lines = (m.mention_status ?? []).filter((s) => mentionTone(s.state) !== "warn" || !s.machine_name || typesMachine(m.text ?? "", s.machine_name));
  if (!lines.length || m.deleted !== null) return null;
  return (
    <div class="mention-status" role="group" aria-label="Agent mentions">
      {lines.map((s) => (
        <div key={`${s.machine}/${s.for_session ?? ""}`} class={`mention-line ${s.state}`} data-mention-status={s.state} data-machine={s.machine} data-session={s.for_session} title={s.reply ? `Reply ${s.reply}` : undefined}>
          <MachineIcon size="xs" />
          {s.state === "answered" ? (
            <Chip tone={mentionTone(s.state)} dot class="mention-link" onClick={() => nav.openRoom(room.id, s.reply, m.parent ?? m.seq)}>
              {mentionStatusText(s)} <ChevronIcon dir="right" />
            </Chip>
          ) : s.state === "no_such_session" && s.names?.length && insert ? (
            <NamesChip s={s} insert={insert} />
          ) : (
            <Chip tone={mentionTone(s.state)} dot>
              {mentionStatusText(s)}
            </Chip>
          )}
        </div>
      ))}
    </div>
  );
}
