// `/p/:project/sessions` (Fleet 5, inversion L3): every session in the
// project, wherever it runs, a computer's or a cloud workspace's, grouped
// by the card it works on (`sessions/by-card`), with its runs and cloud
// workspaces at the top. A group's head names its card, its board and its
// lane, with Open card; groups that need you come first, and Not on a card
// (lead sessions, scratch shells, release terminals) last. Finished
// sessions fold to a line per group. A hosted session's row is the wave's
// (`sessions`): its state, its issue, its box, branch and HEAD, when it
// last pushed, who started it, and what waits on the reader. A machine's
// session opens to its panes, each with its line, Open (the pane live,
// here) and Open on its machine. A cloud from before the by-card route
// (404) lists the wave alone, by its issues.
//
// It narrows by its issue's label, by a tracker (an issue's number: the
// issues its body names as #N, and those whose body names it), by who
// asked, and to the ones going. A row reads again when its session's live
// event comes (`chat.onObject`), and the list now and then while any goes.

import { Card } from "../../shared/ui/card";
import { useEffect, useState } from "preact/hooks";
import { boards } from "../../shared/boards/store";
import { MOVE_LANES } from "../../shared/boards/text";
import { usePlatformApi } from "../../shared/platform-api";
import { SectionHead } from "../../shared/ui/section-head";
import { BY_CARD_EVERY_MS, sessionsByCard, type SessionCard, type SessionsByCard } from "../../shared/project-sessions";
import { branchOf, cardPlace, cardTitle, finished, groupsOf, panesShown, readsBoards, stateOfListed, waitsOn, type Group, type Listed } from "../sessions-by-card";
import { chat } from "../../shared/chat-store";
import { stateOf } from "../../shared/chat/session-card";
import type { SessionRow, SessionWave } from "../../shared/proto.gen";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { BlockError, EmptyLine, EmptyState } from "../../shared/ui/block-states";
import { useSubscribe } from "../../shared/ui/hooks";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { PageHeader } from "../../shared/ui/page-header";
import { StateChip } from "../../shared/ui/state-chip";
import { Time } from "../../shared/ui/time";
import "../../shared/issues/issues.css";
import { names } from "../links";
import { waveQuery, type WaveFilter } from "../paths";
import { follow, go, pathOf } from "../router";
import { session, type PlatformProject } from "../session";
import { ListLoading } from "./issues";
import { Missing } from "./project";
import { Caret, CloseIcon } from "../../shared/ui/icons";

async function wave(project: string, f: WaveFilter): Promise<SessionWave> {
  const qs = waveQuery(f);
  const res = await platformFetch(
    `/api/projects/${encodeURIComponent(project)}/sessions${qs ? `?${qs}` : ""}`,
    { credentials: "same-origin" },
  );
  const j = (await res.json().catch(() => ({}))) as SessionWave & {
    error?: string;
  };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return j;
}

/** Where a card is on the project's boards: its board and row, its lane,
 * and the row's words (a card that isn't an issue is named by them). From
 * the by-card answer, `row` is the card's own id, which the board finds
 * its row by, and there are no words. */
interface Place {
  board: string;
  boardTitle: string;
  row: string;
  lane: string;
  words: string;
}

/** The project's boards' rows, by what they point at and by their own id,
 * read once a page, and only when `read`: the by-card answer places its
 * cards itself, so this is for a cloud from before it said so. None while
 * they're read, or if they can't be. */
function usePlaces(project: string, read: boolean): Map<string, Place> {
  const [m, setM] = useState<Map<string, Place>>(new Map());
  useEffect(() => {
    if (!read) return;
    let live = true;
    void boards
      .list(project)
      .then((l) => Promise.all(l.boards.map((b) => boards.get(b.id).catch(() => null))))
      .then((views) => {
        const out = new Map<string, Place>();
        for (const v of views)
          for (const z of v?.zones ?? [])
            for (const r of z.rows) {
              const p = r.pointer;
              const place = { board: v!.board.id, boardTitle: v!.board.title, row: r.id, lane: MOVE_LANES.find((x) => x.lane === r.lane)?.title ?? z.title, words: r.note };
              if ("id" in p && !out.has(p.id)) out.set(p.id, place);
              out.set(r.id, place);
            }
        if (live) setM(out);
      })
      .catch(() => {});
    return () => void (live = false);
  }, [project, read]);
  return m;
}

