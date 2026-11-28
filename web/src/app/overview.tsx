// The project's Overview (Lift 4, #1480; v6's, on the kit): the page the
// app opens on. Two thirds and one third, fixed (panes come in Lift 9).
// The left holds the title line (the project, then how many sessions work,
// how many cards fail, how many cards were done today), Boards (each moving
// board a card: its mark, title, lane bar, count and what's going on in
// words; the quiet ones folded into one card two wide), three to a row as
// brand v2 drew them (#1567), and the Running line (what runs in the
// project and its CI runs, on to the Running drawer). The right third is Needs you, To do
// and a teammate's asks (./overview-needs.tsx) above the agent panel
// (./overview-talk.tsx), whose first tab is the chief of staff; moving
// through Needs you peeks each asker there.
//
// Its old zoom addresses (`/p/:project/board/:board`, `…/card/:row`,
// `…/work/:session`, Arugula #1287) still land: a board on its board, a
// card in the board's drawer over this page, a session in the panel. A card
// the address names (`?board=&card=`) opens the board's drawer over the
// page, so the path and Esc climb back. The grouping is
// ../shell/fleet-overview.ts; the machines are the machine page's
// (./machine-page), booted once.

import "./overview.css";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { BoardRow, BoardView, InboxItem } from "../shared/proto";
import { panelKey, type PanelTarget } from "../shared/kit/panel";
import type { OverviewZoom } from "../shared/platform-api";
import { boards, BoardError } from "../shared/boards/store";
import { Drawer } from "../shared/boards/card";
import { isCard, pointerLabel } from "../shared/boards/text";
import { chat } from "../shared/chat-store";
import { usePlatformApi } from "../shared/platform-api";
import { BY_CARD_EVERY_MS, sessionsByCard, type SessionsByCard } from "../shared/project-sessions";
import { Mark, type State } from "../shared/kit/state-mark";
import { Surface } from "../shared/kit/surface";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { ChevronIcon } from "../shared/ui/icons";
import type { Fleet } from "../shell/fleet";
import { kindOf } from "../shell/fleet-model";
import { boardSays, overviewOf, type OvBoard, type OvSession, type Standing } from "../shell/fleet-overview";
import { OverviewNeeds } from "./overview-needs";
import { askerOf, paneOfItem, paneTarget } from "./needs";
import { AgentPanel, CHIEF, ovPane } from "./panel";
import { useProjectInbox } from "../platform/inbox";
import { names } from "../platform/links";
import { runs } from "../platform/runs";
import { mayWrite } from "../platform/pages/issues";
import { Faces } from "../platform/pages/project";
import { useBoards } from "../platform/pages/boards";
import { follow, go, pathOf } from "../platform/router";
import { session as platform } from "../platform/session";
import { where } from "../platform/where";
import { boot } from "./machine-page";
import type { AppSession } from "./session";

export function OverviewPlace({ project, board, card, zoom, session }: { project: string; board?: string; card?: string; zoom?: OverviewZoom; session: AppSession | null }) {
  const [b, setB] = useState<Awaited<ReturnType<typeof boot>> | null>(null);
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
  return <OverviewBody project={project} board={board} card={card} zoom={zoom} fleet={b.machines.fleet} session={session} />;
}

/** The panel's tabs on this project's Overview, besides the chief of
 * staff's, and the one in front; kept for the browser tab. */
