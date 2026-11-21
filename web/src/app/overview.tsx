// The project's Overview (Arugula inversion, L6): Swarm for this project,
// the view the app opens on, the window's width (#1085). Under its stats,
// the briefing line: what happened since the reader last looked, from the
// event log (./overview-briefing.tsx, #1277). The field is the
// project's open boards, one hairline row each: its title, its lane bar,
// its count (`13/18`) and what's going on in words. A board that asks you
// is open; a click opens any other, to its cards in Doing and Review as
// one-line rows, the worst first; a row's preview, on hover or focus, shows
// the sessions on it (./overview-row.tsx). Not on a card is the last
// group. The talk panel on the right (./overview-talk.tsx, #1270; under
// the field on a narrow window) holds Needs you on its top (first on a
// phone; one quiet line when nothing does), Swarm's rail scoped to the
// project, its cards naming their card; and below it the conversations
// the field opens: a board's thread, from its row, and a session's peek
// (./overview-peek.tsx, #1271), from a card's row or a session's on no
// card. The old home's lists aren't here (#1088): only where there is no
// Overview (./machines.tsx). A card with no session opens the board's own
// drawer over the page (its address names it, so the path and Esc climb
// back), as its peek's card link does; Open in Panes opens a session on
// its machine's page, on its pane. The grouping is ../shell/swarm/overview.ts; the machines are
// the machine page's (./machine-page), booted once.
//
// It zooms in place (Arugula #1287): from the Project level (the field
// above) to a board (./overview-board.tsx), and on to the work on a card
// or a session on no card (./overview-work.tsx), each at its own address
// (../platform/paths.ts), so a link, a reload and Back land on it. The
// bar's path names each level and climbs back; so do the level pill and
// Esc, a level at a time (a session on no card is a step from Project).
// The talk panel's tabs stay as you zoom: each level brings its own to
// the front, the chief of staff's at Project, the lead's at Board, the
// card's agent's at Work. A Needs you card zooms to the work that asks.

import "./overview.css";
import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { BoardLane, BoardRow, BoardView } from "../shared/proto";
import type { OverviewZoom } from "../shared/platform-api";
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
import { cleanTitle } from "../shared/titles";
import { asksYou, boardSays, inFlight, overviewOf, statsLine, type OvBoard, type OvCard, type OvLead, type OvPane, type OvSession, type Standing, STATE_WORD } from "../shell/swarm/overview";
import { bundlesOf, type Bundle } from "../shell/swarm/view";
import { Asks } from "./overview-asks";
import { names } from "../platform/links";
import { mayWrite } from "../platform/pages/issues";
import { useBoards } from "../platform/pages/boards";
import { follow, go, pathOf } from "../platform/router";
import { session as platform } from "../platform/session";
import { where } from "../platform/where";
import { boot, type Booted } from "./machine-page";
import { BriefingLine, useBriefing } from "./overview-briefing";
import { OverviewBoard } from "./overview-board";
import { CardRow, SessionRow, Word } from "./overview-row";
import { OverviewWork } from "./overview-work";
import { PeekContext, peekKey, type PeekPlace } from "./overview-peek";
import { staffTab, TalkPanel, useTalk, type TalkTab } from "./overview-talk";
import type { AppSession } from "./session";

export function OverviewPlace({ project, board, card, zoom, session }: { project: string; board?: string; card?: string; zoom?: OverviewZoom; session: AppSession | null }) {
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
  return <OverviewBody project={project} board={board} card={card} zoom={zoom} fleet={b.machines.fleet} />;
}