const menuAt = (e: MouseEvent, items: MenuItem[]) => {
  const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
  openMenu(
    {
      clientX: at.left,
      clientY: at.bottom + 4,
      preventDefault: () => e.preventDefault(),
    },
    items,
  );
};

export function SessionsPage({
  project,
  filter,
}: {
  project: string;
  filter: WaveFilter;
}) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p)
    return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <Wave p={p} filter={filter} />;
}

function Wave({ p, filter }: { p: PlatformProject; filter: WaveFilter }) {
  const [w, setW] = useState<SessionWave | null>(null);
  /** Every session by card: undefined until first read, null from a cloud
   * without the route (or a read that failed), when the wave is the list. */
  const [cards, setCards] = useState<SessionsByCard | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const key = JSON.stringify([p.id, filter]);
  useEffect(() => {
    let live = true;
    setError(null);
    wave(p.id, filter).then(
      (r) => live && setW(r),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [key, tick]);
  // The by-card list, as the Overview reads it: at most every
  // BY_CARD_EVERY_MS while the page is seen (the hub allows 600 reads an
  // hour), not on each hosted session's event.
  const [cardsTick, setCardsTick] = useState(0);
  useEffect(() => {
    let live = true;
    sessionsByCard(p.id).then(
      (c) => live && setCards(c),
      () => live && setCards((was) => was ?? null),
    );
    return () => void (live = false);
  }, [p.id, cardsTick]);
  useEffect(() => {
    const t = setInterval(() => document.visibilityState === "visible" && setCardsTick((n) => n + 1), BY_CARD_EVERY_MS);
    return () => clearInterval(t);
  }, [p.id]);
  // The boards are read only to place groups the by-card answer doesn't.
  const places = usePlaces(p.id, cards !== undefined && readsBoards(cards));
  const placeOf = (card: SessionCard | null): Place | undefined => {
    if (!card) return undefined;
    if (card.board === undefined) return places.get(card.id);
    const at = cardPlace(card);
    return at ? { ...at, row: card.id, words: "" } : undefined;
  };
  // Each session's live event reads the wave again; so does a new one,
  // now and then, while any goes.
  const ids = (w?.sessions ?? []).map((s) => s.session).join(" ");
  useEffect(() => {
    const again = () => setTick((n) => n + 1);
    const offs = ids
      ? ids.split(" ").map((id) => chat.onObject(id, again))
      : [];
    return () => offs.forEach((off) => off());
  }, [ids]);
  const narrowed = !!(filter.label || filter.tracker !== undefined || filter.asker);
  const groups = w && cards !== undefined ? groupsOf(cards, w, { narrowed, going: !!filter.going }) : null;
  const listed = groups?.flatMap((g) => g.sessions) ?? [];
  // A hosted session goes until its turn settles; another, until it finishes.
  const isGoing = (s: Listed) => (s.wave ? !s.wave.settled : !finished(s));
  // The wave is read again now and then while a hosted session goes.
  const going = listed.some((s) => !!s.wave && isGoing(s));
  useEffect(() => {
    if (!going) return;
    const t = setInterval(
      () => document.visibilityState === "visible" && setTick((n) => n + 1),
      15_000,
    );
    return () => clearInterval(t);
  }, [going]);

  const to = (f: WaveFilter) =>
    go(pathOf({ page: "sessions", project: p.id, ...f }, names));
  const labelMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Its issue's label" },
      {
        label: "Any label",
        checked: !filter.label,
        run: () => to({ ...filter, label: undefined }),
      },
      ...(w?.labels ?? []).map((l) => ({
        label: l,
        checked: filter.label === l,
        run: () => to({ ...filter, label: l }),
      })),
    ]);
  const askerMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Asked by" },
      {
        label: "Anyone",
        checked: !filter.asker,
        run: () => to({ ...filter, asker: undefined }),
      },
      {
        label: "Me",
        checked: filter.asker === "me",
        run: () => to({ ...filter, asker: "me" }),
      },
      ...(w?.askers ?? []).map(([a, name]) => ({
        label: name || a,
        checked: filter.asker === a,
        run: () => to({ ...filter, asker: a }),
      })),
    ]);
  const anyFilter = narrowed || !!filter.going;
  const askerName =
    filter.asker === "me"
      ? "me"
      : (w?.askers.find(([a]) => a === filter.asker)?.[1] ?? "one");
  const waiting = listed.filter((s) => !!waitsOn(s)).length;
  const runsTo = pathOf({ page: "runs", project: p.id }, names);
  const workspacesTo = pathOf({ page: "workspaces", project: p.id }, names);

  return (
    <section
      class="platform-section sw-page sessions-page"
      data-sessions={p.id}
    >
      <PageHeader
        title="Sessions"
        // Runs and cloud workspaces left the bar (Arugula #989, decision 6): they're here.
        actions={
          <>
            <a class="secondary" href={runsTo} data-sessions-runs onClick={(e) => follow(e, runsTo)}>
              Runs
            </a>
            {session.hosted && (
              <a class="secondary" href={workspacesTo} data-sessions-workspaces onClick={(e) => follow(e, workspacesTo)}>
                Workspaces
              </a>
            )}
          </>
        }
        sub="Every session in the project, on a computer or in a cloud workspace, by the card it works on: where each runs, its branch and last push, and what waits on you. Narrow it to a track by its issues' label, or by a tracker issue."
        stats={
          listed.length > 0 && (
            <span class="dim" data-sessions-stats>
              {listed.filter(isGoing).length} going · {waiting} waiting on you
            </span>
          )
        }
      />
      <div class="issues-tools">
        <button
          class="secondary"
          aria-pressed={!!filter.label}
          data-sessions-label
          onClick={labelMenu}
        >
          {filter.label ? `Label: ${filter.label}` : "Label"}{" "}
          <Caret />
        </button>
        <TrackerFilter filter={filter} to={to} tracker={w?.tracker ?? null} />
        <button
          class="secondary"
          aria-pressed={!!filter.asker}
          data-sessions-asker
          onClick={askerMenu}
        >
          {filter.asker ? `Asked by ${askerName}` : "Asked by"}{" "}
          <Caret />
        </button>
        <label class="issues-filter">
          <input
            type="checkbox"
            checked={!!filter.going}
            data-sessions-going
            onChange={(e) =>
              to({
                ...filter,
                going:
                  (e.currentTarget as HTMLInputElement).checked || undefined,
              })
            }
          />{" "}
          Going only
        </label>
        {anyFilter && (
          <button class="link" data-sessions-clear onClick={() => to({})}>
            Clear
          </button>
        )}
      </div>
      {w === null && error ? (
        <BlockError inline title="Can't read this project's sessions." detail={error} retry={() => (setTick((n) => n + 1), setCardsTick((n) => n + 1))} />
      ) : (
        error && (
          <p class="control-error" role="alert">
            {error}
          </p>
        )
      )}
      {groups === null && !error && <ListLoading />}
      {groups?.length === 0 &&
        (anyFilter ? (
          <EmptyLine data-sessions-none>No sessions match.</EmptyLine>
        ) : (
          <EmptyState
            where="page"
            data-sessions-none
            title="No sessions yet."
            why="Agent on this on an issue, or @arugula in a channel, starts one. A session on your computer shows here once it's in this project."
          />
        ))}
      <div data-sessions-list>
        {groups?.map((g) => (
          <CardGroup key={g.card?.id ?? ""} p={p} g={g} place={placeOf(g.card)} />
        ))}
      </div>
      {w?.more && <p class="dim">More match than are listed: narrow it.</p>}
    </section>
  );
}

