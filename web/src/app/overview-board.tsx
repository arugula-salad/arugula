// The Overview's Board level (Arugula #1287, #1288): a board zoomed to in
// place, at `/p/:project/board/:board`. Its title, goal and a big lane bar;
// its lead's card, with Talk to the lead (that agent's tab) and Board
// thread as two buttons, or "no lead" and Run this board, which puts the
// board's card in Needs you (#1416); then its open tickets as the board groups them, each with its
// number and title, its PR or check line, one word of how it stands and
// the agents on it as chips (sessions by card, #1082). Done tickets fold
// into one line. A ticket zooms on to its Work level; a chip opens that
// agent's tab in the talk panel, as a lead's name does (#1290).

import { useState } from "preact/hooks";
import type { BoardRow, BoardView } from "../shared/proto";
import { chipOf, epicLine, inLaneOrder, isCard, pointerLabel, sections } from "../shared/boards/text";
import { EmptyLine } from "../shared/ui/block-states";
import { ExternalIcon } from "../shared/ui/icons";
import type { Fleet } from "../shell/fleet";
import { shortHost } from "../shell/swarm/model";
import { ticketWord, wordOf, type OvBoard, type OvCard, type OvSession } from "../shell/swarm/overview";
import { names } from "../platform/links";
import { follow, pathOf } from "../platform/router";

/** What the Overview gives its Board level. */
export interface BoardLevelProps {
  project: string;
  /** The board as the Overview groups it: its cards in Doing and Review
   * with the sessions on them, its counts, how it stands and its lead. */
  board: OvBoard;
  /** Its view as the platform has it: its goal, groups and every row. */
  view: BoardView;
  fleet: Fleet;
  /** Zoom in to a card's Work level, by its row's id. */
  zoomCard(row: string): void;
  /** Open a session's tab in the talk panel: the lead's, or an agent's on a ticket. */
  talkTo(s: OvSession): void;
  /** The teammate whose machine the lead runs on, to name it with them. */
  leadWho?: string;
  /** How many of the project's open boards its lead leads. */
  leads: number;
  /** The board's thread in the talk panel, while chat is on. */
  thread?(): void;
  /** Run this board (a board no one leads): its card goes in Needs you. */
  run?(): void;
}

/** The Board level: the board, its lead, and its open tickets to zoom in to. */
export function OverviewBoard({ project, board: b, view, zoomCard, talkTo, leadWho, leads, thread, run }: BoardLevelProps) {
  const e = epicLine({ done: b.done, total: b.total, proven: b.proven, review: b.review.length, doing: b.doing.length });
  // Done tickets fold into one line; their rows are drawn once it's unfolded.
  const [unfolded, setUnfolded] = useState(false);
  const editor = pathOf({ page: "board", project, board: b.id }, names);
  const cards = new Map([...b.doing, ...b.review].map((c) => [c.row.id, c]));
  const groups = sections(view).map((s) => ({ title: s.group?.title ?? null, rows: s.lanes.flatMap((z) => z.rows).filter((r) => r.lane !== "done") }));
  const grouped = (view.groups ?? []).length > 0;
  const open = groups.filter((g) => g.rows.length);
  const done = inLaneOrder(view.zones).flatMap((z) => z.rows).filter((r) => r.lane === "done");
  const ticket = (r: BoardRow) => <TicketRow key={r.id} r={r} c={cards.get(r.id)} project={project} board={b.id} zoom={() => zoomCard(r.id)} talkTo={talkTo} />;
  return (
    <section class="ov-level" data-ov-level-board={b.id}>
      <div class="ov-bl-head">
        <header class="ov-level-head">
          <h2>{b.title}</h2>
          {view.board.goal && (
            <p class="ov-bl-goal" data-ov-level-goal>
              {view.board.goal}
            </p>
          )}
          <div class="board-bar ov-bl-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.total} aria-valuenow={b.done} aria-label={e.words}>
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
        <LeadCard b={b} who={leadWho} leads={leads} talkTo={talkTo} thread={thread} run={run} />
      </div>
      {open.length === 0 ? (
        <EmptyLine data-ov-level-empty>Nothing open on this board.</EmptyLine>
      ) : (
        open.map((g, i) => (
          <section key={g.title ?? `-${i}`} class="ov-bl-group" data-ov-group={g.title ?? ""}>
            {grouped && (
              <h3>
                <span>{g.title ?? "No group"}</span>
                <span class="dim">{g.rows.length} open</span>
              </h3>
            )}
            <ul class="ov-bl-tickets">{g.rows.map(ticket)}</ul>
          </section>
        ))
      )}
      {done.length > 0 && (
        <details class="ov-bl-done" data-ov-level-done onToggle={(ev) => setUnfolded((ev.currentTarget as HTMLDetailsElement).open)}>
          <summary class="dim">{done.length === 1 ? "1 done ticket" : `${done.length} done tickets`}</summary>
          {unfolded && <ul class="ov-bl-tickets">{done.map(ticket)}</ul>}
        </details>
      )}
    </section>
  );
}

/** Who leads the board: its name, how it stands, its machine and how many
 * boards it leads, with Talk to the lead and Board thread apart; or that
 * nobody does, and Run this board. */
