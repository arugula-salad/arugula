// The Overview's Board level (Arugula #1287): a board zoomed to in place,
// at `/p/:project/board/:board`. This is the shell's stub: the board's
// title, goal and lane bar, then its cards in flight, a row each, which
// zoom on to the card's Work level. #1288 fills it in (the lead card, the
// groups, the agents on each ticket) behind the same props.

import type { BoardRow, BoardView } from "../shared/proto";
import { epicLine, isCard, pointerLabel } from "../shared/boards/text";
import { EmptyLine } from "../shared/ui/block-states";
import { ExternalIcon } from "../shared/ui/icons";
import type { Fleet } from "../shell/fleet";
import { inFlight, type OvBoard, type OvCard } from "../shell/swarm/overview";
import { names } from "../platform/links";
import { follow, pathOf } from "../platform/router";
import { Word } from "./overview-row";

/** What the Overview gives its Board level. */
export interface BoardLevelProps {
  project: string;
  /** The board as the Overview groups it: its cards in Doing and Review
   * with the sessions on them, its counts, how it stands and its lead. */
  board: OvBoard;
  /** Its view as the platform has it: its goal, zones and every row. */
  view: BoardView;
  fleet: Fleet;
  /** Zoom in to a card's Work level, by its row's id. */
  zoomCard(row: string): void;
}

/** The Board level: the board, and its cards in flight to zoom in to. */
export function OverviewBoard({ project, board: b, view, zoomCard }: BoardLevelProps) {
  const e = epicLine({ done: b.done, total: b.total, proven: b.proven, review: b.review.length, doing: b.doing.length });
  const { shown } = inFlight(b, Infinity);
  const editor = pathOf({ page: "board", project, board: b.id }, names);
  return (
    <section class="ov-level" data-ov-level-board={b.id}>
      <header class="ov-level-head">
        <h2>{b.title}</h2>
        {view.board.goal && <p class="dim">{view.board.goal}</p>}
        <div class="board-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.total} aria-valuenow={b.done} aria-label={e.words}>
          {e.segments.map((s) => (
            <i key={s.lane} data-lane={s.lane} style={{ width: `${s.pct}%` }} />
          ))}
        </div>
        <p class="dim ov-level-sub">
          {e.words} ·{" "}
          <a href={editor} onClick={(ev) => follow(ev, editor)} data-ov-level-editor>
            Open on Boards <ExternalIcon size="xs" />
          </a>
        </p>
      </header>
      {shown.length === 0 ? (
        <EmptyLine data-ov-level-empty>Nothing in flight on this board.</EmptyLine>
      ) : (
        <ul class="ov-level-list" data-ov-level-cards>
          {shown.map((c) => (
            <TicketRow key={c.row.id} c={c} zoom={() => zoomCard(c.row.id)} />
          ))}
        </ul>
      )}
      {b.next > 0 && <p class="dim ov-level-sub">{b.next} in Next.</p>}
    </section>
  );
}

/** A card in flight: its number and title, how it stands and how many
 * sessions are on it; a click zooms to its Work level. */
function TicketRow({ c, zoom }: { c: OvCard; zoom: () => void }) {
  const n = c.sessions.length;
  return (
    <li>
      <button type="button" class="ov-level-row" data-ov-ticket={c.row.id} onClick={zoom}>
        <span class="mono dim">{pointerLabel(c.row.pointer, c.row.live)}</span>
        <span class="ov-level-row-title">{titleOf(c.row)}</span>
        <Word standing={c.standing} />
        <span class="dim">{n === 0 ? "no session" : n === 1 ? "1 session" : `${n} sessions`}</span>
      </button>
    </li>
  );
}

/** A card's title: its item's, else its note's first line. */
export function titleOf(r: BoardRow): string {
  return isCard(r.live) ? r.live.title : (r.note.split("\n").find((l) => l.trim()) ?? "");
}
