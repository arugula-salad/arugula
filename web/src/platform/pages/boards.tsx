// `/p/:project/boards` and `/p/:project/boards/:board` (boards,
// step 2): a project's boards, and one, on the list page (Arugula #1128): the
// boards in the list, the one selected in the middle. `/boards` (#339): every project's,
// a group a project on the same list (Arugula #1131), where the rail's Boards goes. A board is the platform's object
// (crates/platform/src/boards.rs): zones of rows, each pointing at an issue,
// a PR, a run, a workspace or an agent block, with a note and a status.
// The view is shared with the board block (../../shared/boards/view).
// Boards are on for everyone (they left Labs, #191).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { ProjectBoard } from "../../shared/proto";
import { BoardPanel } from "../../shared/boards/view";
import { openTab } from "../../shared/docs/doc-actions";
import { DocPage } from "./docs";
import { boards } from "../../shared/boards/store";
import { boardMatches, epicLine, firstLine, NOT_STARTED } from "../../shared/boards/text";
import { useSubscribe } from "../../shared/ui/hooks";
import { BlockLoading, EmptyLine, EmptyState } from "../../shared/ui/block-states";
import { Card } from "../../shared/ui/card";
import { ListPage, TIME_SORTS, useListSort, type ListGroup, type ListRowParts, type TimeSort } from "../../shared/ui/list-page";
import { Modal } from "../../shared/ui/modal";
import { PageHeader } from "../../shared/ui/page-header";
import { SearchField } from "../../shared/ui/search-field";
import { homes, names } from "../links";
import { follow, go, pathOf } from "../router";
import { SectionHead } from "../../shared/ui/section-head";
import { controlOrigin, session, type PlatformProject } from "../session";
import { blindLine, blindMachines, type Grant } from "../settings/grants";
import { toast } from "../toast";
import { roleIn } from "../settings/environments";
import { IssueList, mayWrite, useIssueHome } from "./issues";
import type { IssueFilter } from "../../shared/issues/text";
import { Missing } from "./project";
import { currentProject } from "../current-project";
import { where } from "../where";
import { NotOnBoard, dropWhere, useTriage } from "./triage";

/** The reads in flight, by project: the bar, the project's home and a page
 * all ask for the same list at once, and share the one request. */
const reading = new Map<string, Promise<ProjectBoard[]>>();
function readBoards(project: string): Promise<ProjectBoard[]> {
  let r = reading.get(project);
  if (!r) {
    r = boards.list(project).then((l) => l.boards);
    reading.set(project, r);
    const done = () => void reading.delete(project);
    r.then(done, done);
  }
  return r;
}

/** The lists on screen, by project: `boardsChanged` tells them to read again. */
const watching = new Map<string, Set<() => void>>();

/** Read `project`'s boards again with `read` when they change. */
function watch(project: string, read: () => void) {
  const at = watching.get(project) ?? new Set();
  at.add(read);
  watching.set(project, at);
}

/** The project's boards moved (a row was added): every `useBoards` of it reads them again, keeping what it shows until they answer. */
export function boardsChanged(project: string): void {
  for (const again of watching.get(project) ?? []) again();
}

/** The project's boards, `null` while they load, none when they can't be
 * read. Asked of no project (`undefined`), it reads nothing. */
export function useBoards(project: string | undefined): ProjectBoard[] | null {
  const [list, setList] = useState<ProjectBoard[] | null>(null);
  useEffect(() => {
    setList(null);
    if (!project) return;
    let live = true;
    const read = () =>
      readBoards(project).then(
        (l) => live && setList(l),
        () => live && setList((was) => was ?? []),
      );
    void read();
    watch(project, read);
    return () => {
      live = false;
      watching.get(project)?.delete(read);
    };
  }, [project]);
  return list;
}

/** The sort this browser left on, for both lists of boards: Updated until someone chooses Created (Arugula #1369). */
const useBoardSort = () => useListSort<TimeSort>("boards-sort", TIME_SORTS);

/** `boards` newest first by the time they're sorted by. The sort is stable, so equal times keep their order. */
const newest = (boards: ProjectBoard[], by: TimeSort) => [...boards].sort((a, b) => (by === "created" ? b.created_at - a.created_at : b.updated_at - a.updated_at));

