// What's new in the threads on a project's issues and runs (M196's object
// rooms), where those are: a dot and a count on each one's row, and a count
// on the project's Issues and Runs places. Chat's store has the rooms and
// keeps them live over its stream; opening the issue or the run reads its
// thread (shared/chat/room's `RoomBody`), which clears both.

import { chat } from "../shared/chat-store";
import { chatUnread, objectUnread, projectUnread } from "../shared/chat/unread";
import { useSubscribe } from "../shared/ui/hooks";

const said = (n: number, mention: boolean) => `${n} new in its thread${mention ? ", mentioning you" : ""}`;

/** The dot and count on an issue's or a run's row: nothing while its
 * thread has nothing new. */
export function ThreadUnread({ target }: { target: string }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const { n, mention } = objectUnread(chat.rooms, target);
  if (!n) return null;
  return (
    <span class={mention ? "thread-unread mention" : "thread-unread"} title={said(n, mention)} aria-label={said(n, mention)} data-thread-unread={n}>
      <span class="thread-unread-dot" aria-hidden="true" />
      {mention ? `@${n}` : n}
    </span>
  );
}

/** The count on a project's Issues (`iss_`) or Runs (`run_`) place; the
 * bar it's in listens to chat. */
export function PlaceUnread({ project, prefix }: { project: string; prefix: "iss_" | "run_" }) {
  const { n, mention } = projectUnread(chat.rooms, project, prefix);
  if (!n) return null;
  const what = prefix === "iss_" ? "issues'" : "runs'";
  const title = `${n} unread in this project's ${what} threads${mention ? ", mentioning you" : ""}`;
  // The rows' mark, a dot before the count, so "Issues 29" doesn't read
  // as 29 issues (UI audit #159); a reader hears the title.
  return (
    <span class={mention ? "thread-unread place-unread mention" : "thread-unread place-unread"} title={title} data-place-unread={n}>
      <span class="thread-unread-dot" aria-hidden="true" />
      <span aria-hidden="true">{mention ? `@${n}` : n}</span>
      <span class="place-unread-said">{title}</span>
    </span>
  );
}

/** The count on the bar's Chat (UI audit #193), as control's Chat button
 * counts: in a project its chat's rooms (a team's channels and DMs, or
 * Personal's threads), elsewhere every room chat lists; `@N` when any of
 * it mentions the reader. The bar it's in listens to chat. */
export function ChatPlaceUnread({ team }: { team?: string | null }) {
  const { n, mention } = chatUnread(chat.rooms, team);
  if (!n) return null;
  const title = `${n} unread in ${team !== undefined ? "this project's" : "your"} chat${mention ? ", mentioning you" : ""}`;
  const shown = n > 99 ? "99+" : n;
  return (
    <span class={mention ? "thread-unread place-unread mention" : "thread-unread place-unread"} title={title} data-chat-count={n}>
      <span class="thread-unread-dot" aria-hidden="true" />
      <span aria-hidden="true">{mention ? `@${shown}` : shown}</span>
      <span class="place-unread-said">{title}</span>
    </span>
  );
}
