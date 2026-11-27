// The board's desk (Lift 5a, plan §3.3; "desk" is the code's word, never
// the screen's): what the Boards page draws around a board's lanes. On the
// left two thirds, the tabs (`1`–`4`: the board, its Docs, a PR opened from
// the fold), the red alert strip, the open PRs folded one row each, and the
// title block with its lead, machine and progress. On the right third, the
// board's Needs you as NeedRows (asks for a teammate folded under one row),
// then the lead's panel: the board's thread, which its lead reads.
//
// The lead's panel is a seam: Lift 4's Panel (#1480) takes it over, with
// the chief of staff's and a worker's tabs and `j`/`k`'s peek.

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { BoardAsk, BoardRow, BoardView, ChatRoom, CiAlerts } from "../proto";
import { chat } from "../chat-store";
import { platformFetch } from "../maintenance";
import { usePlatformApi } from "../platform-api";
import { sessionLabel } from "../chief-of-staff";
import { RoomBody } from "../chat/room";
import { useSubscribe } from "../ui/hooks";
import { Avatar } from "../kit/avatar";
import { NeedList, NeedRow, type Need } from "../kit/need-row";
import { Panel } from "../kit/panel";
import { Row } from "../kit/row";
import { StateMark } from "../kit/state-mark";
import { Tabs, type TabItem } from "../kit/tabs";
import { boards } from "./store";
import { prWord, redLine, stalledFor, type OpenPr } from "./text";

/** The tabs a board's number keys reach: Board, then three more. */
export const DESK_KEYS = 4;

/** The desk's tabs: the board, each linked Doc, and each PR opened from the fold (closable). */
export function deskTabs(view: BoardView, prs: OpenPr[]): TabItem[] {
  return [{ id: "board", title: "Board" }, ...(view.docs ?? []).map((d) => ({ id: d.id, title: d.title })), ...prs.map((p) => ({ id: p.id, title: p.label, closes: true }))];
}

export function DeskTabs({ tabs, current, pick, close }: { tabs: TabItem[]; current: string; pick: (id: string) => void; close: (id: string) => void }) {
  return (
    <div class="board-desk-tabs" data-board-tabs>
      <Tabs label="The board's tabs" tabs={tabs} current={current} onPick={pick} onClose={close} />
    </div>
  );
}

/** The project's CI alerts (Arugula #774): each branch that's red, as a red mark and edge (no fill), with its issue. Nothing when none is open or they can't be read. */
export function AlertStrip({ project, platform }: { project: string; platform: (path: string) => string | undefined }) {
  const [open, setOpen] = useState<CiAlerts["open"]>([]);
  useEffect(() => {
    let live = true;
    const read = () =>
      void platformFetch(`/api/projects/${encodeURIComponent(project)}/ci-alerts`, { credentials: "same-origin" })
        .then((r) => (r.ok ? (r.json() as Promise<CiAlerts>) : null))
        .then((a) => live && setOpen(a?.on ? a.open : []))
        .catch(() => {});
    read();
    const back = () => document.visibilityState === "visible" && read();
    document.addEventListener("visibilitychange", back);
    return () => {
      live = false;
      document.removeEventListener("visibilitychange", back);
    };
  }, [project]);
  if (!open.length) return null;
  return (
    <div class="board-alerts" data-board-alerts>
      {open.map((a) => {
        const href = platform(`/p/${project}/issues/${a.issue}`);
        return (
          <div key={a.issue} class="board-alert" role="alert" data-board-alert={a.branch}>
            <StateMark state="failing" word={redLine(a)} />
            {href && (
              <a class="k-button is-plain" href={href} data-board-alert-issue={a.issue}>
                Open #{a.number}
              </a>
            )}
          </div>
        );
      })}
    </div>
  );
}

/** The board's open PRs, one row each above its lanes: its number and title, then its queue or state and the row it's on. A click opens it as a tab. */
export function PrFold({ prs, open }: { prs: OpenPr[]; open: (pr: OpenPr) => void }) {
  if (!prs.length) return null;
  return (
    <div class="board-prs" aria-label="Open PRs" data-board-prs>
      {prs.map((p) => (
        <Row
          key={p.id}
          mark={p.live.state === "checks_failed" ? "failing" : p.live.state === "checks_running" ? "working" : "waits"}
          label="PR"
          title={`${p.label} ${p.live.title}`}
          meta={[prWord(p.live)]}
          onClick={() => open(p)}
          data-board-pr={p.id}
        />
      ))}
    </div>
  );
}

