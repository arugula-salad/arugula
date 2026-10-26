// An agent session's own page (M257): `/p/:project/sessions/ses_…`, the
// card a thread shows, larger, and the session's history, each entry
// saying whether its box reported it (signed) or the platform recorded it.
// A link to it unfurls in chat as a preview card (M254).

import { useState } from "preact/hooks";
import { SessionCardView } from "../../shared/chat/session-card";
import type { SessionView } from "../../shared/proto";
import { roomPath } from "../links";
import { follow } from "../router";
import { session as platform } from "../session";
import { Missing } from "./project";

const when = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

export function SessionPage({ project, session }: { project: string; session: string }) {
  const [view, setView] = useState<SessionView | null>(null);
  if (!platform.project(project)) return <Missing what="That project isn't one of yours (or it's gone)." />;
  if (view && view.card.project !== project) return <Missing what="That session is in another project." />;
  const c = view?.card;
  const thread = c ? roomPath(c.room, undefined, c.parent > 0 ? c.parent : undefined) : null;
  return (
    <section class="platform-section sw-page session-page" data-session-page-of={session}>
      {thread && (
        <p class="issue-back">
          <a href={thread} onClick={(e) => follow(e, thread)} data-session-thread>
            ‹ Its thread
          </a>
        </p>
      )}
      <h1>Agent session</h1>
      <SessionCardView ses={session} large onView={setView} />
      {view && (
        <>
          <h2>
            History <span class="dim">{view.events.length}</span>
          </h2>
          <ol class="session-history" data-session-history>
            {view.events.map((e) => (
              <li key={e.seq} data-session-event={e.seq}>
                <time title={new Date(e.at).toLocaleString()}>{when(e.at)}</time>
                <span>{e.text}</span>
                <span class="signed" title={e.signed ? "Its box reported it, signed" : "The platform recorded it"}>
                  {e.signed ? "box, signed" : "platform"}
                </span>
              </li>
            ))}
          </ol>
        </>
      )}
    </section>
  );
}