/** The Overview's levels, outermost first (Arugula #1287). */
const LEVELS = ["Project", "Board", "Work"] as const;
type Level = 0 | 1 | 2;
/** How far in `zoom` is: 0 Project, 1 a board, 2 a card's or a session's work. */
const levelOf = (zoom?: OverviewZoom): Level => (!zoom ? 0 : "session" in zoom || zoom.card ? 2 : 1);
/** A level out from `zoom`: a card's board, else Project. */
const outOf = (zoom: OverviewZoom, to: Level = (levelOf(zoom) - 1) as Level): OverviewZoom | undefined => (to === 1 && "board" in zoom ? { board: zoom.board } : undefined);
const zoomKey = (zoom?: OverviewZoom) => (!zoom ? "" : "session" in zoom ? `s:${zoom.session}` : `b:${zoom.board}/${zoom.card ?? ""}`);
/** A session's peek, as a tab. */
const peekTab = (s: OvSession): TalkTab => ({ kind: "peek", id: peekKey(s), title: s.name });

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
    title: cleanTitle(p.info.title ?? ""),
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

/** How long the page is hidden before coming back to it reads again. A shorter
 * absence misses nothing: the live pushes ran while it was hidden. */
const REFOCUS_AFTER_MS = 30_000;

/** A count that goes up when the page comes back into view after being hidden
 * for `REFOCUS_AFTER_MS`: the one refocus the boards and the briefing share (#1297). */
function useRefocus(): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    let hidden: number | null = document.visibilityState === "hidden" ? Date.now() : null;
    const vis = () => {
      if (document.visibilityState === "hidden") hidden ??= Date.now();
      else {
        const long = hidden !== null && Date.now() - hidden >= REFOCUS_AFTER_MS;
        hidden = null;
        if (long) setN((x) => x + 1);
      }
    };
    document.addEventListener("visibilitychange", vis);
    return () => document.removeEventListener("visibilitychange", vis);
  }, []);
  return n;
}

/** The project's open boards' views from one read of the hub, so they paint
 * together (#1296), each read again when the platform says it changed, and
 * all of them when the page comes back into view after a while (`refocus`).
 * `views` is null until they land. */
function useBoardViews(project: string, refocus: number): { views: BoardView[] | null; put(v: BoardView): void } {
  const list = useBoards(project);
  const [read, setRead] = useState<Map<string, BoardView> | null>(null);
  // The hub makes each open board's thread as it answers the one read: that is what pushes a board's change.
  // A hub without the route (404) doesn't, so then the page opens the threads and reads each board, as it did
  // before the route. That fallback goes once every hub answers the route.
  const [each, setEach] = useState(false);
  const ids = (list ?? []).filter((b) => !b.closed_at).map((b) => b.id);
  const key = ids.join(",");
  // Boards read alone since the last whole read began: it doesn't write over them with what it saw before.
  const newer = useRef(new Set<string>());
  const whole = useRef<() => void>(() => {});
  const asked = useRef("");
  const eachNow = useRef(false);
  const again = useRef<() => void>(() => {});
  eachNow.current = each;
  useEffect(() => {
    let live = true;
    setRead(null);
    setEach(false);
    asked.current = "";
    whole.current = () => {
      newer.current = new Set();
      boards.views(project).then(
        (v) => {
          if (!live) return;
          if (!v) return setEach(true);
          setRead((was) => {
            const m = new Map(v.map((x) => [x.board.id, x] as const));
            for (const id of newer.current) if (was?.has(id)) m.set(id, was.get(id)!);
            return m;
          });
        },
        () => live && setEach(true),
      );
    };
    whole.current();
    return () => {
      live = false;
    };
  }, [project]);
  // Back after a while: the one read of every board, or, from a hub without the route, each board alone.
  const seen = useRef(refocus);
  useEffect(() => {
    if (seen.current === refocus) return;
    seen.current = refocus;
    if (!eachNow.current) whole.current();
    else again.current();
  }, [refocus]);
  // A board opened since that read (the list says so) is read for.
  useEffect(() => {
    if (read && !each && asked.current !== key && ids.some((id) => !read.has(id))) {
      asked.current = key;
      whole.current();
    }
  }, [key, read]);
  useEffect(() => {
    let live = true;
    const again = (id: string) =>
      boards.get(id).then(
        (v) => {
          if (!live) return;
          newer.current.add(id);
          setRead((m) => new Map(m ?? []).set(id, v));
        },
        () => {},
      );
    const offs = ids.map((id) => chat.onObject(id, () => void again(id)));
    if (each && list !== null) {
      // Together, as the one read paints them.
      void Promise.all(
        ids.map((id) => {
          boards.thread(id).catch(() => {});
          return boards.get(id).then((v) => [id, v] as const, () => null);
        }),
      ).then((got) => live && setRead((m) => new Map([...(m ?? []), ...got.flatMap((g) => (g ? [g] : []))])));
    }
    again.current = () => ids.forEach((id) => void again(id));
    return () => {
      live = false;
      offs.forEach((off) => off());
    };
  }, [key, each, list !== null]);
  const views = list === null || read === null ? null : ids.flatMap((id) => (read.has(id) ? [read.get(id)!] : []));
  return { views, put: (v) => setRead((m) => new Map(m ?? []).set(v.board.id, v)) };
}

