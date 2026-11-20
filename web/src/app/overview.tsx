// The project's Overview (Arugula inversion, L6): Swarm for this project,
// the view the app opens on, the window's width (#1085). The field is the
// project's open boards, one hairline row each: its title, its lane bar,
// its count (`13/18`) and what's going on in words. A board that asks you
// is open; a click opens any other, to its cards in Doing and Review as
// one-line rows, the worst first; a row's preview, on hover or focus, shows
// the sessions on it (./overview-row.tsx). Not on a card is the last
// group. The talk panel on the right (./overview-talk.tsx, #1270; under
// the field on a narrow window) holds Needs you on its top (first on a
// phone; one quiet line when nothing does), Swarm's rail scoped to the
// project, its cards naming their card; and below it the conversations a
// board's row opens, its thread. The old home's lists aren't here (#1088): only where there
// is no Overview (./machines.tsx). A
// card opens the board's own drawer over the page (its address names it,
// so the path and Esc climb back); a session opens on its machine's page,
// on its pane. The grouping is ../shell/swarm/overview.ts; the machines are
// the machine page's (./machine-page), booted once.

import "./overview.css";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { BoardLane, BoardRow, BoardView } from "../shared/proto";
import { boards, BoardError } from "../shared/boards/store";
import { Drawer } from "../shared/boards/card";
import { epicLine, isCard, pointerLabel } from "../shared/boards/text";
import { chat } from "../shared/chat-store";
import { NEEDS_YOU, needsYou, needsYouRank } from "../shared/needs-you";
import { usePlatformApi } from "../shared/platform-api";
import { BY_CARD_EVERY_MS, sessionsByCard, type SessionsByCard } from "../shared/project-sessions";
import { EmptyLine, EmptyState } from "../shared/ui/block-states";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { ChevronIcon, ThreadIcon } from "../shared/ui/icons";
import type { Fleet, FleetPane } from "../shell/fleet";
import { activityOf, holdStill, kindOf, readableCommand, shownNow, whatItDoes } from "../shell/swarm/model";
import { asksYou, boardSays, inFlight, overviewOf, statsLine, type OvBoard, type OvCard, type OvPane, type OvSession, type Standing } from "../shell/swarm/overview";
import { Card as RailCard, bundlesOf, type Bundle } from "../shell/swarm/view";
import { names } from "../platform/links";
import { mayWrite } from "../platform/pages/issues";
import { useBoards } from "../platform/pages/boards";
import { follow, go, pathOf } from "../platform/router";
import { session as platform } from "../platform/session";
import { where } from "../platform/where";
import { boot, type Booted } from "./machine-page";
import { CardRow, SessionRow, Word } from "./overview-row";
import { TalkPanel, useTalk, type TalkTab } from "./overview-talk";
import type { AppSession } from "./session";

export function OverviewPlace({ project, board, card, session }: { project: string; board?: string; card?: string; session: AppSession | null }) {
  const [b, setB] = useState<Booted | null>(null);
  useEffect(() => {
    let live = true;
    void boot(session).then((x) => live && setB(x));
    return () => void (live = false);
  }, []);
  if (!b)
    return (
      <p class="dim" data-overview-loading>
        Reaching your machines…
      </p>
    );
  return <OverviewBody project={project} board={board} card={card} fleet={b.machines.fleet} />;
}

/** A pane as the Overview groups it. */
function ovPane(fleet: Fleet, p: FleetPane): OvPane {
  return {
    key: p.key,
    host: p.host,
    machine: fleet.host(p.host)?.id,
    id: p.id,
    session: p.session,
    project: p.info.session_project?.id ?? null,
    on_issue: p.info.on_issue,
    name: sessionName(p),
    agent: kindOf(p) === "agent",
    standing: standingOf(p),
    last: p.info.activity?.last_ms ?? 0,
  };
}

/** It asks, it failed, it works, or it's quiet: what a tile says of a pane. */
function standingOf(p: FleetPane): Standing {
  if (needsYou(p.info)) return needsYouRank(p.info) <= 1 ? "needs" : "failed";
  if (!p.stale && (activityOf(p) > 0.3 || p.info.attention === "working")) return "working";
  return "quiet";
}

