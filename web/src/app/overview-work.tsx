// The Overview's Work level (Arugula #1287): a card's work zoomed to in
// place, at `/p/:project/board/:board/card/:row`, or a session on no card,
// at `/p/:project/work/:session`. This is the shell's stub: the card (or
// the session) and the sessions on it, a row each, with Talk (its tab in
// the panel) and Open in Panes ↗. #1289 fills it in with their live
// terminals side by side, behind the same props.

import type { BoardRow } from "../shared/proto";
import { pointerLabel } from "../shared/boards/text";
import { EmptyLine } from "../shared/ui/block-states";
import { ExternalIcon } from "../shared/ui/icons";
import type { Fleet } from "../shell/fleet";
import { shortHost } from "../shell/swarm/model";
import type { OvSession, Standing } from "../shell/swarm/overview";
import { titleOf } from "./overview-board";
import { peekKey } from "./overview-peek";
import { Word } from "./overview-row";

/** What the Overview gives its Work level. */
export interface WorkLevelProps {
  project: string;
  fleet: Fleet;
  /** The card zoomed to, with the board it's on and how it stands; null
   * for a session on no card. */
  card: { board: string; boardTitle: string; row: BoardRow; standing: Standing } | null;
  /** The sessions working on it: the card's, or the one on no card. */
  sessions: OvSession[];
  /** The session whose tab is in front of the talk panel, by its peek's
   * key (`peekKey`): its pane is the one outlined. */
  talking: string | null;
  /** Bring a session's tab to the front of the talk panel. */
  talkTo(s: OvSession): void;
  /** A session on its machine's page, on its pane (or a cloud
   * workspace's on its own). */
  openPanes(s: OvSession): void;
}

/** The Work level: what's working on a card (or the session on no card). */
export function OverviewWork({ card, sessions, talking, talkTo, openPanes }: WorkLevelProps) {
  const lone = card ? null : sessions[0];
  return (
    <section class="ov-level" data-ov-level-work={card ? card.row.id : lone ? peekKey(lone) : ""}>
      <header class="ov-level-head">
        <h2>{card ? [pointerLabel(card.row.pointer, card.row.live), titleOf(card.row)].filter(Boolean).join(" ") : lone ? `${lone.name} on ${shortHost(lone.where)}` : "A session"}</h2>
        <p class="dim ov-level-sub">
          {card ? (
            <>
              {card.boardTitle} <Word standing={card.standing} />
            </>
          ) : (
            "Not on a card"
          )}
        </p>
      </header>
      {sessions.length === 0 ? (
        <EmptyLine data-ov-level-empty>{card ? "No session is working on this card yet." : "This session isn't in the project now."}</EmptyLine>
      ) : (
        <ul class="ov-level-list" data-ov-level-sessions>
          {sessions.map((s) => {
            const k = peekKey(s);
            const far = !s.lead && !s.hosted;
            return (
              <li key={s.key} class={`ov-level-pane${talking === k ? " ov-level-pane-talking" : ""}`} data-ov-work-session={k} data-ov-talking={talking === k ? "" : undefined}>
                <b>{s.name}</b>
                <Word standing={s.standing} />
                <span class="dim mono">
                  {shortHost(s.where)}
                  {s.branch ? ` · ${s.branch}` : ""}
                </span>
                <span class="ov-level-pane-acts">
                  <button type="button" class="secondary" data-ov-work-talk onClick={() => talkTo(s)}>
                    Talk
                  </button>
                  <button type="button" class="link" disabled={far} data-ov-work-panes onClick={() => openPanes(s)}>
                    Open in Panes <ExternalIcon size="xs" />
                  </button>
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