/** Rows of the board list's own shape, still, until the boards' views land: what comes takes their place without the page moving. */
function BoardsSkeleton() {
  return (
    <ul class="ov-boards ov-skeleton" data-ov-skeleton aria-busy="true" aria-label="Loading the boards">
      {[0, 1, 2].map((i) => (
        <li key={i} class="ov-board ov-board-still">
          <div class="ov-board-line">
            <span />
            <i class="ov-skel-bar" />
            <i class="ov-skel-bar" />
            <span />
            <i class="ov-skel-bar" />
            <i class="ov-skel-bar" />
            <span />
          </div>
        </li>
      ))}
    </ul>
  );
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

function OverviewBody({ project, board, card, zoom, fleet }: { project: string; board?: string; card?: string; zoom?: OverviewZoom; fleet: Fleet }) {
  useSubscribe((fn) => fleet.subscribe(fn));
  useSubscribe((fn) => platform.subscribe(fn));
  // The chief of staff's room shows as the talk panel's first tab once chat lists it.
  useSubscribe((fn) => chat.subscribe(fn));
  const api = usePlatformApi();
  const phone = usePhone();
  const refocus = useRefocus();
  const { views, put } = useBoardViews(project, refocus);
  const byCard = useByCard(project);
  const briefing = useBriefing(project, byCard, refocus);
  const hostOf = (m: string) => fleet.hostById(m)?.name ?? fleet.host(m)?.name;
  const o = overviewOf(project, views ?? [], fleet.panes.map((p) => ovPane(fleet, p)), byCard, hostOf);
  const mine = fleet.panes.filter((p) => o.cardOf.has(p.key));
  const needs = mine.filter((p) => needsYou(p.info)).length;
  const p = platform.project(project);

  // Needs you: Swarm's rail, for this project's panes, held still under the pointer as Swarm's is.
  const bundles = bundlesOf(mine);
  const [over, setHeld] = useState(false);
  // An open ask holds the list still too, whether or not the pointer is on it.
  const [pinned, setPinned] = useState<ReadonlySet<string>>(new Set());
  const pin = (key: string, on: boolean) => setPinned((s) => (s.has(key) === on ? s : new Set(on ? [...s, key] : [...s].filter((k) => k !== key))));
  const held = over || pinned.size > 0;
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
  const openPanes = (s: OvSession) => {
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
  // A card's row, or a session's, opens its peek there (#1271); a card with no session, its drawer.
  const openSession = (s: OvSession) => talkTo({ kind: "peek", id: peekKey(s), title: s.name });
  const ovCards = o.boards.flatMap((b) => [...b.doing, ...b.review]);
  const peeks: PeekPlace = {
    fleet,
    find: (key) => {
      for (const c of ovCards) for (const s of c.sessions) if (peekKey(s) === key) return { s, card: c };
      const s = o.loose.find((x) => peekKey(x) === key) ?? o.boards.find((b) => b.lead && peekKey(b.lead.peek) === key)?.lead?.peek;
      return s && { s, card: null };
    },
    openPanes,
    openCard,
    askLead: chat.on ? (id) => talkTo({ kind: "thread", id, title: o.boards.find((b) => b.id === id)?.title ?? "Board" }) : undefined,
  };

  // Zooming (Arugula #1287): where the address is, and the way in and out.
  const level = levelOf(zoom);
  const zkey = zoomKey(zoom);
  const zoomTo = (z?: OverviewZoom) => go(pathOf({ page: "project", project, ...(z ? { zoom: z } : {}) }, names));
  const zoomBoard = zoom && "board" in zoom ? zoom.board : undefined;
  const zoomCard = zoom && "board" in zoom ? zoom.card : undefined;
  const zoomedBoard = zoomBoard ? o.boards.find((b) => b.id === zoomBoard) : undefined;
  const zoomedView = zoomBoard ? views?.find((v) => v.board.id === zoomBoard) : undefined;
  const zoomedRow = zoomCard ? zoomedView?.zones.flatMap((z) => z.rows).find((r) => r.id === zoomCard) : undefined;
  const zoomedCard = zoomCard ? ovCards.find((c) => c.board === zoomBoard && c.row.id === zoomCard) : undefined;
  const zoomedSession = zoom && "session" in zoom ? peeks.find(zoom.session)?.s : undefined;
  const workSessions = zoomedCard ? zoomedCard.sessions : zoomedSession ? [zoomedSession] : [];
  // A Needs you card's pane: the work it's on, its card's or its own on no card.
  const zoomToWork = (key: string) => {
    const at = o.cardOf.get(key);
    if (at) return zoomTo({ board: at.board, card: at.row.id });
    const s = o.loose.find((x) => x.panes.some((p) => p.key === key));
    if (s) zoomTo({ session: peekKey(s) });
    else show(key);
  };
  // The bar's path names the card, or the session, zoomed to.
  useEffect(() => {
    if (zoomedRow) where.cardIs(zoomedRow.id, cardWords(zoomedRow));
    else if (zoomedSession && zoom && "session" in zoom) where.cardIs(zoom.session, zoomedSession.name);
  }, [zoomedRow && cardWords(zoomedRow), zoomedRow?.id, zoomedSession?.name]);
  // Esc zooms out a level, unless it's someone else's: a box being typed in, a dialog.
  useEffect(() => {
    if (!zoom) return;
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
      if ((e.target as Element | null)?.closest?.("input, textarea, select, [contenteditable], dialog, [role=dialog]")) return;
      if (document.querySelector("dialog[open], [aria-modal=true]")) return;
      e.preventDefault();
      zoomTo(outOf(zoom));
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [zkey]);

  // A lead on a teammate's machine is named with them (a team's own box has no one to name).
  const teammateOf = (machine: string) => {
    const h = fleet.hostById(machine);
    return h && !h.team ? h.owner : undefined;
  };

  // The chief of staff's tab, pinned first, once it has a room (#1275).
  const chief: TalkTab | null = staffTab(project);

  // Its pinned tab is picked, not opened, so it never takes one of the tabs' places.
  const pickChief = (t: TalkTab) => {
    talk.pick(`${t.kind}:${t.id}`);
    requestAnimationFrame(() => document.querySelector("[data-ov-talk]")?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
  };

  // Each level brings its own tab to the front, once, as it's zoomed to
  // (the tab the reader had stays at Project on a reload): the chief of
  // staff's at Project, the lead's at Board, the agent's at Work. The
  // others stay open.
  const front: TalkTab | null = level === 0 ? chief : level === 1 ? (zoomedBoard ? leadTab(zoomedBoard) : null) : workSessions.length ? peekTab(workSessions.find((s) => s.panes.some((p) => p.agent)) ?? workSessions[0]) : null;
  const fronted = useRef<string | null>(zoom ? null : zkey);
  useEffect(() => {
    if (fronted.current === zkey || !front) return;
    fronted.current = zkey;
    if (front === chief) talk.pick(`${front.kind}:${front.id}`);
    else talk.open(front);
  }, [zkey, front && `${front.kind}:${front.id}`]);
  // A board no one leads: ask the chief of staff for one, in the staff room, and bring its tab up to read the answer.
  async function askChief(t: TalkTab, b: OvBoard) {
    try {
      await chat.post(t.id, `Please start a session to lead the board "${b.title}" (${b.id}): read its goal and open tickets, then lead it.`);
      pickChief(t);
    } catch (e) {
      api.toast(`Couldn't ask the chief of staff: ${(e as Error).message}`);
    }
  }
  // The lead's tab: its session's peek, as its chip opens it (#1290); a board no one leads, its thread.
  function leadTab(b: OvBoard): TalkTab | null {
    if (b.lead) return peekTab(b.lead.peek);
    return chat.on ? { kind: "thread", id: b.id, title: b.title } : null;
  }

  // A card no agent works on offers its board's lead (#1289): its tab, as the Board level opens it.
  const askLeadOf = (b: OvBoard) => {
    const t = leadTab(b);
    return t ? () => talkTo(t) : undefined;
  };

  const any = o.boards.length > 0 || o.loose.length > 0;
  const projectLevel = (
    <>
      <p class="dim ov-stats" data-ov-stats>
        {statsLine(o, needs)}
      </p>
      {/* Since the reader last looked (#1277), counted from the event log; it opens the chief of staff's tab. */}
      <BriefingLine b={briefing} staffRoom={chief?.id} open={chief ? () => pickChief(chief) : undefined} />
      {views === null && <BoardsSkeleton />}
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
            <BoardRow key={b.id} b={b} zoom={() => zoomTo({ board: b.id })} leadWho={b.lead && teammateOf(b.lead.machine)} talkLead={b.lead && (() => openSession(b.lead!.peek))} open={isOpen(b)} setOpen={(v) => setOpen(b.id, v)} project={project} fleet={fleet} openCard={openCard} openSession={openSession} opened={card} faceOf={faceOf} talk={chat.on ? () => talkTo({ kind: "thread", id: b.id, title: b.title }) : undefined} />
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
    </>
  );
  // Zoomed in, the field is that level's; at Project, the rows above.
  const talkingPeek = talk.active?.startsWith("peek:") ? talk.active.slice("peek:".length) : null;
  const levelView =
    level === 0 ? null : views === null || (level === 2 && zoom && "session" in zoom && !zoomedSession && fleet.panes.length === 0) ? (
      <p class="dim">Loading…</p>
    ) : level === 1 || zoomCard ? (
      !zoomedBoard || !zoomedView ? (
        <EmptyLine data-ov-level-gone>That board isn't open in this project now.</EmptyLine>
      ) : level === 1 ? (
        <OverviewBoard
          project={project}
          board={zoomedBoard}
          view={zoomedView}
          fleet={fleet}
          zoomCard={(row) => zoomTo({ board: zoomedBoard.id, card: row })}
          talkTo={openSession}
          leadWho={zoomedBoard.lead && teammateOf(zoomedBoard.lead.machine)}
          leads={zoomedBoard.lead ? o.boards.filter((x) => x.lead?.machine === zoomedBoard.lead!.machine && x.lead.session === zoomedBoard.lead!.session).length : 0}
          thread={chat.on ? () => talkTo({ kind: "thread", id: zoomedBoard.id, title: zoomedBoard.title }) : undefined}
          askChief={chief ? () => void askChief(chief, zoomedBoard) : undefined}
        />
      ) : !zoomedRow ? (
        <EmptyLine data-ov-level-gone>That card isn't on this board now.</EmptyLine>
      ) : (
        <OverviewWork project={project} fleet={fleet} card={{ board: zoomedBoard.id, boardTitle: zoomedBoard.title, row: zoomedRow, standing: zoomedCard?.standing ?? "idle" }} sessions={workSessions} talking={talkingPeek} talkTo={(s) => talkTo(peekTab(s))} openPanes={openPanes} askLead={askLeadOf(zoomedBoard)} />
      )
    ) : (
      <OverviewWork project={project} fleet={fleet} card={null} sessions={workSessions} talking={talkingPeek} talkTo={(s) => talkTo(peekTab(s))} openPanes={openPanes} />
    );
  const field = (
    <div class="ov-field" data-ov-field data-ov-level={LEVELS[level].toLowerCase()}>
      <ZoomLevels level={level} board={!!zoomBoard} phone={phone} out={(to) => zoom && zoomTo(outOf(zoom, to))} />
      <Stage zkey={zkey} level={level}>
        {levelView ?? projectLevel}
      </Stage>
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
        <Asks
          asks={railed}
          fleet={fleet}
          pin={pin}
          card={(b) => {
            const first = b.panes[0];
            const at = o.cardOf.get(first.key);
            return {
              work: at ? cardWords(at.row) : "Not on a card",
              show: () => zoomToWork(first.key),
              open: () => paneAt(first),
              share: () => paneAt(first, true),
              follow: () => paneAt(first),
              acted: () => acted.current.add(b.key),
              back: () => {},
            };
          }}
        />
      </div>
    </section>
  );
  return (
    <div class={phone ? "ov phone" : "ov"} data-overview={project} style={{ "--ov-bar-bottom": `${phone ? 0 : barBottom}px` }}>
      {/* On a phone, a list with Needs you on top and the talk under the field. */}
      <PeekContext.Provider value={peeks}>{phone ? [rail, field, (talk.tabs.length > 0 || chief) && <TalkPanel talk={talk} pinned={chief} phone />] : [field, <TalkPanel talk={talk} pinned={chief} needs={rail} phone={false} />]}</PeekContext.Provider>
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

/** A board's lead (#414, #1272): a dot for how its session stands (none
 * where no machine or the hub tells), its name, a teammate's with theirs,
 * and its machine. It opens that agent's peek (#1290), never the board's
 * thread, which is the row's own button. */
function LeadChip({ lead, who, talk }: { lead: OvLead; who?: string; talk: () => void }) {
  const name = who ? `${who}'s ${lead.session}` : lead.session;
  const says = lead.standing ? STATE_WORD[lead.standing] : "";
  const title = `Led by ${name}${lead.machineName ? ` on ${lead.machineName}` : ""}${says ? `, ${says}` : ""}`;
  const inner = (
    <>
      <i class="ov-lead-dot" data-ov-lead-standing={lead.standing ?? undefined} aria-hidden="true" />
      <span class="ov-lead-name">{name}</span>
      {lead.machineName && <span class="ov-lead-machine">{lead.machineName}</span>}
    </>
  );
  return (
    <button type="button" class="ov-lead" title={`${title}: talk to it`} aria-label={`${title}: talk to it`} data-ov-board-lead={lead.session} onClick={talk}>
      {inner}
    </button>
  );
}

/** A board, one hairline row: the way to open it (when it has cards in
 * flight), its title, its lane bar, its count and what's going on in
 * words; open, its cards in flight, one line each, the worst first, up to
 * eight, and how many more. A click on the row (its title, or anywhere
 * but a button) zooms to the board (#1287). */
function BoardRow({ b, zoom, leadWho, talkLead, open, setOpen, project, fleet, openCard, openSession, opened, faceOf, talk }: { b: OvBoard; zoom: () => void; leadWho?: string; talkLead?: () => void; open: boolean; setOpen: (open: boolean) => void; project: string; fleet: Fleet; openCard: (c: OvCard) => void; openSession: (s: OvSession) => void; opened?: string; faceOf: (name: string) => string | undefined; talk?: () => void }) {
  const to = pathOf({ page: "project", project, zoom: { board: b.id } }, names);
  const e = epicLine({ done: b.done, total: b.total, proven: b.proven, review: b.review.length, doing: b.doing.length });
  const flight = b.doing.length + b.review.length > 0;
  const shown = flight && open;
  const { shown: rows, more } = inFlight(b, 8);
  const id = `ov-board-${b.id}`;
  return (
    <li class={`ov-board${shown ? " ov-board-open" : ""}${flight ? "" : " ov-board-still"}`} data-ov-board={b.id} data-ov-open={shown ? "" : undefined} data-ov-standing={b.standing} data-ov-panes={[...b.doing, ...b.review].flatMap((c) => c.sessions.flatMap((s) => s.panes.map((p) => p.key))).join(" ")}>
      {/* A click anywhere on the row zooms to the board; its chevron opens it in place. */}
      <div class="ov-board-line" onClick={(ev) => !(ev.target as Element).closest("a, button, [data-ov-board-lead]") && !getSelection()?.toString() && zoom()}>
        <button type="button" class="ov-board-toggle" disabled={!flight} aria-expanded={flight ? shown : undefined} aria-controls={shown ? id : undefined} aria-label={shown ? `Close ${b.title}` : `Open ${b.title}`} data-ov-board-toggle onClick={() => setOpen(!open)}>
          {flight && <ChevronIcon dir={shown ? "down" : "right"} size="sm" />}
        </button>
        <a class="ov-board-title" href={to} onClick={(ev) => follow(ev, to)} title={`Zoom in to ${b.title}`} data-ov-board-title>
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
        <span class="ov-board-lead-cell">{b.lead && talkLead && <LeadChip lead={b.lead} who={leadWho} talk={talkLead} />}</span>
        {/* Its thread, in the talk panel (while chat is on): the lead and anyone mentioned read it. */}
        {talk && (
          <button type="button" class="ov-board-talk" aria-label={`Board thread: talk in ${b.title}'s thread`} title="Board thread: its lead and anyone mentioned read it" data-ov-board-talk onClick={talk}>
            <ThreadIcon size="sm" />
          </button>
        )}
      </div>
      {shown && (
        <div class="ov-board-rows" id={id}>
          <ul class="ov-rows">
            {rows.map((c) => (
              <CardRow key={c.row.id} c={c} fleet={fleet} open={() => (c.sessions.length ? openSession(c.sessions[0]) : openCard(c))} openSession={openSession} current={opened === c.row.id} faceOf={faceOf} />
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

/** The level pill (#1287): Project, Board and Work, the one you're at
 * lit; a level out is a click, a level in isn't (you zoom in by clicking
 * what's there), and Board isn't on the way out of a session on no card.
 * Esc says so beside it. */
function ZoomLevels({ level, board, phone, out }: { level: Level; board: boolean; phone: boolean; out: (to: Level) => void }) {
  return (
    <div class="ov-zoom-bar" data-ov-zoom-bar>
      <div class="ov-levels" role="group" aria-label="Zoom level" data-ov-levels>
        {LEVELS.map((name, i) => (
          <button key={name} type="button" class={i === level ? "ov-level-on" : undefined} aria-current={i === level ? "true" : undefined} disabled={i > level || (i === 1 && !board)} data-ov-level-pill={name.toLowerCase()} onClick={() => i < level && out(i as Level)}>
            {name}
          </button>
        ))}
      </div>
      {!phone && level > 0 && <span class="dim ov-zoom-hint">Esc to zoom out</span>}
    </div>
  );
}

/** Where the pointer last went down, for the zoom to grow from (#1287). */
let pressed: { x: number; y: number; at: number } | null = null;
if (typeof document !== "undefined") document.addEventListener("pointerdown", (e) => void (pressed = { x: e.clientX, y: e.clientY, at: Date.now() }), { capture: true, passive: true });

/** A level's view, drawn anew as the zoom changes: a short scale and fade,
 * in from where you clicked going in, settling back going out. None on
 * the first draw (a link, a reload). */
function Stage({ zkey, level, children }: { zkey: string; level: Level; children: ComponentChildren }) {
  const was = useRef<{ key: string; level: Level; dir: "in" | "out" | null }>({ key: zkey, level, dir: null });
  if (was.current.key !== zkey) was.current = { key: zkey, level, dir: level >= was.current.level ? "in" : "out" };
  const dir = was.current.dir;
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !dir) return;
    const r = el.getBoundingClientRect();
    const p = pressed && Date.now() - pressed.at < 1000 ? pressed : null;
    el.style.transformOrigin = p ? `${Math.round(p.x - r.left)}px ${Math.round(p.y - r.top)}px` : "50% 0";
  }, [zkey]);
  return (
    <div key={zkey} ref={ref} class={`ov-stage${dir ? ` ov-zoom-${dir}` : ""}`} data-ov-stage>
      {children}
    </div>
  );
}