/** A session's name as Arugula names an agent: its agent and title
 * (`Claude Code · #983 cloud half`); anything else, what it runs. */
function sessionName(p: FleetPane): string {
  const what = whatItDoes(p);
  if (kindOf(p) !== "agent") return what;
  const cmd = p.info.command ?? "";
  const agent = cmd && readableCommand(cmd) !== cmd ? readableCommand(cmd) : "";
  return agent && what !== agent ? `${agent} · ${what}` : what;
}

/** A card's words in a list and the path: `#983 · Its title`. */
function cardWords(r: BoardRow): string {
  const label = pointerLabel(r.pointer, r.live);
  const title = isCard(r.live) ? r.live.title : r.note.split("\n").find((l) => l.trim()) ?? "";
  return [label, title].filter(Boolean).join(" · ") || "A card";
}

/** The project's open boards' views, each read again when the platform
 * says it changed, and when the page comes back into view. */
function useBoardViews(project: string): { views: BoardView[] | null; put(v: BoardView): void } {
  const list = useBoards(project);
  const [read, setRead] = useState<Map<string, BoardView>>(new Map());
  const ids = (list ?? []).filter((b) => !b.closed_at).map((b) => b.id);
  const key = ids.join(",");
  useEffect(() => {
    let live = true;
    const again = (id: string) =>
      boards.get(id).then(
        (v) => live && setRead((m) => new Map(m).set(id, v)),
        () => {},
      );
    const offs = ids.map((id) => {
      void again(id);
      // The thread is what the platform pushes a change on: open it so there is one.
      boards.thread(id).catch(() => {});
      return chat.onObject(id, () => void again(id));
    });
    const back = () => document.visibilityState === "visible" && ids.forEach((id) => void again(id));
    document.addEventListener("visibilitychange", back);
    return () => {
      live = false;
      offs.forEach((off) => off());
      document.removeEventListener("visibilitychange", back);
    };
  }, [key]);
  const views = list === null ? null : ids.flatMap((id) => (read.has(id) ? [read.get(id)!] : []));
  return { views, put: (v) => setRead((m) => new Map(m).set(v.board.id, v)) };
}

/** The hub's sessions by card, read again a minute later; null where the
 * hub has no such route yet, or it couldn't answer. */
