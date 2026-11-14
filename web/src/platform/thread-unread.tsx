// What's new in the threads on a project's issues and runs (M196's object
// rooms), where those are: a dot and a count on each one's row (the rail
// counts them on Boards and Sessions). Chat's store has the rooms and
// keeps them live over its stream; opening the issue or the run reads its
// thread (shared/chat/room's `RoomBody`), which clears both.

import { chat } from "../shared/chat-store";
import { objectUnread } from "../shared/chat/unread";
import { useSubscribe } from "../shared/ui/hooks";

const said = (n: number, mention: boolean) => `${n} new in its thread${mention ? ", mentioning you" : ""}`;

/** The dot and count on an issue's or a run's row: nothing while its
 * thread has nothing new. */
export function ThreadUnread({ target }: { target: string }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const { n, mention, quiet } = objectUnread(chat.rooms, target);
  // New, and asking nothing of the reader: a dot with no number.
  if (quiet) {
    const title = `${quiet} new in its thread`;
    return <span class="thread-unread quiet" title={title} aria-label={title} data-thread-new={quiet}><span class="thread-unread-dot" aria-hidden="true" /></span>;
  }
  if (!n) return null;
  return (
    <span class={mention ? "thread-unread mention" : "thread-unread"} title={said(n, mention)} aria-label={said(n, mention)} data-thread-unread={n}>
      <span class="thread-unread-dot" aria-hidden="true" />
      {mention ? `@${n}` : n}
    </span>
  );
}
