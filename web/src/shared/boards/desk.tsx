// The board's desk (Lift 5a, plan §3.3; "desk" is the code's word, never
// the screen's): what the Boards page draws around a board's lanes. On the
// left two thirds, the tabs (`1`–`6`: the board's own, then those opened
// beside it, Lift 5b's ./arranged.tsx), the red alert strip, the open PRs folded one row each, and the
// title block with its lead, machine and progress. On the right third, the
// board's Needs you as NeedRows (its asks, and the agents' asks and to-dos
// on its issues from the project's Inbox, Arugula #1469; a teammate's
// folded under one neutral row each),
// then the lead's panel: the board's thread, which its lead reads.
//
// The lead's panel is a seam: Lift 4's Panel (#1480) takes it over, with
// the chief of staff's and a worker's tabs and `j`/`k`'s peek.

import { createContext, type ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { BoardAsk, BoardRow, BoardView, ChatRoom, CiAlerts, InboxItem } from "../proto";
import { chat } from "../chat-store";
import { platformFetch } from "../maintenance";
import { usePlatformApi } from "../platform-api";
import { sessionLabel } from "../chief-of-staff";
import { RoomBody } from "../chat/room";
import { useSubscribe } from "../ui/hooks";
import { Avatar } from "../kit/avatar";
import { NeedList, NeedRow, type Need, type NeedOption } from "../kit/need-row";
import { Panel } from "../kit/panel";
import { Row } from "../kit/row";
import { StateMark } from "../kit/state-mark";
import { Tabs, type TabItem } from "../kit/tabs";
import { boards } from "./store";
import { boardItems, forTeammateFold, itemFor, itemNeed, itemWord, prWord, redLine, stalledFor, type OpenPr } from "./text";

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
export function askNeed(r: BoardRow, a: BoardAsk, now: number): Need {
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

/** The project's Inbox, for a board: the agents' items on its rows' issues (Arugula #1469), who reads, and how they answer. Null where the page has no Inbox (the shell's block: board asks alone). Read again when the stream says the Inbox changed, and after an answer. */
export interface BoardInbox {
  byRow: Map<string, InboxItem[]>;
  /** All of the project's items, for those not on a row (a tab an agent opened for you). */
  items: InboxItem[];
  me: string | null;
  name: (account: string) => string;
  answer: (i: InboxItem, choice: string, note?: string) => Promise<boolean>;
}

export function useBoardInbox(view: BoardView | null): BoardInbox | null {
  const api = usePlatformApi();
  useSubscribe((fn) => chat.subscribe(fn));
  const said = chat.inbox?.heard ?? 0;
  const project = view?.board.project;
  const [read, setRead] = useState<{ project: string; items: InboxItem[] } | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!api.inbox || !project) return;
    let live = true;
    api.inbox.read(project).then(
      (v) => live && setRead({ project, items: v.items ?? [] }),
      () => {},
    );
    return () => void (live = false);
  }, [project, said, tick]);
  const inbox = api.inbox;
  if (!inbox || !view || !read || read.project !== project) return null;
  return {
    byRow: boardItems(
      view.zones.flatMap((z) => z.rows),
      read.items,
    ),
    items: read.items,
    me: inbox.me(),
    name: inbox.name,
    answer: (i, choice, note) =>
      inbox.answer(i.id, choice, note).then(
        () => (setTick((n) => n + 1), true),
        (e: Error) => (api.toast(`Couldn't answer it: ${e.message}`), false),
      ),
  };
}

/** Each row's word for the agents' items on it, for its tile: the reader's first ("asks you", "waits on you"), else a teammate's ("asks Jake"). */
export const RowItems = createContext<Map<string, { word: string; mine: boolean }>>(new Map());

export function rowItemWords(inbox: BoardInbox | null): Map<string, { word: string; mine: boolean }> {
  const out = new Map<string, { word: string; mine: boolean }>();
  if (!inbox) return out;
  for (const [row, items] of inbox.byRow) {
    const whos = items.map((i) => ({ i, who: itemFor(i, inbox.me, inbox.name) }));
    const pick = whos.find((x) => x.who === "you") ?? whos[0];
    if (pick) out.set(row, { word: itemWord(pick.i, pick.who), mine: pick.who === "you" });
  }
  return out;
}

/** One row of the board's Needs you: a board ask or an agent's item, and who it waits on. */
interface Asked {
  key: string;
  who: string;
  since: number;
  need: Need;
  answer?: (o: NeedOption, note?: string) => void;
  /** Where it's answered, when not here: its card, its session. */
  open?: string;
  /** Or the tab it's in, on this board. */
  go?: () => void;
  data: Record<string, string>;
}

