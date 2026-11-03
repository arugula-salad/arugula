// Where an `@machine` mention got to (#502): one quiet line under the
// message per machine it names. Control says it when the machine's daemon
// answers, and again when the machine's agent replies in the thread.

import type { ChatMentionStatus, ChatMessage, ChatRoom } from "../proto";
import { usePlatformApi } from "../platform-api";
import { MachineIcon } from "./icons";

/** The line's words: what the daemon said, in the sender's terms. */
export function mentionStatusText(s: ChatMentionStatus): string {
  const who = s.machine_name || "the machine";
  switch (s.state) {
    case "sending":
      return `Sending to ${who}…`;
    case "delivered":
      return s.session ? `Delivered to ${who}/${s.session}` : `Delivered to ${who}`;
    case "fell_back":
      return `${who}: went to ${s.session ?? "another session"} (no session called ${s.asked ?? "that"})`;
    case "waiting":
      return `Waiting on ${who}: no session took it`;
    case "queued":
      return `${who}${s.session ? `/${s.session}` : ""}: held: no hooks, waiting for ${who}`;
    case "silenced":
      return `Not delivered to ${who}${s.asked ? `/${s.asked}` : ""}: its owner silenced mentions for that session`;
    case "no_such_session": {
      const none = `No session on ${who} is called ${s.asked ?? "that"}`;
      return s.names?.length ? `${none}: ${s.names.join(", ")}` : none;
    }
    case "working":
      return `${who}${s.session ? `/${s.session}` : ""}: working…`;
    case "read":
      return `${who}${s.session ? `/${s.session}` : ""}: read`;
    case "dropped":
      return `${who}${s.session ? `/${s.session}` : ""}: dropped: ${s.why === "stopped" ? "the session was stopped" : "the session ended mid-turn"}`;
    case "offline":
      return `${who} is offline or didn't answer`;
    case "answered":
      return `${who} answered`;
    case "chain":
      return `Not delivered to ${who}: a chain of agent replies; a person's message restarts it`;
    case "not_granted":
      return `Not granted: ${who}'s agents can't read this room; its owner was asked`;
  }
}

/** The lines under a message: nothing for one that mentions no machine. An
 * answer is a link to the thread, where the reply is. */
export function MentionStatus({ room, m }: { room: ChatRoom; m: ChatMessage }) {
  const { nav } = usePlatformApi();
  const lines = m.mention_status ?? [];
  if (!lines.length || m.deleted !== null) return null;
  return (
    <div class="mention-status" role="group" aria-label="Agent mentions">
      {lines.map((s) => (
        <div key={s.machine} class={`mention-line ${s.state}`} data-mention-status={s.state} data-machine={s.machine} title={s.reply ? `Reply ${s.reply}` : undefined}>
          <MachineIcon size={12} />
          {s.state === "answered" ? (
            <button type="button" class="mention-link" onClick={() => nav.openRoom(room.id, undefined, m.parent ?? m.seq)}>
              {mentionStatusText(s)} ›
            </button>
          ) : (
            <span>{mentionStatusText(s)}</span>
          )}
        </div>
      ))}
    </div>
  );
}