const TABS_MAX = 5;
function usePanelTabs(project: string) {
  const store = `arugula.ov-panel:${project}`;
  const [s, setS] = useState<{ targets: PanelTarget[]; current?: string }>(() => {
    try {
      const v = JSON.parse(sessionStorage.getItem(store) ?? "null") as { targets?: PanelTarget[]; current?: string } | null;
      if (v && Array.isArray(v.targets)) return { targets: v.targets.filter((t) => t && (t.kind === "agent" || t.kind === "thread")), current: v.current };
    } catch {
      // Unreadable: start empty.
    }
    return { targets: [] };
  });
  const put = (change: (was: typeof s) => typeof s) =>
    setS((was) => {
      const next = change(was);
      try {
        sessionStorage.setItem(store, JSON.stringify(next));
      } catch {
        // Not kept: it holds for this page.
      }
      return next;
    });
  return {
    ...s,
    /** Open a tab (or bring the one on the same thing to the front). */
    open: (t: PanelTarget) =>
      put(({ targets }) => {
        const k = panelKey(t);
        if (targets.some((x) => panelKey(x) === k)) return { targets: targets.map((x) => (panelKey(x) === k ? t : x)), current: k };
        return { targets: [...targets, t].slice(-TABS_MAX), current: k };
      }),
    close: (k: string) =>
      put(({ targets, current }) => {
        const i = targets.findIndex((x) => panelKey(x) === k);
        const left = targets.filter((x) => panelKey(x) !== k);
        // Closing the one in front brings its neighbour, else the chief of staff.
        const next = current !== k ? current : left.length ? panelKey(left[Math.min(i, left.length - 1)]!) : undefined;
        return { targets: left, current: next };
      }),
    pick: (k: string) => put((was) => ({ ...was, current: k === CHIEF ? undefined : k })),
  };
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

/** The hub's sessions by card, read again a minute later; null where the
 * hub has no such route yet, or it couldn't answer, and undefined until
 * it first answers (or doesn't). */
function useByCard(project: string): SessionsByCard | null | undefined {
  const [v, setV] = useState<SessionsByCard | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    const read = () =>
      sessionsByCard(project).then(
        (x) => live && setV(x),
        () => live && setV((was) => was ?? null),
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

/** How many of the project's CI runs are going, read again on refocus; 0 until it answers. */
function useCiRuns(project: string, refocus: number): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    let live = true;
    runs.list(project, { state: "running" }, 100).then(
      (l) => live && setN(l.runs.length),
      () => {},
    );
    return () => void (live = false);
  }, [project, refocus]);
  return n;
}

/** How a board stands, as a kit mark. */
const MARK: Record<Standing, State> = {
  needs: "asks",
  asks: "asks",
  failed: "failing",
  checks_failing: "failing",
  waits: "waits",
  quiet: "idle",
  working: "working",
  checks_running: "working",
  review: "working",
  idle: "idle",
};

/** A board is moving while it has cards in flight or something to say. */
const moving = (b: OvBoard) => b.doing.length + b.review.length > 0 || b.standing !== "idle";

/** Midnight today, here. */
const today = () => new Date(new Date().setHours(0, 0, 0, 0)).getTime();

