// `/p/:project/issues/:issue` (M201): one native issue, drawn by the
// issue view the shell's issue blocks draw too (../../shared/issues/view,
// M203), as the reader, with the way back to the project's list. Only here
// does it open on GitHub (M205).
//
// On a board, it is the board's card (Lift 5a, plan §3.4): its row's
// properties (the board, its lead, the worker and its machine and branch,
// the PR), the row's open ask as a NeedRow above Activity, and a line in
// Activity for each answer. The crumb climbs Boards › the board › #N.

import { useEffect, useState } from "preact/hooks";
import type { BoardRow, BoardView } from "../../shared/proto";
import { usePlatformApi } from "../../shared/platform-api";
import { sessionLabel } from "../../shared/chief-of-staff";
import { chat } from "../../shared/chat-store";
import type { Between } from "../../shared/chat/room";
import { askNeed } from "../../shared/boards/desk";
import { boards } from "../../shared/boards/store";
import { liveSessions, openPrs, prWord, waitingOf } from "../../shared/boards/text";
import { issues, linkedRepos, openOnGithub } from "../../shared/issues/store";
import { IssueApiContext, IssueView, type IssueApi } from "../../shared/issues/view";
import { Avatar } from "../../shared/kit/avatar";
import { NeedRow } from "../../shared/kit/need-row";
import { day } from "../../shared/time";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { where } from "../where";
import { mayWrite, membersOf, useIssuesOn } from "./issues";
import { Missing } from "./project";

export function IssuePage({ project, issue, msg, thread }: { project: string; issue: string; msg?: number; thread?: number }) {
  const on = useIssuesOn(project);
  const card = useCard(project, issue);
  if ("show" in on) return on.show;
  const p = on.p;
  const list = pathOf({ page: "issues", project: p.id }, names);
  const back = { href: list, label: "Issues", onClick: (e: MouseEvent) => follow(e, list), data: "issue-back" };
  return (
    <IssueApiContext.Provider value={here}>
      <IssueView key={issue} class="platform-section sw-page" place={{ project: p.id, members: membersOf(p), writer: mayWrite(p) }} id={issue} msg={msg} thread={thread} back={back} Missing={Missing} card={card ? { props: <CardProps c={card} />, ask: <OpenAsk c={card} />, between: answered(card.row) } : undefined} />
    </IssueApiContext.Provider>
  );
}

/** The platform's routes as the reader, and Open on GitHub (M205): GitHub's
 * authorization comes back to this page, so only it sends as the reader. */
const here: IssueApi = { ...issues, openOnGithub, repos: linkedRepos };

interface Card {
  view: BoardView;
  row: BoardRow;
  again: () => void;
}

/** The issue's row on one of the project's open boards, read with the rest of them (`boards.views`): again when its board changes. Null on no board. */
function useCard(project: string, issue: string): Card | null {
  const [card, setCard] = useState<Omit<Card, "again"> | null>(null);
  const [tick, setTick] = useState(0);
  const again = () => setTick((n) => n + 1);
  useEffect(() => {
    let live = true;
    boards.views(project).then(
      (vs) => {
        if (!live) return;
        for (const view of vs ?? []) {
          const row = view.zones.flatMap((z) => z.rows).find((r) => r.pointer.kind === "issue" && r.pointer.id === issue);
          if (row) return setCard({ view, row });
        }
        setCard(null);
      },
      () => live && setCard(null),
    );
    return () => void (live = false);
  }, [project, issue, tick]);
  // Its board says when it changed (an answer, a new worker).
  const board = card?.view.board.id;
  useEffect(() => (board ? chat.onObject(board, again) : undefined), [board]);
  useEffect(() => {
    if (card) where.issueOn(issue, { board: card.view.board.id, title: card.view.board.title });
  }, [issue, card?.view.board.id, card?.view.board.title]);
  return card && { ...card, again };
}

function Prop({ name, children, data }: { name: string; children: preact.ComponentChildren; data: string }) {
  return (
    <div class="issue-prop" {...{ [`data-card-${data}`]: "" }}>
      <dt>{name}</dt>
      <dd>{children}</dd>
    </div>
  );
}

/** The row's properties, as v6's card has them: its board, its lead, the worker and where it runs, its branch, its PR. */
function CardProps({ c }: { c: Card }) {
  const api = usePlatformApi();
  const { view, row } = c;
  const b = view.board;
  const to = pathOf({ page: "board", project: b.project, board: b.id }, names);
  const worker = liveSessions(row, waitingOf(row))[0];
  const block = row.attachments.find((a) => a.role === "work" && a.pointer.kind === "block" && a.block);
  const machine = block?.pointer.kind === "block" ? api.machine(block.pointer.machine)?.name : undefined;
  const pr = openPrs({ zones: [{ id: "", rev: 0, title: "", rows: [row] }] })[0];
  return (
    <>
      <Prop name="Board" data="board">
        <a href={to} onClick={(e) => follow(e, to)}>
          {b.title}
        </a>
      </Prop>
      {worker && (
        <Prop name="Worker" data="worker">
          <Avatar name={worker.name} agent /> {worker.name} <span class="dim">· {worker.word}</span>
        </Prop>
      )}
      {machine && (
        <Prop name="Machine" data="machine">
          {machine}
        </Prop>
      )}
      {b.lead && (
        <Prop name="Lead" data="lead">
          {sessionLabel(b.lead.session, api.projectName)}
        </Prop>
      )}
      {block?.block?.branch && (
        <Prop name="Branch" data="branch">
          <span class="m-id">{block.block.branch}</span>
        </Prop>
      )}
      {pr && (
        <Prop name="PR" data="pr">
          {pr.label} <span class="dim">· {prWord(pr.live)}</span>
        </Prop>
      )}
    </>
  );
}

/** The row's open ask, above Activity: answered in place, as on the board. */
function OpenAsk({ c }: { c: Card }) {
  const { toast } = usePlatformApi();
  const a = c.row.ask;
  if (!a || a.raised) return null;
  const answer = (x: { option: string } | { text: string }) =>
    boards.answer(c.view.board.id, a.id, x).then(c.again, (e: Error) => toast(`Couldn't answer it: ${e.message}`));
  return (
    <section class="card-ask" aria-label="Open ask" data-card-ask={a.id}>
      <h2>
        Open ask <span class={a.mine ? "k-needs" : "dim"}>{a.mine ? "asks you" : `asks ${a.who_name || "someone"}`}</span>
      </h2>
      <NeedRow need={askNeed(c.row, a, Date.now())} open onAnswer={(o) => void answer(o.id ? { option: o.id } : { text: o.label })} />
    </section>
  );
}

/** Each answer on the row, as a line in Activity: `aaron answered Truncate middle`. */
function answered(r: BoardRow): Between[] {
  return (r.asked ?? []).map((q) => ({
    key: `a${q.id}`,
    at: q.answered_at,
    el: (
      <div class="issue-event answered" data-issue-event="answered" data-card-answer={q.id} title={q.question}>
        <span class="issue-event-mark" aria-hidden="true">
          ◆
        </span>
        <span>
          <b>{q.answered_by}</b> answered <i>{q.answer}</i> <span class="dim">{day(q.answered_at)}</span>
        </span>
      </div>
    ),
  }));
}
