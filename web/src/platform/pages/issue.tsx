// `/p/:project/issues/:issue` (M201): one native issue, drawn by the
// issue view the shell's issue blocks draw too (../../shared/issues/view,
// M203), as the reader, with the way back to the project's list. Only here
// does it open on GitHub (M205).
//
// On a board, it is the board's card (Lift 5a, plan §3.4): its row's
// properties (the board, its lead, the worker and its machine and branch,
// the PR), the row's open ask as a NeedRow above Activity, with the agents'
// asks and to-dos on the issue from the project's Inbox (Arugula #1469), and
// a line in Activity for each answer: `aaron answered Squash`, `aaron did
// Rotate the key · note`, the note as the record keeps it. The crumb climbs
// Boards › the board › #N.

import { useEffect, useState } from "preact/hooks";
import { panelKey, usePanel } from "../../shared/kit/panel";
import type { BoardRow, BoardView } from "../../shared/proto";
import { usePlatformApi } from "../../shared/platform-api";
import { shownSession } from "../../shared/chief-of-staff";
import { chat } from "../../shared/chat-store";
import type { Between } from "../../shared/chat/room";
import { askNeed, workerOf } from "../../shared/boards/desk";
import { answeredWords, boardItems, itemFor, itemNeed } from "../../shared/boards/text";
import { boards } from "../../shared/boards/store";
import { liveSessions, openPrs, prWord, waitingOf } from "../../shared/boards/text";
import { issues, linkedRepos, openOnGithub } from "../../shared/issues/store";
import { IssueApiContext, IssueView, type IssueApi } from "../../shared/issues/view";
import { Avatar } from "../../shared/kit/avatar";
import { PrMark, prMarkOf } from "../../shared/kit/pr-mark";
import { NeedList, NeedRow } from "../../shared/kit/need-row";
import { day } from "../../shared/time";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { where } from "../where";
import { mayWrite, membersOf, useIssuesOn } from "./issues";
import { answerItem, memberName, useProjectInbox } from "../inbox";
import { session } from "../session";
import { Missing } from "./project";

export function IssuePage({ project, issue, msg, thread }: { project: string; issue: string; msg?: number; thread?: number }) {
  const on = useIssuesOn(project);
  const card = useCard(project, issue);
  const AgentPanel = usePanel();
  if ("show" in on) return on.show;
  const p = on.p;
  const list = pathOf({ page: "issues", project: p.id }, names);
  const back = { href: list, label: "Issues", onClick: (e: MouseEvent) => follow(e, list), data: "issue-back" };
  const view = (
    <IssueApiContext.Provider value={here}>
      <IssueView key={issue} class="platform-section sw-page" place={{ project: p.id, members: membersOf(p), writer: mayWrite(p) }} id={issue} msg={msg} thread={thread} back={back} Missing={Missing} card={card ? { props: <CardProps c={card} />, ask: <OpenAsk c={card} project={project} />, between: answered(card.row) } : undefined} />
    </IssueApiContext.Provider>
  );
  // A card (an issue on a board) has the agent panel beside it, its worker in front (#1520).
  if (!card || !AgentPanel) return view;
  const worker = workerOf(card.row, card.view.board.id);
  return (
    <div class="card-place" data-card-place={issue}>
      <div class="card-place-work" data-work tabIndex={-1}>
        {view}
      </div>
      <AgentPanel project={p.id} targets={worker ? [worker] : []} current={worker ? panelKey(worker) : undefined} />
    </div>
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
          {shownSession(b.lead.session)}
        </Prop>
      )}
      {block?.block?.branch && (
        <Prop name="Branch" data="branch">
          <span class="m-id">{block.block.branch}</span>
        </Prop>
      )}
      {pr && (
        <Prop name="PR" data="pr">
          <PrMark {...prMarkOf(pr.live)} /> {pr.label} <span class="dim">· {prWord(pr.live)}</span>
        </Prop>
      )}
    </>
  );
}

/** The row's open ask, above Activity, and the agents' asks and to-dos on its issue (Arugula #1469): answered in place, as on the board. A teammate's is neutral and says who it waits on. */
function OpenAsk({ c, project }: { c: Card; project: string }) {
  const { toast } = usePlatformApi();
  const [tick, setTick] = useState(0);
  const inbox = useProjectInbox(project, `${c.row.id}:${tick}`);
  const me = session.me?.account ?? null;
  const items = inbox ? (boardItems([c.row], inbox.items ?? []).get(c.row.id) ?? []) : [];
  const a = c.row.ask && !c.row.ask.raised ? c.row.ask : null;
  if (!a && !items.length) return null;
  const answer = (x: { option: string } | { text: string }) =>
    boards.answer(c.view.board.id, a!.id, x).then(c.again, (e: Error) => toast(`Couldn't answer it: ${e.message}`));
  const answerOne = (id: string, choice: string, note?: string) =>
    answerItem(id, choice, note).then(
      () => (setTick((n) => n + 1), c.again()),
      (e: Error) => toast(`Couldn't answer it: ${e.message}`),
    );
  const first = a ? (a.mine ? "you" : a.who_name || "someone") : itemFor(items[0]!, me, memberName);
  const now = Date.now();
  return (
    <section class="card-ask" aria-label="Open ask" data-card-ask={a?.id ?? items[0]!.id}>
      <h2>
        Open ask <span class={first === "you" ? "k-needs" : "dim"}>{first === "you" ? "asks you" : `asks ${first}`}</span>
      </h2>
      <NeedList label="Open on this card">
        {a && <NeedRow need={askNeed(c.row, a, now)} open onAnswer={(o) => void answer(o.id ? { option: o.id } : { text: o.label })} />}
        {items.map((i) => {
          const need = itemNeed(i, itemFor(i, me, memberName), c.row.live?.number, now);
          return (
            <div key={i.id} data-card-item={i.id}>
              <NeedRow
                need={need}
                item={i.id}
                open
                onAnswer={need.answer ? (o, note) => void answerOne(i.id, o.id, note) : undefined}
                extra={
                  need.answer || !i.subject.url ? undefined : (
                    <a class="k-button is-plain" href={i.subject.url} onClick={(e) => i.subject.url.startsWith("/") && follow(e, i.subject.url)}>
                      Open
                    </a>
                  )
                }
              />
            </div>
          );
        })}
      </NeedList>
    </section>
  );
}

/** Each answer on the row, as a line in Activity: `aaron answered Truncate middle`, `aaron did Rotate the key · done in 1Password` (a to-do's note, from its record). */
function answered(r: BoardRow): Between[] {
  return (r.asked ?? []).map((q) => {
    const w = answeredWords(q);
    return {
      key: `a${q.id}`,
      at: q.answered_at,
      el: (
        <div class="issue-event answered" data-issue-event="answered" data-card-answer={q.id} data-card-answer-kind={q.kind ?? "ask"} title={q.question}>
          <span class="issue-event-mark" aria-hidden="true">
            ◆
          </span>
          <span>
            <b>{q.answered_by}</b> {w.verb} <i>{w.what}</i>
            {w.note && <span data-card-answer-note> · {w.note}</span>} <span class="dim">{day(q.answered_at)}</span>
          </span>
        </div>
      ),
    };
  });
}
