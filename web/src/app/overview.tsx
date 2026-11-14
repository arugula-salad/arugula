// The project's Overview (Arugula inversion, L6): Swarm for this project,
// the view the app opens on. The field on the left is the project's boards,
// each with the cards in Doing and Review as tiles (Next and Done are
// counts in its header, with its progress), each tile holding the sessions
// on that card side by side: their agent and title, where they run and
// their last line. Not on a card is the last group. Needs you on the right
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
import { Card } from "../shared/ui/card";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { StateChip, type Tone } from "../shared/ui/state-chip";
import type { Fleet, FleetPane } from "../shell/fleet";
import { activityOf, holdStill, kindOf, readableCommand, shortHost, shownNow, whatItDoes } from "../shell/swarm/model";
import { boardLine, overviewOf, statsLine, type OvBoard, type OvCard, type OvPane, type OvSession, type Standing } from "../shell/swarm/overview";
import { Card as RailCard, bundlesOf, type Bundle } from "../shell/swarm/view";
import { names } from "../platform/links";
import { mayWrite } from "../platform/pages/issues";
import { useBoards } from "../platform/pages/boards";
import { follow, go, pathOf } from "../platform/router";
import { session as platform } from "../platform/session";
import { where } from "../platform/where";
import { boot, type Booted } from "./machine-page";
import type { AppSession } from "./session";

/** How often a session's last line is read again, while the page is shown. */
const LINE_EVERY_MS = 6000;

/** How each standing reads on a card or a session, and its tone. */
const STANDING: Record<Standing, { word: string; tone: Tone }> = {
  needs: { word: "waiting on you", tone: "accent" },
  failed: { word: "failed", tone: "danger" },
  quiet: { word: "quiet", tone: "warn" },
  working: { word: "working", tone: "ok" },
  idle: { word: "no one on it", tone: "muted" },
};

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
        <BoardGroup key={b.id} b={b} project={project} fleet={fleet} openCard={openCard} openSession={openSession} opened={card} />
      ))}
      {o.loose.length > 0 && (
        <section class="ov-board ov-loose" data-ov-loose>
          <header class="ov-board-head">
            <b>Not on a card</b>
            <span class="dim">Sessions in this project that aren't working on a card</span>
          </header>
          <div class="ov-sessions">
            {o.loose.map((s) => (
              <SessionTile key={s.key} s={s} fleet={fleet} open={() => openSession(s)} />
            ))}
          </div>
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
    <div class={phone ? "ov phone" : "ov"} data-overview={project}>
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

/** A board: its title (to the board), what's in flight, next and done, its progress, then Doing and Review. */
function BoardGroup({ b, project, fleet, openCard, openSession, opened }: { b: OvBoard; project: string; fleet: Fleet; openCard: (c: OvCard) => void; openSession: (s: OvSession) => void; opened?: string }) {
  const to = pathOf({ page: "board", project, board: b.id }, names);
  const pct = b.total ? Math.round((b.done / b.total) * 100) : 0;
  return (
    <section class="ov-board" data-ov-board={b.id}>
      <header class="ov-board-head">
        <b data-ov-board-title>{b.title}</b>
        <span class="dim" data-ov-board-line>
          {boardLine(b)}
        </span>
        <a class="ov-board-open" href={to} onClick={(e) => follow(e, to)} data-ov-board-open>
          Open board
        </a>
      </header>
      <div class="ov-progress" role="progressbar" aria-label={`${b.done} of ${b.total} done`} aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
        <i style={{ width: `${pct}%` }} />
      </div>
      <div class="ov-lanes">
        {(
          [
            ["Doing", b.doing],
            ["Review", b.review],
          ] as const
        )
          .filter(([, cards]) => cards.length)
          .map(([name, cards]) => (
            <div key={name} class="ov-lane" data-ov-lane={name}>
              <h3>
                {name} · {cards.length}
              </h3>
              {cards.map((c) => (
                <CardTile key={c.row.id} c={c} fleet={fleet} open={() => openCard(c)} openSession={openSession} opened={opened === c.row.id} />
              ))}
            </div>
          ))}
      </div>
    </section>
  );
}

/** One card: its number and how it stands, its title, and the sessions on it side by side. A card waiting on you pulses. */
function CardTile({ c, fleet, open, openSession, opened }: { c: OvCard; fleet: Fleet; open: () => void; openSession: (s: OvSession) => void; opened: boolean }) {
  const r = c.row;
  const st = STANDING[c.standing];
  const label = pointerLabel(r.pointer, r.live);
  const title = isCard(r.live) ? r.live.title : r.note.split("\n").find((l) => l.trim()) ?? label;
  return (
    <Card
      as="article"
      tone={st.tone}
      selected={opened}
      class={c.standing === "needs" ? "ov-tile ov-pulse" : "ov-tile"}
      data-ov-card={r.id}
      data-ov-standing={c.standing}
      data-ov-panes={c.sessions.flatMap((s) => s.panes.map((p) => p.key)).join(" ")}
      onClick={open}
    >
      <div class="ov-tile-top">
        {label && <span class="dim">{label}</span>}
        <StateChip tone={st.tone} data-ov-card-state>
          {st.word}
        </StateChip>
      </div>
      <button type="button" class="ov-tile-title" data-ov-card-open onClick={(e) => (e.stopPropagation(), open())}>
        {title}
      </button>
      {c.sessions.length > 0 && (
        <div class="ov-sessions">
          {c.sessions.map((s) => (
            <SessionTile key={s.key} s={s} fleet={fleet} open={() => openSession(s)} />
          ))}
        </div>
      )}
    </Card>
  );
}

/** A session: its agent and title, where it runs and its last line. It opens full size, on its machine. */
function SessionTile({ s, fleet, open }: { s: OvSession; fleet: Fleet; open: () => void }) {
  const st = STANDING[s.standing];
  const can = !!s.lead || !!s.hosted;
  return (
    <button
      type="button"
      class={`ov-session tone-${st.tone}`}
      disabled={!can}
      title={can ? `${s.name}: open it` : `${s.name}, on ${s.where}: not reachable from here`}
      data-ov-session={s.key}
      data-ov-session-standing={s.standing}
      data-ov-panes={s.panes.map((p) => p.key).join(" ")}
      onClick={(e) => (e.stopPropagation(), open())}
    >
      <b>{s.name}</b>
      <span class="dim" data-ov-where>
        {shortHost(s.where)}
      </span>
      {s.lead ? <LastLine fleet={fleet} pane={s.lead} /> : <span class="ov-line dim">{st.word}</span>}
    </button>
  );
}

/** A pane's last line, read from its machine every few seconds while the page is shown. */
function LastLine({ fleet, pane }: { fleet: Fleet; pane: OvPane }) {
  const p = fleet.panes.find((x) => x.key === pane.key);
  const fallback = p?.info.reason?.headline ?? p?.info.current?.text ?? "";
  const [line, setLine] = useState<string | null>(null);
  useLayoutEffect(() => setLine(null), [pane.key]);
  useEffect(() => {
    let live = true;
    const read = async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const res = await fleet.request(pane.host, "GET", `/api/panes/${pane.id}/capture?format=text`);
        const text = res.ok && res.text ? await res.text() : "";
        const last = text.split("\n").map((l) => l.trimEnd()).filter(Boolean).pop();
        if (live && last) setLine(last);
      } catch {
        // Its machine isn't reachable now: the line it had stays.
      }
    };
    void read();
    const t = setInterval(() => void read(), LINE_EVERY_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [pane.key]);
  return (
    <span class="ov-line" data-ov-line>
      {line ?? fallback}
    </span>
  );
}