/** An ask on a row, as the A2P row draws it. */
function askNeed(r: BoardRow, a: BoardAsk, now: number): Need {
  return {
    kind: "ask",
    title: a.question,
    from: { name: a.asked_by },
    for: a.mine ? "you" : a.who_name || "someone",
    subject: r.live ? { kind: "issue", id: `#${r.live.number}` } : undefined,
    lines: a.why ? [a.why] : undefined,
    answer: a.can_answer ? { via: "card", options: a.options.map((o) => ({ id: o, label: o === a.recommend ? `${o} · recommended` : o })) } : undefined,
    age: stalledFor(a.created_at, now),
  };
}

/** The board's Needs you, at the top of the right third: the asks for you as NeedRows, the first open; the asks for teammates folded under one row (`1 for Jake`). */
export function BoardNeeds({ view, answer }: { view: BoardView; answer: (r: BoardRow, a: { option: string } | { text: string }) => Promise<boolean> }) {
  const asked = view.zones.flatMap((z) => z.rows).filter((r) => r.ask && !r.ask.raised);
  const mine = asked.filter((r) => r.ask!.mine);
  const others = asked.filter((r) => !r.ask!.mine);
  const [open, setOpen] = useState<string | null>(null);
  const [folded, setFolded] = useState(true);
  const now = Date.now();
  const shown = open ?? mine[0]?.id ?? null;
  const names = [...new Set(others.map((r) => r.ask!.who_name || "someone"))];
  const row = (r: BoardRow) => (
    <div key={r.id} data-board-ask={r.ask!.id} data-board-ask-row={r.id}>
      <NeedRow
        need={askNeed(r, r.ask!, now)}
        open={shown === r.id}
        onToggle={() => setOpen(shown === r.id ? "" : r.id)}
        onAnswer={(o) => void answer(r, o.id ? { option: o.id } : { text: o.label })}
      />
    </div>
  );
  return (
    <section class="board-needs" aria-label="Needs you" data-board-needs={mine.length}>
      <h2 class="board-needs-head">
        Needs you <span class="m-meta">{mine.length}</span>
      </h2>
      {mine.length === 0 && others.length === 0 ? (
        <p class="m-meta board-needs-none" data-board-needs-none>
          Nothing here needs you.
        </p>
      ) : (
        <NeedList label="Needs you on this board">
          {mine.map(row)}
          {others.length > 0 && (
            <Row title={`${others.length} ${others.length === 1 ? "ask" : "asks"} for ${names.length === 1 ? names[0] : "others"}`} mark="waits" open={!folded} onClick={() => setFolded(!folded)} data-board-needs-others={others.length}>
              {others.map(row)}
            </Row>
          )}
        </NeedList>
      )}
    </section>
  );
}

/** The lead's panel (a seam for Lift 4's Panel): who leads and where, then the board's thread, which its lead reads, with the box to write to it. */
export function LeadPanel({ view, id, phone }: { view: BoardView; id: string; phone: boolean }) {
  const api = usePlatformApi();
  useSubscribe((fn) => chat.subscribe(fn));
  useSubscribe((fn) => api.ownPane?.watch(fn) ?? (() => {}));
  const lead = view.board.lead;
  const machine = lead ? lead.machine_name || api.machine(lead.machine)?.name || "" : "";
  const name = lead ? sessionLabel(lead.session, api.projectName) : "No lead yet";
  const head = (
    <div class="board-lead-head" data-board-lead={lead?.session ?? ""}>
      <Avatar name={name} agent />
      <span class="board-lead-who">
        <b>{view.board.title}</b>
        <span class="m-meta">{lead ? ["lead", name, machine].filter(Boolean).join(" · ") : name}</span>
      </span>
      {lead?.pane !== undefined && api.ownPane?.here(lead.machine, lead.pane) && (
        <button type="button" class="k-button is-plain" title="Its pane, on its machine" data-board-lead-open onClick={() => api.goToPane(lead.machine, String(lead.pane), null)}>
          Terminal
        </button>
      )}
    </div>
  );
  return (
    <Panel label="The board's lead" head={head}>
      <LeadThread id={id} phone={phone} name={lead ? "the lead" : "the board's thread"} />
    </Panel>
  );
}

function LeadThread({ id, phone, name }: { id: string; phone: boolean; name: string }): ComponentChildren {
  const [room, setRoom] = useState<ChatRoom | null>(null);
  const [error, setError] = useState<string | null>(null);
  const on = chat.on;
  useEffect(() => {
    if (!on) return;
    let live = true;
    boards.thread(id).then(
      (r) => live && setRoom(r),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [id, on]);
  if (on === false) return <p class="m-meta">The lead's thread comes with chat, which is off for you.</p>;
  if (!room) return <p class="m-meta">{error ? `The thread didn't load: ${error}` : "Loading the thread…"}</p>;
  return (
    <div class="chat-thread board-lead-thread" data-board-thread={room.id}>
      <RoomBody key={room.id} room={room} phone={phone} embedded name={name} empty={{ title: "Nothing said to the lead yet.", why: "Everyone in the project reads it. Write @name to notify someone." }} />
    </div>
  );
}