function OverviewBody({ project, board, card, zoom, fleet, session }: { project: string; board?: string; card?: string; zoom?: OverviewZoom; fleet: Fleet; session: AppSession | null }) {
  useSubscribe((fn) => fleet.subscribe(fn));
  useSubscribe((fn) => platform.subscribe(fn));
  // The chief of staff's room shows as the panel's first tab once chat lists it.
  useSubscribe((fn) => chat.subscribe(fn));
  const api = usePlatformApi();
  const phone = usePhone();
  const refocus = useRefocus();
  const { views, put } = useBoardViews(project, refocus);
  const byCard = useByCard(project) ?? null;
  const hostOf = (m: string) => fleet.hostById(m)?.name ?? fleet.host(m)?.name;
  // Whose a machine is, for a session the hub names that no pane here shows: "" for one this page doesn't know.
  const machineOwner = (m: string) => {
    const h = fleet.hostById(m);
    if (!h) return "";
    const who = fleet.personOf(h);
    return who.kind === "me" || who.name === fleet.me ? undefined : who.name;
  };
  const ovPanes = fleet.panes.map((p) => ovPane(fleet, p));
  const o = overviewOf(project, views ?? [], ovPanes, byCard, hostOf, machineOwner);
  // Needs you is the project's Inbox (Arugula #1433), as the server says it, read again when the stream says it changed and on refocus.
  const inbox = useProjectInbox(project, String(refocus));
  const p = platform.project(project);
  const staffRoom = chat.rooms.find((r) => r.staff && r.project === project);

  // The old zoom addresses (#1287) land where their things live now.
  useEffect(() => {
    if (!zoom) return;
    if ("board" in zoom) go(pathOf(zoom.card ? { page: "project", project, board: zoom.board, card: zoom.card } : { page: "board", project, board: zoom.board }, names), true);
  }, [zoom && ("board" in zoom ? `${zoom.board}/${zoom.card}` : "")]);

  // The card the address opens: the board's drawer, over the page.
  const shownView = board ? views?.find((v) => v.board.id === board) : undefined;
  const opened = card ? shownView?.zones.flatMap((z) => z.rows).find((r) => r.id === card) : undefined;
  useEffect(() => {
    if (opened) where.cardIs(opened.id, cardWords(opened));
  }, [opened && cardWords(opened), opened?.id]);
  // The drawer's backdrop starts under the bar, so the path above it stays a way up while the card is open.
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

  // The panel's tabs: the chief of staff's first (the panel's own), then the agents and rooms opened here.
  const tabs = usePanelTabs(project);
  const ovCards = o.boards.flatMap((b) => [...b.doing, ...b.review]);
  const sessions = [...ovCards.flatMap((c) => c.sessions), ...o.loose];
  const leads = o.boards.flatMap((b) => (b.lead ? [b.lead.peek] : []));
  /** A session's rung: a board's lead, a worker on a card, or an agent. */
  const roleOf = (paneKey: string): "lead" | "worker" | "agent" =>
    leads.some((l) => l.panes.some((x) => x.key === paneKey)) ? "lead" : o.cardOf.get(paneKey) ? "worker" : "agent";
  /** A session's tab: its agent's pane (else the pane it opens), or a cloud workspace's session. */
  const targetOf = (s: OvSession): PanelTarget | null => {
    const ov = s.panes.find((x) => x.agent) ?? s.lead;
    const fp = ov && fleet.panes.find((x) => x.key === ov.key);
    if (fp) return paneTarget(fleet, fp, roleOf(fp.key), s.name);
    return s.hosted ? { kind: "agent", role: "worker", machine: "cloud", session: s.hosted, title: s.name } : null;
  };

  // A session by its old address (`/work/:session`, #1287, by its tile's key or the hub's id, #1380): in the panel.
  const zoomKey = zoom && "session" in zoom ? zoom.session : undefined;
  const zoomed = zoomKey ? [...sessions, ...leads].find((s) => s.key.slice(s.key.indexOf("|") + 1) === zoomKey || s.hub === zoomKey) : undefined;
  useEffect(() => {
    const t = zoomed && targetOf(zoomed);
    if (!t) return;
    tabs.open(t);
    go(pathOf({ page: "project", project }, names), true);
  }, [zoomed?.key]);

  // Moving through Needs you peeks the asker: its pane's session (a to-do's
  // block, ended too, #1468), or the chief of staff for the staff room's.
  const peek = (i: InboxItem) => {
    const asker = askerOf(fleet, i);
    const pane = paneOfItem(fleet, i) ?? asker.pane;
    if (pane) tabs.open(paneTarget(fleet, pane, roleOf(pane.key), sessions.find((x) => x.panes.some((y) => y.key === pane.key))?.name));
    else if (asker.target) tabs.open(asker.target);
    else if ((i.subject.room && i.subject.room === staffRoom?.id) || i.from.name?.startsWith("Chief of staff")) tabs.pick(CHIEF);
  };

  const moved = o.boards.filter(moving);
  const quiet = o.boards.filter((b) => !moving(b));
  const [quietOpen, setQuietOpen] = useState(false);
  const all = [...ovCards.flatMap((c) => c.sessions), ...o.loose];
  const working = all.filter((s) => s.standing === "working").length;
  const failing = ovCards.filter((c) => c.standing === "failed" || c.standing === "checks_failing").length;
  const since = today();
  const doneToday = (views ?? []).flatMap((v) => v.zones.flatMap((z) => z.rows)).filter((r) => r.lane === "done" && r.updated_at >= since).length;

  // Running: what runs in the project, by kind, and its CI runs going.
  const ciRuns = useCiRuns(project, refocus);
  const mine = ovPanes.filter((x) => x.project === project);
  const kinds = new Map<string, number>();
  for (const x of fleet.panes) if (mine.some((m) => m.key === x.key)) kinds.set(kindOf(x), (kinds.get(kindOf(x)) ?? 0) + 1);
  const failed = mine.filter((x) => x.standing === "failed").length;
  const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
  const running = [
    kinds.get("agent") && plural(kinds.get("agent")!, "agent"),
    kinds.get("server") && plural(kinds.get("server")!, "server"),
    kinds.get("shell") && plural(kinds.get("shell")!, "shell"),
    kinds.get("app") && plural(kinds.get("app")!, "app"),
    ciRuns && plural(ciRuns, "CI run"),
  ].filter(Boolean);
  const machines = pathOf({ page: "machines", project, at: { at: "machines", running: true } }, names);

  const tile = (b: OvBoard) => {
    const to = pathOf({ page: "board", project, board: b.id }, names);
    const says = boardSays(b);
    return (
      <Surface key={b.id} kind="card" as="li" class="ov-board" data-ov-board={b.id} data-ov-standing={b.standing}>
        <a class="ov-board-title" href={to} onClick={(e) => follow(e, to)} data-ov-board-title>
          <Mark state={MARK[b.standing]} />
          <span>{b.title}</span>
        </a>
        <div class="ov-board-bar">
          <span class="ov-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.total} aria-valuenow={b.done} aria-label={`${b.done} of ${b.total} done`}>
            <i style={{ width: `${b.total ? Math.round((100 * b.done) / b.total) : 0}%` }} />
          </span>
          <span class="m-meta" data-ov-board-count>
            {b.total ? `${b.done}/${b.total}` : "–"}
          </span>
        </div>
        <p class="ov-says m-meta" data-ov-board-says>
          {says.map((x, i) => [i > 0 && " · ", <span key={x.standing} data-standing={MARK[x.standing]}>{x.words}</span>])}
        </p>
      </Surface>
    );
  };

  const main = (
    <Surface kind="section" as="section" class="ov-main" aria-label="Overview" tabIndex={-1} data-ov-main data-work>
      <header class="ov-title" data-ov-title>
        <h1>{p?.name ?? "Overview"}</h1>
        {chat.on && p?.team && <Faces p={p} />}
        <p class="m-meta" data-ov-stats aria-busy={views === null || undefined}>
          {views === null ? (
            "Reading the boards…"
          ) : (
            <>
              <span data-standing="working">{working} working</span> · <span data-standing={failing ? "failing" : undefined}>{failing} failing</span> · {doneToday} done today
            </>
          )}
        </p>
      </header>
      <header class="ov-head">
        <h2>Boards</h2>
        <span class="m-meta">
          {moved.length} moving{quiet.length ? ` · ${quiet.length} quiet` : ""}
        </span>
        <span class="ov-head-end m-meta">↵ opens its desk</span>
      </header>
      {views !== null && o.boards.length === 0 ? (
        <p class="ov-quiet" data-ov-empty>
          No open boards.{" "}
          <a href={pathOf({ page: "boards", project }, names)} onClick={(e) => follow(e, pathOf({ page: "boards", project }, names))}>
            Boards
          </a>
        </p>
      ) : (
        <ul class="ov-boards" data-ov-boards aria-busy={views === null || undefined}>
          {moved.map(tile)}
          {quietOpen && quiet.map(tile)}
          {quiet.length > 0 && !quietOpen && (
            <li class="ov-board ov-folded" data-wide={moved.length % 3 !== 2 || undefined}>
              <button type="button" class="ov-board-title" aria-expanded={false} data-ov-quiet onClick={() => setQuietOpen(true)}>
                <Mark state="idle" />
                <span>{quiet.length} quiet</span>
                <ChevronIcon dir="right" size="xs" />
              </button>
              <p class="ov-says m-meta" data-ov-quiet-names>{quiet.map((b) => b.title).join(", ")}</p>
            </li>
          )}
        </ul>
      )}
      <a class="ov-running" href={machines} onClick={(e) => follow(e, machines)} data-ov-running>
        <b>Running</b>
        <span class="m-meta">
          {running.length ? running.join(" · ") : "nothing"}
          {failed ? (
            <>
              {" · "}
              <span data-standing="failing">{failed} failed</span>
            </>
          ) : null}
        </span>
        <span class="ov-head-end m-meta">open</span>
      </a>
    </Surface>
  );
  const needs = <OverviewNeeds items={inbox?.items ?? []} me={platform.me?.account} fleet={fleet} toast={(t) => api.toast(t)} peek={peek} />;
  const panel = (above?: typeof needs) => (
    <AgentPanel project={project} above={above} targets={tabs.targets} current={tabs.current} onPick={tabs.pick} onClose={tabs.close} fleet={fleet} session={session} />
  );
  return (
    <div class={phone ? "ov phone" : "ov"} data-overview={project} style={{ "--ov-bar-bottom": `${phone ? 0 : barBottom}px` }}>
      {phone ? (
        [needs, main, panel()]
      ) : (
        <>
          {main}
          <div class="ov-side" data-ov-side>
            {panel(needs)}
          </div>
        </>
      )}
      {opened && shownView && (
        <Drawer
          r={opened}
          platform={(path) => path}
          close={() => go(pathOf({ page: "project", project }, names))}
          project={project}
          others={shownView.zones.flatMap((z) => z.rows)}
          write={!!p && mayWrite(p)}
          act={act}
          id={shownView.board.id}
        />
      )}
    </div>
  );
}
