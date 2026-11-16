// Where an `@machine` mention got to (#502): one quiet line under the
// message per machine it names, or per session of one when it names its
// sessions (`@machine/session`). Control says it when the machine's daemon
// answers, and again when the machine's agent replies in the thread. Each
// is a status chip in the tone of its state.

import type { ChatMentionStatus, ChatMessage, ChatRoom } from "../proto";
import { usePlatformApi } from "../platform-api";
import { ChevronIcon, MachineIcon } from "../ui/icons";
import { Chip, type Tone } from "../ui/state-chip";

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
    case "no_such_session": {
      const none = `No session on ${who} is called ${s.asked ?? "that"}`;
      return s.names?.length ? `${none}: ${s.names.join(", ")}` : none;
    }
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
    case "answered":
      return `${named} answered`;
    case "chain":
      return `Not delivered to ${named}: a chain of agent replies; a person's message restarts it`;
    case "not_granted":
      return `Not granted: ${who}'s agents can't read this room; its owner was asked`;
  }
}

/** The lines under a message: nothing for one that mentions no machine. An
 * answer is a link to the reply it names, in the thread. */
export function MentionStatus({ room, m }: { room: ChatRoom; m: ChatMessage }) {
  const { nav } = usePlatformApi();
  const lines = m.mention_status ?? [];
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