/** The board's Needs you, at the top of the right third: what waits on you as NeedRows (its asks, and the agents' asks and to-dos on its issues, oldest first), the first open; a teammate's folded under one neutral row each (`· 1 for Jake ›`). */
export function BoardNeeds({ view, answer, inbox, platform, tabs = [] }: { view: BoardView; answer: (r: BoardRow, a: { option: string } | { text: string }) => Promise<boolean>; inbox: BoardInbox | null; platform: (path: string) => string | undefined; tabs?: { item: InboxItem; n: number; go: () => void }[] }) {
  const [open, setOpen] = useState<string | null>(null);
  const [unfolded, setUnfolded] = useState<ReadonlySet<string>>(new Set());
  const now = Date.now();
  const all: Asked[] = [];
  for (const r of view.zones.flatMap((z) => z.rows)) {
    const a = r.ask;
    if (a && !a.raised)
      all.push({
        key: a.id,
        who: a.mine ? "you" : a.who_name || "someone",
        since: a.created_at,
        need: askNeed(r, a, now),
        answer: (o) => void answer(r, o.id ? { option: o.id } : { text: o.label }),
        data: { "data-board-ask": a.id, "data-board-ask-row": r.id },
      });
    for (const i of inbox?.byRow.get(r.id) ?? []) {
      const who = itemFor(i, inbox!.me, inbox!.name);
      const need = itemNeed(i, who, r.live?.number, now);
      all.push({
        key: i.id,
        who,
        since: i.since,
        need,
        answer: need.answer ? (o, note) => void inbox!.answer(i, o.id, note) : undefined,
        // A teammate's card has no address: only its person opens it (Arugula #1469).
        open: need.answer || !i.subject.url ? undefined : i.subject.url.startsWith("/") ? platform(i.subject.url) : i.subject.url,
        data: { "data-board-item": i.id, "data-board-ask-row": r.id },
      });
    }
  }
  // What an agent put in a tab for you (Lift 5b): its row points at the tab, and only you go there.
  for (const t of tabs) {
    const who = inbox ? itemFor(t.item, inbox.me, inbox.name) : "you";
    all.push({
      key: t.item.id,
      who,
      since: t.item.since,
      need: { ...itemNeed(t.item, who, undefined, now), kind: t.n > 0 ? `tab ${t.n}` : "tab" },
      go: t.n > 0 ? t.go : undefined,
      data: { "data-board-item": t.item.id, "data-board-for-you": t.item.subject.tab ?? "" },
    });
  }
  all.sort((a, b) => a.since - b.since);
  const mine = all.filter((x) => x.who === "you");
  const theirs = new Map<string, Asked[]>();
  for (const x of all) if (x.who !== "you") theirs.set(x.who, [...(theirs.get(x.who) ?? []), x]);
  const shown = open ?? mine[0]?.key ?? null;
  const row = (x: Asked) => (
    <div key={x.key} {...x.data}>
      <NeedRow
        need={x.need}
        item={x.key}
        open={shown === x.key}
        onToggle={() => setOpen(shown === x.key ? "" : x.key)}
        onAnswer={x.answer}
        extra={
          x.go ? (
            <button type="button" class="k-button is-plain" data-board-item-tab onClick={x.go}>
              Open tab
            </button>
          ) : x.open ? (
            <a class="k-button is-plain" href={x.open} data-board-item-open>
              Open
            </a>
          ) : undefined
        }
      />
    </div>
  );
  const toggle = (who: string) => setUnfolded((s) => (s.has(who) ? new Set([...s].filter((w) => w !== who)) : new Set([...s, who])));
  return (
    <section class="board-needs" aria-label="Needs you" data-board-needs={mine.length}>
      <h2 class="board-needs-head">
        Needs you <span class="m-meta">{mine.length}</span>
      </h2>
      {all.length === 0 ? (
        <p class="m-meta board-needs-none" data-board-needs-none>
          Nothing here needs you.
        </p>
      ) : (
        <NeedList label="Needs you on this board">
          {mine.map(row)}
          {[...theirs].map(([who, list]) => (
            <Row key={who} title={forTeammateFold(list.length, who)} open={unfolded.has(who)} onClick={() => toggle(who)} data-board-needs-others={list.length} data-board-needs-for={who}>
              {list.map(row)}
            </Row>
          ))}
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