/** The tracker it's narrowed to, or a field for an issue's number. */
function TrackerFilter({
  filter,
  to,
  tracker,
}: {
  filter: WaveFilter;
  to: (f: WaveFilter) => void;
  tracker: SessionWave["tracker"];
}) {
  const [text, setText] = useState("");
  if (filter.tracker !== undefined)
    return (
      <button
        class="secondary"
        aria-pressed="true"
        data-sessions-tracker={filter.tracker}
        title="Show every session again"
        onClick={() => to({ ...filter, tracker: undefined })}
      >
        Tracker #{filter.tracker}
        {tracker ? ` ${tracker.title}` : ""} <CloseIcon size="xs" />
      </button>
    );
  return (
    <form
      class="issues-filter"
      onSubmit={(e) => {
        e.preventDefault();
        const n = Number(text.replace(/^#/, ""));
        if (Number.isInteger(n) && n > 0) to({ ...filter, tracker: n });
      }}
    >
      <input
        size={9}
        placeholder="Tracker #N"
        aria-label="A tracker issue's number"
        value={text}
        data-sessions-tracker-input
        onInput={(e) => setText((e.currentTarget as HTMLInputElement).value)}
      />
    </form>
  );
}

/** A card's sessions: its head (the card, its board and lane, Open card),
 * the going ones, then the finished ones folded to a line. */
function CardGroup({ p, g, place }: { p: PlatformProject; g: Group; place: Place | undefined }) {
  const [open, setOpen] = useState(false);
  const done = g.sessions.filter(finished);
  const live = g.sessions.filter((s) => !finished(s));
  const card = cardPath(p.id, g.card, place);
  return (
    <section class="sessions-group" data-sessions-group={g.card?.id ?? "none"}>
      <SectionHead
        title={cardTitle(g.card, place?.words)}
        count={g.sessions.length}
        titleData="sessions-card"
        action={
          <>
            {place && (
              <span class="dim" data-sessions-card-place>
                {place.boardTitle}
                {place.lane && ` · ${place.lane}`}
              </span>
            )}
            {card && (
              <a class="secondary small" href={card} data-sessions-open-card onClick={(e) => follow(e, card)}>
                Open card
              </a>
            )}
          </>
        }
      />
      <ul class="issues-list card-list">
        {live.map((s) => (
          <SessionLine key={s.key} p={p} s={s} />
        ))}
        {done.length > 0 && !open && (
          <li class="sessions-folded">
            <button type="button" class="link" data-sessions-finished={done.length} onClick={() => setOpen(true)}>
              {done.length} finished
            </button>
          </li>
        )}
        {open && done.map((s) => <SessionLine key={s.key} p={p} s={s} />)}
      </ul>
    </section>
  );
}

/** Where Open card goes: the card's row on its board, whose drawer opens
 * there, else its issue's page; nothing for Not on a card. */
function cardPath(project: string, card: SessionCard | null, place: Place | undefined): string | null {
  if (!card) return null;
  if (place) return `${pathOf({ page: "board", project, board: place.board }, names)}#row=${encodeURIComponent(place.row)}`;
  return card.id.startsWith("iss_") ? pathOf({ page: "issue", project, issue: card.id }, names) : null;
}

function SessionLine({ p, s }: { p: PlatformProject; s: Listed }) {
  if (s.wave) return <Row p={p} s={s.wave} />;
  if (s.src?.kind === "hosted") return <HostedLine p={p} s={s} />;
  return <MachineRow s={s} />;
}

/** A hosted session the wave hasn't listed: its name and state. */
function HostedLine({ p, s }: { p: PlatformProject; s: Listed }) {
  const src = s.src!;
  const st = stateOf(stateOfListed(s), false);
  const id = String(src.id);
  const branch = branchOf(src);
  const path = id.startsWith("ses_") ? pathOf({ page: "session", project: p.id, session: id }, names) : null;
  return (
    <Card as="li" class="issue-row" tone={st.tone} data-session-row={id} data-session-state={stateOfListed(s)}>
      <div class="issue-row-main">
        {path ? (
          <a href={path} class="issue-row-title" onClick={(e) => follow(e, path)}>
            {src.name}
          </a>
        ) : (
          <b>{src.name}</b>
        )}
        <span class="issue-row-meta dim">
          <StateChip tone={st.tone}>{st.label}</StateChip> · in a cloud workspace
          {branch && (
            <>
              {" · "}
              <code data-session-branch>{branch}</code>
            </>
          )}
        </span>
      </div>
    </Card>
  );
}

/** A pane's kind, in people's words. */
const KIND: Record<string, string> = { agent: "Agent", terminal: "Terminal", shell: "Terminal", browser: "Browser", editor: "Editor", doc: "Doc", board: "Board" };

/** A session on a computer: its state, name and panes, where it runs and
 * whose that is, its branch, and what waits on you. A click shows its
 * panes, each with its line, Open and Open on its machine. */
function MachineRow({ s }: { s: Listed }) {
  const api = usePlatformApi();
  const [open, setOpen] = useState(false);
  const [live, setLive] = useState<{ machine: string; pane: number; name: string } | null>(null);
  const Live = api.LivePane;
  const [, tick] = useState(0);
  const src = s.src!;
  const panes = panesShown(src);
  // The machines' panes are read while the page shows any.
  useEffect(() => api.panes?.watch(() => tick((n) => n + 1)), [api.panes]);
  const on = src.on.kind === "machine" ? src.on : null;
  const owner = on ? api.panes?.owner(on.id) : undefined;
  // Its branch as the cloud says it, else as its machine's panes do.
  const branch = branchOf(src) ?? panes.map((x) => api.panes?.pane(x.machine, x.pane)?.branch).find(Boolean);
  const state = stateOfListed(s);
  const st = stateOf(state, false);
  const waits = waitsOn(s);
  // Open: the pane live over the page, where the page draws panes (the
  // app's LivePane, as the card drawer's); else, and Open on its machine,
  // its machine's page on it.
  const goTo = (machine: string, pane: number, alone: boolean) => {
    if (alone && Live) return setLive({ machine, pane, name: `${KIND[panes.find((x) => x.pane === pane)?.kind ?? ""] ?? "Pane"} %${pane}` });
    api.goToPane(machine, String(pane), null);
  };
  const toggle = () => setOpen((o) => !o);
  return (
    <Card as="li" class="issue-row sessions-machine" tone={st.tone} data-session-row={s.key} data-session-state={state} aria-expanded={open}>
      <div class="issue-row-main">
        <button type="button" class="link issue-row-title" data-session-toggle onClick={toggle}>
          {src.name || (src.kind === "agent" ? "Agent" : "Session")}
        </button>
        <span class="dim">{panes.length === 1 ? "1 pane" : `${panes.length} panes`}</span>
        {waits && (
          <StateChip tone="warn" data-session-waiting>
            Waits on you: {waits}
          </StateChip>
        )}
        <span class="issue-row-meta dim">
          {state && <StateChip tone={st.tone}>{st.label}</StateChip>}
          {on && (
            <span data-session-where>
              {state ? " · on " : "on "}
              {on.name}
              {owner === null ? ", yours" : owner ? `, ${owner}'s` : ""}
            </span>
          )}
          {branch && (
            <>
              {" · "}
              <code data-session-branch>{branch}</code>
            </>
          )}
          {src.updated_at > 0 && (
            <>
              {" · changed "}
              <Time at={src.updated_at} />
            </>
          )}
        </span>
        {open && (
          <ul class="sessions-panes" data-session-panes>
            {panes.map((x) => {
              const seen = api.panes?.pane(x.machine, x.pane);
              const pst = x.state ? stateOf(x.state, false) : null;
              return (
                <li key={x.pane} data-session-pane={x.pane}>
                  <span class="sessions-pane-name">
                    {KIND[x.kind] ?? x.kind} <code>%{x.pane}</code>
                  </span>
                  {pst && <StateChip tone={pst.tone}>{pst.label}</StateChip>}
                  <span class="sessions-pane-line dim" data-session-pane-line>
                    {seen?.line ?? ""}
                  </span>
                  <button type="button" class="primary small" data-session-open onClick={() => goTo(x.machine, x.pane, true)}>
                    Open
                  </button>
                  <button type="button" class="secondary small" data-session-on-machine onClick={() => goTo(x.machine, x.pane, false)}>
                    Open on its machine
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
      {live && Live && (
        <LiveSheet name={live.name} where={on?.name ?? ""} close={() => setLive(null)} onMachine={() => (setLive(null), goTo(live.machine, live.pane, false))}>
          <Live machine={live.machine} pane={String(live.pane)} />
        </LiveSheet>
      )}
    </Card>
  );
}

/** A pane live and full size over the page, with Open on its machine and
 * Close. Escape is the pane's: Close is the button. */
function LiveSheet({ name, where, close, onMachine, children }: { name: string; where: string; close: () => void; onMachine: () => void; children: preact.ComponentChildren }) {
  return (
    <div class="sessions-live" role="dialog" aria-label={`${name} on ${where}`} data-sessions-live>
      <header class="sessions-live-head">
        <b>{name}</b>
        <span class="dim">on {where}</span>
        <span class="sessions-live-gap" />
        <button type="button" class="secondary small" data-sessions-live-machine onClick={onMachine}>
          Open on its machine
        </button>
        <button type="button" class="secondary small" data-sessions-live-close onClick={close}>
          Close
        </button>
      </header>
      <div class="sessions-live-body">{children}</div>
    </div>
  );
}

function Row({ p, s }: { p: PlatformProject; s: SessionRow }) {
  const path = pathOf(
    { page: "session", project: p.id, session: s.session },
    names,
  );
  const issue = s.issue
    ? pathOf({ page: "issue", project: p.id, issue: s.issue.id }, names)
    : null;
  const st = stateOf(s.state, false);
  return (
    <Card
      as="li"
      class="issue-row"
      tone={st.tone}
      data-session-row={s.session}
      data-session-state={s.state}
    >
      <div class="issue-row-main">
        <a href={path} class="issue-row-title" onClick={(e) => follow(e, path)}>
          {s.issue ? `#${s.issue.number} ${s.issue.title}` : s.branch}
        </a>
        {s.waiting && (
          <StateChip tone="warn" data-session-waiting>
            Waits on you: {s.waiting}
          </StateChip>
        )}
        <span class="issue-row-meta dim">
          <StateChip tone={st.tone}>{st.label}</StateChip> · on{" "}
          {s.workspace_name || s.workspace} · <code>{s.branch}</code>
          {s.head ? (
            <>
              {" "}
              at <code>{s.head}</code>
            </>
          ) : null}
          {" · "}
          {s.pushed_at ? (
            <>
              pushed <Time at={s.pushed_at} />
            </>
          ) : (
            "not pushed"
          )}
          {" · "}
          {s.started_by ?? `asked by ${s.asker_name || "someone"}`}
          {s.pr && (
            <>
              {" · "}
              <a href={s.pr} target="_blank" rel="noopener">
                {s.merged_at ? "PR merged" : "PR"}
              </a>
            </>
          )}
          {issue && (
            <>
              {" · "}
              <a href={issue} onClick={(e) => follow(e, issue)}>
                its issue
              </a>
            </>
          )}
        </span>
        {s.question && (
          <span class="issue-row-meta">It asks: {s.question}</span>
        )}
      </div>
      <code class="card-id" title={s.session}>
        {s.session.replace(/^ses_/, "").slice(0, 8)}
      </code>
    </Card>
  );
}