/** The filter box above a list of boards (#546): the page's search (`/`). */
function BoardFilter({ q, setQ }: { q: string; setQ: (q: string) => void }) {
  return <SearchField class="board-filter" value={q} onValue={setQ} label="Filter boards" placeholder="Filter boards by title, epic key or goal" slash data-boards-filter />;
}

/** `/boards` and `/boards/:board`, every project's boards on one list
 * until Lift 5a (Go to… finds a board now): kept as addresses (aru-082).
 * `/boards` goes to the current project's Boards (else your first
 * project's); a board's goes to its page, once one of your projects says
 * it's there, and says so when none does. */
export function AllBoardsPage({ board }: { board?: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const [lost, setLost] = useState(false);
  const ids = session.projects.map((p) => p.id);
  const key = ids.join(",");
  useEffect(() => {
    if (!ids.length) return;
    if (!board) {
      const here = currentProject();
      return void go(pathOf({ page: "boards", project: here && ids.includes(here) ? here : ids[0]! }, names), true);
    }
    let live = true;
    setLost(false);
    void Promise.all(ids.map((p) => readBoards(p).then((l) => (l.some((b) => b.id === board) ? p : null), () => null))).then((homes) => {
      const home = homes.find(Boolean);
      if (!live) return;
      if (home) go(pathOf({ page: "board", project: home, board }, names), true);
      else setLost(true);
    });
    return () => void (live = false);
  }, [board, key]);
  if (session.state === "ready" && !ids.length) return <EmptyState where="page" title="No boards yet." why="A board follows a goal across its issues and PRs. Make one on a project's Boards page." />;
  if (lost) return <EmptyState where="page" data-all-boards-missing={board} title="That board isn't here." why="It's in none of your projects, or it's gone." />;
  return <BlockLoading inline what="boards" />;
}

/** `/p/:project/boards`, `/p/:project/boards/:board` (that board's row
 * selected) and `/p/:project/triage` (Not on a board's row selected): one page,
 * so going between them keeps the list as it is. */
export function BoardsPage({ project, board, triage, issues }: { project: string; board?: string; triage?: boolean; issues?: IssueFilter }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return board ? <BoardDesk key={board} p={p} id={board} /> : <BoardList p={p} triage={triage} issues={issues} />;
}

/** The names of the machines of the reader's whose agents can't see the project's boards (#313); none while the grants load or when they can't be read. */
function useBlindMachines(project: string, may: boolean): string[] {
  const [blind, setBlind] = useState<string[]>([]);
  useEffect(() => {
    setBlind([]);
    if (!may) return;
    let live = true;
    void platformFetch("/api/grants", { credentials: "same-origin" })
      .then((r) => (r.ok ? (r.json() as Promise<{ grants: Grant[] }>) : null))
      .then((v) => live && v && setBlind(blindMachines(v.grants, project)))
      .catch(() => {});
    return () => void (live = false);
  }, [project, may]);
  return blind;
}

/** A quiet line under the header when some of the reader's machines have no Boards grant over the project (#313): their agents' first `board_read` would be refused. It only points at Settings → This machine → Agents' grants, where the grant is given. */
function BlindMachines({ project, may }: { project: string; may: boolean }) {
  const line = blindLine(useBlindMachines(project, may));
  if (!line) return null;
  const to = pathOf({ page: "settings", scope: "machine", section: "agents" }, names);
  return (
    <p class="dim" data-boards-blind>
      {line}{" "}
      <a href={to} data-boards-blind-link onClick={(e) => follow(e, to)}>
        Let them
      </a>
    </p>
  );
}

/** Not on a board's row: pinned first in the Boards list (Arugula #1130), whatever the search, while it has any (Lift 5a). */
const LOOSE = { loose: true } as const;
const LOOSE_KEY = "loose";
/** All issues' row, last before Done (Lift 5a): the project's issues, the Issues page that was. */
const ALL = { all: true } as const;
const ALL_KEY = "all";
type BoardRow = ProjectBoard | typeof LOOSE | typeof ALL;
const isLoose = (r: BoardRow): r is typeof LOOSE => "loose" in r;
const isAll = (r: BoardRow): r is typeof ALL => "all" in r;

/** A project's boards on the list page (Arugula #1128): a row a board, its
 * progress as `13/18` and the lanes' bar, From Plane and Done (folded) as
 * groups; the board itself in the middle. Not on a board is the first row,
 * with its count (Arugula #1130): selected, its issues go in the middle, and
 * one dragged onto a board's row goes on that board. */
function BoardList({ p, triage, issues }: { p: PlatformProject; triage?: boolean; issues?: IssueFilter }) {
  const list = useBoards(p.id);
  const [q, setQ] = useState("");
  const [making, setMaking] = useState(false);
  const [unfolded, setUnfolded] = useState(false);
  const [sort, setSort] = useBoardSort();
  const tri = useTriage(p.id, () => boardsChanged(p.id));
  const all = list ?? [];
  const open = all.filter((b) => b.closed_at == null);
  const shown = newest(all.filter((b) => boardMatches(b, q)), sort);
  const done = shown.filter((b) => b.closed_at != null);
  const live = shown.filter((b) => b.closed_at == null);
  const made = live.filter((b) => !b.source);
  const plane = live.filter((b) => b.source);
  const fromPlane = [...plane.filter((b) => b.source?.started), ...plane.filter((b) => !b.source?.started)];
  const total = tri.whole;
  const home = useIssueHome(p.id);
  const groups: ListGroup<BoardRow>[] = [
    // Not on a board only while it has issues (or its own address asks for it).
    { key: LOOSE_KEY, items: triage || (total ?? 0) > 0 ? [LOOSE] : [] },
    { items: made },
    { title: "From Plane", key: "plane", items: fromPlane, attrs: { "data-boards-plane": "" } },
    { key: ALL_KEY, items: [ALL] },
    { title: "Done", key: "done", items: done, fold: { open: unfolded || !!q.trim(), toggle: () => setUnfolded(!unfolded) }, attrs: { "data-boards-done": "" } },
  ];
  const boardPath = (id: string) => pathOf({ page: "board", project: p.id, board: id }, names);
  const loosePath = pathOf({ page: "triage", project: p.id }, names);
  const allPath = pathOf({ page: "issues", project: p.id }, names);
  // Not on a board's row and All issues' are selected by their own addresses; a board opens on its own page (Lift 5a).
  const select = triage ? LOOSE_KEY : issues ? ALL_KEY : null;
  const pick = (r: BoardRow) => (isLoose(r) ? go(loosePath) : isAll(r) ? go(allPath) : go(boardPath(r.id)));
  const status =
    list === null ? (
      <BlockLoading inline what="boards" />
    ) : all.length === 0 ? (
      <EmptyState where="page" title="No boards yet." why={`A board follows a goal across its issues and PRs.${mayWrite(p) ? " Make one with New board." : ""}`} />
    ) : shown.length === 0 ? (
      <EmptyLine data-boards-filter-none>No board matches “{q.trim()}”.</EmptyLine>
    ) : undefined;
  return (
    <div class="boards-page" data-boards-page data-boards {...dropWhere(tri, (e) => {
      const el = (e.target as Element | null)?.closest?.<HTMLElement>("[data-board-item]");
      const b = el && open.find((x) => x.id === el.dataset.boardItem);
      return el && b ? { el, board: b } : null;
    })}>
      <ListPage<BoardRow>
        label="Boards"
        splitKey="platform-boards"
        header={
          <>
            <PageHeader
              title="Boards"
              actions={
                mayWrite(p) && (
                  <button type="button" class="secondary small" data-board-new-open onClick={() => setMaking(true)}>
                    New board
                  </button>
                )
              }
            />
            <BlindMachines project={p.id} may={roleIn(p) !== "viewer"} />
            {making && <NewBoard p={p} done={() => setMaking(false)} />}
          </>
        }
        searchLabel="Filter boards by title, epic key or goal"
        query={q}
        setQuery={setQ}
        sort={{ options: TIME_SORTS, value: sort, onSort: setSort }}
        groups={groups}
        keyOf={(r) => (isLoose(r) ? LOOSE_KEY : isAll(r) ? ALL_KEY : r.id)}
        previewOn="click"
        select={select}
        opened={!!triage || !!issues}
        // The address follows the row, so a board's link is the page with its row selected; from Not on a board, Back returns there.
        onSelect={pick}
        onOpen={pick}
        // Under Not on a board's row, which is always there.
        below={status && <div class="list-status">{status}</div>}
        row={(r): ListRowParts => {
          if (isLoose(r))
            return {
              dense: true,
              name: "Not on a board",
              title: "Not on a board",
              age: total !== null && <span data-boards-loose-n>{total.toLocaleString()}</span>,
              attrs: { "data-boards-loose": "" },
            };
          if (isAll(r))
            return {
              dense: true,
              name: "All issues",
              title: "All issues",
              age: home && <span data-boards-all-n>{home.native_issues.toLocaleString()}</span>,
              attrs: { "data-boards-all": "" },
            };
          return { ...boardRow(r), attrs: { "data-board-item": r.id, ...(r.closed_at == null ? { "data-drop-target": "" } : {}) } };
        }}
        preview={(r) => (isLoose(r) ? <NotOnBoard p={p} tri={tri} open={open} /> : isAll(r) ? <IssueList p={p} filter={issues ?? {}} /> : null)}
      />
    </div>
  );
}

/** A board's row, on a project's Boards and on every project's (Arugula
 * #1128, #1131): its title, its progress as `13/18` and the lanes' bar; a
 * Plane epic's not started yet says so. */
function boardRow(b: ProjectBoard): ListRowParts {
  const e = epicLine(b.progress);
  return {
    dense: true,
    name: b.title,
    title: <span data-board-title>{b.title}</span>,
    tags: b.source && !b.source.started && b.closed_at == null && (
      <span class="dim board-item-waiting" data-board-not-started>
        {NOT_STARTED}
      </span>
    ),
    age: (
      <span title={e.words} data-board-tally>
        {b.progress.done}/{b.progress.total}
      </span>
    ),
    mark: b.progress.total > 0 && (
      <span class="board-bar board-item-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.progress.total} aria-valuenow={b.progress.done} aria-label={e.words} data-board-progress>
        {e.segments.map((s) => (
          <i key={s.lane} data-lane={s.lane} style={{ width: `${s.pct}%` }} />
        ))}
      </span>
    ),
    attrs: { "data-board-item": b.id },
  };
}

/** New board: a title, in the page's project, then its row selected. */
function NewBoard({ p, done }: { p: PlatformProject; done: () => void }) {
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const make = async (e: Event) => {
    e.preventDefault();
    if (!title.trim() || busy) return;
    setBusy(true);
    try {
      const v = await boards.create(p.id, title.trim());
      done();
      boardsChanged(p.id);
      go(pathOf({ page: "board", project: p.id, board: v.board.id }, names));
    } catch (err) {
      toast(`Couldn't make the board: ${(err as Error).message}`);
      setBusy(false);
    }
  };
  return (
    <Modal
      title="New board"
      close={done}
      onSubmit={(e) => void make(e)}
      root={{ "data-board-new-dialog": true }}
      buttons={
        <>
          <button type="button" class="secondary" onClick={done}>
            Cancel
          </button>
          <button type="submit" class="primary" data-board-new disabled={busy || !title.trim()}>
            Make a board
          </button>
        </>
      }
    >
      <input class="field" type="text" value={title} maxLength={120} placeholder="Its title" aria-label="New board's title" data-board-new-title autoFocus onInput={(e) => setTitle(e.currentTarget.value)} />
    </Modal>
  );
}

/** One board on its own page (Lift 5a): the desk, with the board's Docs as tabs (drawn as the Docs page draws one) and Workers in its ⋯. */
function BoardDesk({ p, id }: { p: PlatformProject; id: string }) {
  return (
    <div class="boards-board" data-board-page={id}>
      <BoardPanel
        placement="desk"
        id={id}
        write={mayWrite(p)}
        platform={(path) => path}
        keepInUrl
        // Each card's sessions on its row: the Sessions page folded into the board (Arugula #1082).
        cardSessions
        // A board moved to another project (Arugula #972): the page's address goes to its new one.
        onProject={(home) => (home !== p.id ? homes.set(id, home) : where.boardRead())}
        // Closed, reopened, renamed or moved on: the lists of boards and the crumb follow (Arugula #1159).
        onBoard={() => (boardsChanged(p.id), where.boardRead())}
        docTab={(doc) => <DocPage project={p.id} doc={doc} />}
        // The swarm is the shell's, on control's page (#466).
        extra={[{ label: "Workers", external: true, hint: "This board's agents in the swarm", run: () => openTab(`${controlOrigin()}/#swarm=board:${id}`), attrs: { "data-board-workers": "" } }]}
      />
    </div>
  );
}

/** The project home's list of boards (`list`, from `useBoards`): nothing
 * while the flag is off. */
export function ProjectBoards({ p, list }: { p: PlatformProject; list: ProjectBoard[] | null }) {
  const [q, setQ] = useState("");
  if (list === null) return null;
  const all = pathOf({ page: "boards", project: p.id }, names);
  return (
    <>
      <SectionHead title="Boards" all={all} onAll={(e) => follow(e, all)} data="project-boards-link" />
      {list.length === 0 && <EmptyState where="page" title="No boards yet." why="A board follows a goal across its issues and PRs." />}
      {list.length > 0 && <BoardFilter q={q} setQ={setQ} />}
      <BoardItems p={p} list={list} q={q} />
    </>
  );
}

/** One board in a list, a compact card: its title (the whole card is the link), the goal's first line (a closed board has the done note's instead), the bar and its words. A Done board's has no bar. */
function BoardItem({ p, b, waiting }: { p: PlatformProject; b: ProjectBoard; waiting?: boolean }) {
  const to = pathOf({ page: "board", project: p.id, board: b.id }, names);
  const closed = b.closed_at != null;
  const e = epicLine(b.progress);
  const line = firstLine(closed ? b.done_note : b.goal);
  return (
    <Card as="li" data-board-card={b.id}>
      <a href={to} class="card-title" data-board-link={b.id} onClick={(ev) => follow(ev, to)}>
        {b.title}
      </a>
      {waiting && (
        <p class="card-meta" data-board-not-started>
          {NOT_STARTED}
        </p>
      )}
      {line && <p class="card-meta">{line}</p>}
      {!closed && (
        <div class="card-foot" data-board-progress>
          {b.progress.total > 0 && (
            <div class="board-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.progress.total} aria-valuenow={b.progress.done} aria-label={e.words}>
              {e.segments.map((s) => (
                <i key={s.lane} data-lane={s.lane} style={{ width: `${s.pct}%` }} />
              ))}
            </div>
          )}
          <span>{e.short}</span>
        </div>
      )}
    </Card>
  );
}

/** A project's boards as cards (the project's home, every project's `/boards`): the ones people made, then the open ones that follow a Plane epic under "From Plane" (those whose epic is started first, the rest marked), and the closed ones folded under "Done (N)". */
function BoardItems({ p, list: all, q = "" }: { p: PlatformProject; list: ProjectBoard[]; q?: string }) {
  const list = all.filter((b) => boardMatches(b, q));
  const open = list.filter((b) => b.closed_at == null);
  const done = list.filter((b) => b.closed_at != null);
  const made = open.filter((b) => !b.source);
  const plane = open.filter((b) => b.source);
  const started = plane.filter((b) => b.source?.started);
  const waiting = plane.filter((b) => !b.source?.started);
  return (
    <>
      <ul class="board-list card-grid" data-project-boards>
        {made.map((b) => (
          <BoardItem key={b.id} p={p} b={b} />
        ))}
      </ul>
      {plane.length > 0 && (
        <section class="board-plane-group" aria-labelledby={`plane-boards-${p.id}`} data-boards-plane>
          <h3 id={`plane-boards-${p.id}`}>From Plane</h3>
          <ul class="board-list card-grid">
            {started.map((b) => (
              <BoardItem key={b.id} p={p} b={b} />
            ))}
            {waiting.map((b) => (
              <BoardItem key={b.id} p={p} b={b} waiting />
            ))}
          </ul>
        </section>
      )}
      {done.length > 0 && (
        <details class="board-done-list" data-boards-done open={q.trim() ? true : undefined}>
          <summary>Done ({done.length})</summary>
          <ul class="board-list card-grid">
            {done.map((b) => (
              <BoardItem key={b.id} p={p} b={b} />
            ))}
          </ul>
        </details>
      )}
    </>
  );
}