function LeadCard({ b, who, leads, talkTo, thread, run }: { b: OvBoard; who?: string; leads: number; talkTo: (s: OvSession) => void; thread?: () => void; run?: () => void }) {
  const l = b.lead;
  const threadButton = thread && (
    <button type="button" class="secondary small" data-ov-lead-thread onClick={thread}>
      Board thread
    </button>
  );
  if (!l)
    return (
      <aside class="ov-bl-lead" data-ov-lead-card="">
        <span class="dim ov-bl-lead-label">Lead</span>
        <b class="dim">No lead</b>
        <span class="dim ov-bl-lead-sub">No session leads this board.</span>
        <div class="ov-bl-lead-acts">
          {run && (
            <button type="button" class="primary small" data-ov-run-board onClick={run}>
              Run this board
            </button>
          )}
          {threadButton}
        </div>
      </aside>
    );
  const name = who ? `${who}'s ${l.session}` : l.session;
  const says = l.standing ? wordOf({ standing: l.standing, asks: who }) : "";
  return (
    <aside class="ov-bl-lead" data-ov-lead-card={l.session}>
      <span class="dim ov-bl-lead-label">Lead</span>
      <b class="ov-bl-lead-name">
        <i class="ov-lead-dot" data-ov-lead-standing={l.standing ?? undefined} aria-hidden="true" />
        {name}
      </b>
      <span class="dim ov-bl-lead-sub" data-ov-lead-sub>
        {[l.machineName && `on ${l.machineName}`, says, leads > 1 ? `leads ${leads} boards` : "leads this board"].filter(Boolean).join(" · ")}
      </span>
      <div class="ov-bl-lead-acts">
        <button type="button" class="primary small" data-ov-lead-talk onClick={() => talkTo(l.peek)}>
          Talk to the lead
        </button>
        {threadButton}
      </div>
    </aside>
  );
}

/** A ticket's PR or check line: its own PR's, else the first PR attached as its work, as the board's chip says it. */
function prLine(r: BoardRow): string | null {
  const own = r.live?.kind === "pr" ? chipOf({ pointer: r.pointer, live: r.live, block: null, role: "work" }) : null;
  const att = own ? null : r.attachments.find((a) => a.role === "work" && !a.released && a.live?.kind === "pr");
  const chip = own ?? (att && chipOf(att));
  return chip ? [chip.label, chip.state].filter(Boolean).join(" · ") : null;
}

/** An open ticket: its number and title, its PR line, its word and the
 * agents on it; a click anywhere but a chip zooms to its Work level. */
function TicketRow({ r, c, project, board, zoom, talkTo }: { r: BoardRow; c?: OvCard; project: string; board: string; zoom: () => void; talkTo: (s: OvSession) => void }) {
  const { word, asks } = ticketWord(r, c);
  const sessions = c?.sessions ?? [];
  const line = prLine(r) ?? (r.lane !== "done" && !sessions.length ? "no agent yet" : null);
  const to = pathOf({ page: "project", project, zoom: { board, card: r.id } }, names);
  // `owner/repo#N` in full for the title; the column says `repo#N`, as a PR's chip does.
  const pointer = pointerLabel(r.pointer, r.live);
  return (
    <li class="ov-bl-ticket" data-ov-ticket={r.id} data-ov-ticket-lane={r.lane} onClick={(ev) => !(ev.target as Element).closest("a, button") && !getSelection()?.toString() && zoom()}>
      <span class="mono dim ov-bl-num" title={pointer} data-ov-ticket-num>
        {pointer.replace(/^[^#/]*\//, "")}
      </span>
      <span class="ov-bl-what">
        <a class="ov-bl-title" href={to} onClick={(ev) => follow(ev, to)} title={`Zoom in to ${titleOf(r)}`} data-ov-ticket-title>
          {titleOf(r)}
        </a>
        {line && (
          <span class="dim ov-bl-pr" data-ov-ticket-pr>
            {line}
          </span>
        )}
      </span>
      <span class={`ov-bl-word${asks ? " ov-bl-asks" : ""}`} data-ov-ticket-word>
        {word}
      </span>
      <span class="ov-bl-agents">
        {sessions.map((s) => (
          <AgentChip key={s.key} s={s} talk={() => talkTo(s)} />
        ))}
      </span>
    </li>
  );
}

/** An agent on a ticket: a dot for how it stands, its name and machine; it opens that agent's tab. */
function AgentChip({ s, talk }: { s: OvSession; talk: () => void }) {
  const says = wordOf(s);
  const title = `${s.name} on ${s.where}${says ? `, ${says}` : ""}: talk to it`;
  return (
    <button type="button" class="ov-lead" title={title} aria-label={title} data-ov-agent-chip={s.key} onClick={talk}>
      <i class="ov-lead-dot" data-ov-lead-standing={s.standing === "idle" ? undefined : s.standing} aria-hidden="true" />
      <span class="ov-lead-name">{s.name}</span>
      <span class="ov-lead-machine">{shortHost(s.where)}</span>
    </button>
  );
}

/** A card's title: its item's, else its note's first line. */
export function titleOf(r: BoardRow): string {
  return isCard(r.live) ? r.live.title : (r.note.split("\n").find((l) => l.trim()) ?? "");
}
