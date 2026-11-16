// The project's Overview (Arugula inversion, L6): Swarm for this project,
// the view the app opens on. The field on the left is the project's boards,
// each with its cards in Doing and Review as one-line rows, the worst first
// (Next and Done are counts in its header, with its progress); a row's
// preview, on hover or focus, shows the sessions on it (./overview-row.tsx).
// Not on a card is the last group. Needs you on the right
// is Swarm's rail, scoped to the project, its cards naming their card. A
// card opens the board's own drawer over the page (its address names it,
// so the path and Esc climb back); a session opens on its machine's page,
// on its pane. The grouping is ../shell/swarm/overview.ts; the machines are
// the machine page's (./machine-page), booted once.

import "./overview.css";
import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { BoardLane, BoardRow, BoardView } from "../shared/proto";
import { boards, BoardError } from "../shared/boards/store";
import { Drawer } from "../shared/boards/card";
import { isCard, pointerLabel } from "../shared/boards/text";
import { chat } from "../shared/chat-store";
import { NEEDS_YOU, needsYou, needsYouRank } from "../shared/needs-you";
import { usePlatformApi } from "../shared/platform-api";
import { BY_CARD_EVERY_MS, sessionsByCard, type SessionsByCard } from "../shared/project-sessions";
import { EmptyLine, EmptyState } from "../shared/ui/block-states";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import type { Fleet, FleetPane } from "../shell/fleet";
import { activityOf, holdStill, kindOf, readableCommand, shownNow, whatItDoes } from "../shell/swarm/model";
import { boardLine, inFlight, overviewOf, statsLine, type OvBoard, type OvCard, type OvPane, type OvSession, type Standing } from "../shell/swarm/overview";
import { Card as RailCard, bundlesOf, type Bundle } from "../shell/swarm/view";
import { names } from "../platform/links";
import { mayWrite } from "../platform/pages/issues";
import { useBoards } from "../platform/pages/boards";
import { follow, go, pathOf } from "../platform/router";
import { session as platform } from "../platform/session";
import { where } from "../platform/where";
import { boot, type Booted } from "./machine-page";
import { CardRow, SessionRow } from "./overview-row";
import type { AppSession } from "./session";

export function OverviewPlace({ project, board, card, session, children }: { project: string; board?: string; card?: string; session: AppSession | null; children?: ComponentChildren }) {
  const [b, setB] = useState<Booted | null>(null);
  useEffect(() => {
    let live = true;
    void boot(session).then((x) => live && setB(x));
    return () => void (live = false);
  }, []);
  if (!b)
    return (
      <>
        <p class="dim" data-overview-loading>
          Reaching your machines…
        </p>
        {children}
      </>
    );
  return (
    <OverviewBody project={project} board={board} card={card} fleet={b.machines.fleet}>
      {children}
    </OverviewBody>
  );
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

function OverviewBody({ project, board, card, fleet, children }: { project: string; board?: string; card?: string; fleet: Fleet; children?: ComponentChildren }) {
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
  // A rail card's Find: the tile its pane is in, brought into view.
  const show = (key: string) => {
    const tile = document.querySelector<HTMLElement>(`[data-ov-panes~="${CSS.escape(key)}"]`);
    tile?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    tile?.classList.remove("ov-found");
    void tile?.offsetWidth;
    tile?.classList.add("ov-found");
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

  const flight = o.boards.length > 0 || o.loose.length > 0;
  const field = (
    <div class="ov-field" data-ov-field>
      <p class="dim ov-stats" data-ov-stats>
        {statsLine(o, needs)}
      </p>
      {views === null && <p class="dim">Loading…</p>}
      {views !== null && !flight && (
        <EmptyState
          where="page"
          data-ov-empty
          title="Nothing in flight."
          why="A card in Doing or Review shows here, with the sessions working on it. So do the project's sessions that aren't on a card."
          action={
            <a class="secondary" href={pathOf({ page: "boards", project }, names)} onClick={(e) => follow(e, pathOf({ page: "boards", project }, names))}>
              Boards
            </a>
          }
        />
      )}
      {o.boards.map((b) => (
        <BoardGroup key={b.id} b={b} project={project} fleet={fleet} openCard={openCard} openSession={openSession} opened={card} faceOf={faceOf} />
      ))}
      {o.loose.length > 0 && (
        <section class="ov-board ov-loose" data-ov-loose>
          <header class="ov-board-head">
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
      {children && (
        <div class="ov-more" data-ov-more>
          {children}
        </div>
      )}
    </div>
  );
  const rail = (
    <aside class="ov-needs" aria-label={NEEDS_YOU} data-ov-needs>
      <header>
        <h2>{NEEDS_YOU}</h2>
        {needs > 0 && <span data-ov-needs-count>{needs}</span>}
      </header>
      <div class="swarm-cards" onPointerEnter={() => setHeld(true)} onPointerLeave={() => setHeld(false)}>
        {railed.length === 0 && <EmptyLine>Nothing needs you. Answered cards fade, then clear.</EmptyLine>}
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
    </aside>
  );
  return (
    <div class={phone ? "ov phone" : "ov"} data-overview={project} style={{ "--ov-bar-bottom": `${phone ? 0 : barBottom}px` }}>
      {/* On a phone, a list with Needs you on top. */}
      {phone ? [rail, field] : [field, rail]}
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

/** A board: its title, what's in flight, next and done, and its progress;
 * then its cards in flight, one line each, the worst first, up to four,
 * and how many more (to the board). */
function BoardGroup({ b, project, fleet, openCard, openSession, opened, faceOf }: { b: OvBoard; project: string; fleet: Fleet; openCard: (c: OvCard) => void; openSession: (s: OvSession) => void; opened?: string; faceOf: (name: string) => string | undefined }) {
  const to = pathOf({ page: "board", project, board: b.id }, names);
  const pct = b.total ? Math.round((b.done / b.total) * 100) : 0;
  const { shown, more } = inFlight(b);
  return (
    <section class="ov-board" data-ov-board={b.id}>
      <header class="ov-board-head">
        <b data-ov-board-title>{b.title}</b>
        <span class="dim" data-ov-board-line>
          {boardLine(b)}
        </span>
      </header>
      <div class="ov-progress" role="progressbar" aria-label={`${b.done} of ${b.total} done`} aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
        <i style={{ width: `${pct}%` }} />
      </div>
      <ul class="ov-rows">
        {shown.map((c) => (
          <CardRow key={c.row.id} c={c} fleet={fleet} open={() => openCard(c)} openSession={openSession} current={opened === c.row.id} faceOf={faceOf} />
        ))}
      </ul>
      {more > 0 && (
        <a class="ov-more-rows" href={to} onClick={(e) => follow(e, to)} data-ov-more-rows>
          {more} more in flight
        </a>
      )}
    </section>
  );
}