function useByCard(project: string): SessionsByCard | null {
  const [v, setV] = useState<SessionsByCard | null>(null);
  useEffect(() => {
    let live = true;
    const read = () =>
      sessionsByCard(project).then(
        (x) => live && setV(x),
        () => {},
      );
    void read();
    const t = setInterval(() => document.visibilityState === "visible" && void read(), BY_CARD_EVERY_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [project]);
  return v;
}

/** The boards the reader opened or closed on this project's Overview, kept for the tab. */
function useOpened(project: string): [Map<string, boolean>, (board: string, open: boolean) => void] {
  const key = `arugula.ov-open:${project}`;
  const [m, setM] = useState<Map<string, boolean>>(() => {
    try {
      return new Map(JSON.parse(sessionStorage.getItem(key) ?? "[]") as [string, boolean][]);
    } catch {
      return new Map();
    }
  });
  const set = (board: string, open: boolean) =>
    setM((was) => {
      const next = new Map(was).set(board, open);
      try {
        sessionStorage.setItem(key, JSON.stringify([...next]));
      } catch {
        // Not kept: it holds for this page.
      }
      return next;
    });
  return [m, set];
}

function OverviewBody({ project, board, card, fleet }: { project: string; board?: string; card?: string; fleet: Fleet }) {
  useSubscribe((fn) => fleet.subscribe(fn));
  useSubscribe((fn) => platform.subscribe(fn));
  const api = usePlatformApi();
  const phone = usePhone();
  const { views, put } = useBoardViews(project);
  const byCard = useByCard(project);
  const hostOf = (m: string) => fleet.hostById(m)?.name ?? fleet.host(m)?.name;
  const o = overviewOf(project, views ?? [], fleet.panes.map((p) => ovPane(fleet, p)), byCard, hostOf);
  const mine = fleet.panes.filter((p) => o.cardOf.has(p.key));
  const needs = mine.filter((p) => needsYou(p.info)).length;
  const p = platform.project(project);

  // Needs you: Swarm's rail, for this project's panes, held still under the pointer as Swarm's is.
  const bundles = bundlesOf(mine);
  const [held, setHeld] = useState(false);
  const kept = useRef<Bundle[]>([]);
  const acted = useRef(new Set<string>());
  const railed = holdStill(held, kept.current, bundles, (b) => ({ ...b, gone: true }), acted.current);
  kept.current = railed;
  useEffect(() => shownNow(bundles.flatMap((b) => b.panes)));

  const up = () => go(pathOf({ page: "project", project }, names));
  const openCard = (c: OvCard) => go(pathOf({ page: "project", project, board: c.board, card: c.row.id }, names));
  const paneAt = (pane: { host: string; id: number }, share = false) => {
    const machine = fleet.host(pane.host)?.id;
    go(pathOf({ page: "machines", project, at: machine ? { at: "machine", machine, pane: pane.id, ...(share ? { share: true as const } : {}) } : { at: "machines", name: pane.host, pane: pane.id } }, names));
  };
  const openSession = (s: OvSession) => {
    if (s.lead) paneAt(s.lead);
    else if (s.hosted) go(api.sessionHref?.(project, s.hosted) ?? pathOf({ page: "session", project, session: s.hosted }, names));
  };
  // A board's row is open when the reader opened it, else when it asks
  // them (and stays so once answered, so the row answered doesn't go), or
  // holds the card the address opens.
  const [toggled, setOpen] = useOpened(project);
  const asked = useRef(new Set<string>());
  for (const b of o.boards) if (asksYou(b)) asked.current.add(b.id);
  const isOpen = (b: OvBoard) => toggled.get(b.id) ?? (asked.current.has(b.id) || b.id === board);
  // A rail card's Find: the row its pane is on (its board opened), brought into view.
  const show = (key: string) => {
    const on = o.cardOf.get(key);
    const ob = on && o.boards.find((b) => b.id === on.board);
    if (ob && !isOpen(ob)) setOpen(ob.id, true);
    requestAnimationFrame(() => {
      const at = `[data-ov-panes~="${CSS.escape(key)}"]`;
      const tile = document.querySelector<HTMLElement>(`.ov-row${at}`) ?? document.querySelector<HTMLElement>(at);
      tile?.scrollIntoView({ block: "nearest", behavior: "smooth" });
      tile?.classList.remove("ov-found");
      void tile?.offsetWidth;
      tile?.classList.add("ov-found");
    });
  };

  // The card the address opens: the board's drawer, over the page.
  const shownView = board ? views?.find((v) => v.board.id === board) : undefined;
  const opened = card ? shownView?.zones.flatMap((z) => z.rows).find((r) => r.id === card) : undefined;
  useEffect(() => {
    if (opened) where.cardIs(opened.id, cardWords(opened));
  }, [opened && cardWords(opened), opened?.id]);
  // The drawer's backdrop starts under the bar, as the prototype's does, so
  // the path above it stays a way up while the card is open.
  const [barBottom, setBarBottom] = useState(0);
  useLayoutEffect(() => {
    if (opened) setBarBottom(Math.max(0, Math.round(document.querySelector("[data-platform-bar]")?.getBoundingClientRect().bottom ?? 0)));
  }, [!!opened, phone]);
  const act = async (write: () => Promise<BoardView>): Promise<boolean> => {
    try {
      put(await write());
      return true;
    } catch (e) {
      if (e instanceof BoardError && shownView) void boards.get(shownView.board.id).then(put, () => {});
      api.toast(`Couldn't change it: ${(e as Error).message}`);
      return false;
    }
  };
  // Done closes the card's issue, which the board asks about first: the board, on this card.
  const moveLane = (row: string, lane: BoardLane) => {
    if (!board) return;
    if (lane === "done") return go(`${pathOf({ page: "board", project, board }, names)}#row=${row}`);
    void act(() => boards.setLane(board, row, lane));
  };
  const nameOf = (account: string) => p?.team?.members.find((m) => m.account === account)?.name;
  // An assignee's picture: a native issue names them, a member by that name (or login) has one.
  const faceOf = (name: string) => (chat.on ? p?.team?.members.find((m) => m.name === name || m.account === name)?.account : undefined);

  // The talk panel's tabs; a board's thread opens there, and the panel comes into view where it's under the field.
  const talk = useTalk(project);
  useSubscribe((fn) => chat.subscribe(fn));
  const talkTo = (t: TalkTab) => {
    talk.open(t);
    requestAnimationFrame(() => document.querySelector("[data-ov-talk]")?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
  };

  const any = o.boards.length > 0 || o.loose.length > 0;
  const field = (
    <div class="ov-field" data-ov-field>
      <p class="dim ov-stats" data-ov-stats>
        {statsLine(o, needs)}
      </p>
      {views === null && <p class="dim">Loading…</p>}
      {views !== null && !any && (
        <EmptyState
          where="page"
          data-ov-empty
          title="Nothing in flight."
          why="The project's open boards show here, each a row, with their cards in Doing and Review and the sessions working on them. So do the project's sessions that aren't on a card."
          action={
            <a class="secondary" href={pathOf({ page: "boards", project }, names)} onClick={(e) => follow(e, pathOf({ page: "boards", project }, names))}>
              Boards
            </a>
          }
        />
      )}
      {o.boards.length > 0 && (
        <ul class="ov-boards" data-ov-boards>
          {o.boards.map((b) => (
            <BoardRow key={b.id} b={b} open={isOpen(b)} setOpen={(v) => setOpen(b.id, v)} project={project} fleet={fleet} openCard={openCard} openSession={openSession} opened={card} faceOf={faceOf} talk={chat.on ? () => talkTo({ kind: "thread", id: b.id, title: b.title }) : undefined} />
          ))}
        </ul>
      )}
      {o.loose.length > 0 && (
        <section class="ov-loose" data-ov-loose>
          <header class="ov-loose-head">
            <b>Not on a card</b>
            <span class="dim">Sessions in this project that aren't working on a card</span>
          </header>
          <ul class="ov-rows">
            {o.loose.map((s) => (
              <SessionRow key={s.key} s={s} fleet={fleet} open={() => openSession(s)} />
            ))}
          </ul>
        </section>
      )}
    </div>
  );
  // Nothing needs you: one quiet line, not a column.
  const rail = railed.length === 0 ? (
    <section class="ov-needs ov-needs-none" aria-label={NEEDS_YOU} data-ov-needs>
      <EmptyLine data-ov-needs-none>Nothing needs you.</EmptyLine>
    </section>
  ) : (
    <section class="ov-needs" aria-label={NEEDS_YOU} data-ov-needs>
      <header>
        <h2>{NEEDS_YOU}</h2>
        {needs > 0 && <span data-ov-needs-count>{needs}</span>}
      </header>
      <div class="swarm-cards" onPointerEnter={() => setHeld(true)} onPointerLeave={() => setHeld(false)}>
        {railed.map((b) => {
          const first = b.panes[0];
          const at = o.cardOf.get(first.key);
          return (
            <RailCard
              key={b.key}
              b={b}
              fleet={fleet}
              work={at ? cardWords(at.row) : "Not on a card"}
              show={() => show(first.key)}
              open={() => paneAt(first)}
              share={() => paneAt(first, true)}
              follow={() => paneAt(first)}
              acted={() => acted.current.add(b.key)}
              back={() => {}}
            />
          );
        })}
      </div>
    </section>
  );
  // The chief of staff's tab, pinned first, once it has a room (#1275).
  const chief: TalkTab | null = null;
  return (
    <div class={phone ? "ov phone" : "ov"} data-overview={project} style={{ "--ov-bar-bottom": `${phone ? 0 : barBottom}px` }}>
      {/* On a phone, a list with Needs you on top and the talk under the field. */}
      {phone ? [rail, field, talk.tabs.length > 0 && <TalkPanel talk={talk} pinned={chief} phone />] : [field, <TalkPanel talk={talk} pinned={chief} needs={rail} phone={false} />]}
      {opened && shownView && (
        <Drawer
          r={opened}
          platform={(path) => path}
          close={up}
          project={project}
          others={shownView.zones.flatMap((z) => z.rows)}
          write={!!p && mayWrite(p)}
          act={act}
          id={shownView.board.id}
          moveLane={moveLane}
          nameOf={nameOf}
        />
      )}
    </div>
  );
}

/** A board, one hairline row: the way to open it (when it has cards in
 * flight), its title (the board), its lane bar, its count and what's going
 * on in words; open, its cards in flight, one line each, the worst first,
 * up to eight, and how many more (to the board). */
function BoardRow({ b, open, setOpen, project, fleet, openCard, openSession, opened, faceOf, talk }: { b: OvBoard; open: boolean; setOpen: (open: boolean) => void; project: string; fleet: Fleet; openCard: (c: OvCard) => void; openSession: (s: OvSession) => void; opened?: string; faceOf: (name: string) => string | undefined; talk?: () => void }) {
  const to = pathOf({ page: "board", project, board: b.id }, names);
  const e = epicLine({ done: b.done, total: b.total, proven: b.proven, review: b.review.length, doing: b.doing.length });
  const flight = b.doing.length + b.review.length > 0;
  const shown = flight && open;
  const { shown: rows, more } = inFlight(b, 8);
  const id = `ov-board-${b.id}`;
  return (
    <li class={`ov-board${shown ? " ov-board-open" : ""}${flight ? "" : " ov-board-still"}`} data-ov-board={b.id} data-ov-open={shown ? "" : undefined} data-ov-standing={b.standing} data-ov-panes={[...b.doing, ...b.review].flatMap((c) => c.sessions.flatMap((s) => s.panes.map((p) => p.key))).join(" ")}>
      {/* The row opens with a click anywhere on it but its title, which is the board. */}
      <div class="ov-board-line" onClick={(ev) => flight && !(ev.target as Element).closest("a, button") && setOpen(!open)}>
        <button type="button" class="ov-board-toggle" disabled={!flight} aria-expanded={flight ? shown : undefined} aria-controls={shown ? id : undefined} aria-label={shown ? `Close ${b.title}` : `Open ${b.title}`} data-ov-board-toggle onClick={() => setOpen(!open)}>
          {flight && <ChevronIcon dir={shown ? "down" : "right"} size="sm" />}
        </button>
        <a class="ov-board-title" href={to} onClick={(ev) => follow(ev, to)} data-ov-board-title>
          {b.title}
        </a>
        <div class="board-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.total} aria-valuenow={b.done} aria-label={e.words}>
          {e.segments.map((s) => (
            <i key={s.lane} data-lane={s.lane} style={{ width: `${s.pct}%` }} />
          ))}
        </div>
        <span class="ov-board-count" title={e.words} data-ov-board-count>
          {b.total ? `${b.done}/${b.total}` : "–"}
        </span>
        <span class="ov-board-says" data-ov-board-says>
          {boardSays(b).map((x) => (
            <Word key={x.standing} standing={x.standing} words={x.words} />
          ))}
        </span>
        {/* Its thread, in the talk panel (while chat is on). */}
        {talk && (
          <button type="button" class="ov-board-talk" aria-label={`Talk in ${b.title}'s thread`} title="Talk in its thread" data-ov-board-talk onClick={talk}>
            <ThreadIcon size="sm" />
          </button>
        )}
      </div>
      {shown && (
        <div class="ov-board-rows" id={id}>
          <ul class="ov-rows">
            {rows.map((c) => (
              <CardRow key={c.row.id} c={c} fleet={fleet} open={() => openCard(c)} openSession={openSession} current={opened === c.row.id} faceOf={faceOf} />
            ))}
          </ul>
          {more > 0 && (
            <a class="ov-more-rows" href={to} onClick={(ev) => follow(ev, to)} data-ov-more-rows>
              {more} more in flight
            </a>
          )}
        </div>
      )}
    </li>
  );
}
