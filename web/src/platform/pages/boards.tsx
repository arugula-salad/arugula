// `/p/:project/boards` and `/p/:project/boards/:board` (boards,
// step 2): a project's boards, and one. `/boards` (#339): every project's,
// grouped by project, where the rail's Boards goes. A board is the platform's object
// (crates/platform/src/boards.rs): zones of rows, each pointing at an issue,
// a PR, a run, a workspace or an agent block, with a note and a status.
// The view is shared with the board block (../../shared/boards/view).
// Boards are on for everyone (they left Labs, #191).

import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { ProjectBoard } from "../../shared/proto";
import { BoardPanel } from "../../shared/boards/view";
import { boards } from "../../shared/boards/store";
import { boardMatches, epicLine, firstLine, NOT_STARTED } from "../../shared/boards/text";
import { useSubscribe } from "../../shared/ui/hooks";
import { BlockLoading, EmptyLine, EmptyState } from "../../shared/ui/block-states";
import { Card } from "../../shared/ui/card";
import { PageHeader } from "../../shared/ui/page-header";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { SectionHead } from "../../shared/ui/section-head";
import { controlOrigin, session, type PlatformProject } from "../session";
import { blindLine, blindMachines, type Grant } from "../settings/grants";
import { toast } from "../toast";
import { roleIn } from "../settings/environments";
import { mayWrite } from "./issues";
import { Missing } from "./project";
import { NotOnBoard, dropOn, useTriage, useUntriaged, type Triage } from "./triage";
import { ChevronIcon } from "../../shared/ui/icons";

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
    const at = watching.get(project) ?? new Set();
    at.add(read);
    watching.set(project, at);
    return () => {
      live = false;
      at.delete(read);
    };
  }, [project]);
  return list;
}

/** The filter box above a list of boards (#546): `/` focuses it from anywhere that isn't a field, Esc clears it. */
function BoardFilter({ q, setQ }: { q: string; setQ: (q: string) => void }) {
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const slash = (e: KeyboardEvent) => {
      if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return;
      if ((e.target as Element | null)?.closest("input, textarea, select, [contenteditable]")) return;
      e.preventDefault();
      box.current?.focus();
    };
    window.addEventListener("keydown", slash);
    return () => window.removeEventListener("keydown", slash);
  }, []);
  return (
    <input
      ref={box}
      type="search"
      class="board-filter"
      value={q}
      placeholder="Filter boards by title, epic key or goal  (/)"
      aria-label="Filter boards"
      data-boards-filter
      autoComplete="off"
      onInput={(e) => setQ(e.currentTarget.value)}
      onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), setQ(""), box.current?.blur())}
    />
  );
}

/** `/boards`: the boards of every project the reader belongs to, by
 * project. A project with none isn't listed. */
export function AllBoardsPage() {
  useSubscribe((fn) => session.subscribe(fn));
  const projects = session.projects;
  const key = projects.map((p) => p.id).join(",");
  const [lists, setLists] = useState<Map<string, ProjectBoard[]> | null>(null);
  const [q, setQ] = useState("");
  useEffect(() => {
    setLists(null);
    let live = true;
    void Promise.all(
      projects.map((p) =>
        readBoards(p.id).then(
          (l): [string, ProjectBoard[]] => [p.id, l],
          (): [string, ProjectBoard[]] => [p.id, []],
        ),
      ),
    ).then((all) => live && setLists(new Map(all)));
    return () => void (live = false);
  }, [key]);
  const shown = projects.flatMap((p) => {
    const l = lists?.get(p.id);
    return Array.isArray(l) && l.length > 0 ? [{ p, list: l }] : [];
  });
  const kept = shown.filter((s) => s.list.some((b) => boardMatches(b, q)));
  // Each shown project's issues on no board, read together.
  const counted = shown.map((s) => s.p.id);
  const open = useUntriaged(counted);
  return (
    <section class="platform-section sw-page" data-all-boards-page>
      <PageHeader title="Boards" />
      {lists === null && <BlockLoading inline what="boards" />}
      {lists && shown.length === 0 && (
        <EmptyState where="page" title="No boards yet." why="A board follows a goal across its issues and PRs. Make one on a project's Boards page." />
      )}
      {shown.length > 0 && <BoardFilter q={q} setQ={setQ} />}
      {q.trim() && shown.length > 0 && kept.length === 0 && (
        <EmptyLine data-boards-filter-none>No board matches “{q.trim()}”.</EmptyLine>
      )}
      {kept.map(({ p, list }) => {
        const all = pathOf({ page: "boards", project: p.id }, names);
        return (
          <div key={p.id} data-all-boards-project={p.id}>
            <SectionHead
              title={p.name}
              action={
                <>
                  {!!open?.get(p.id) && (
                    <a class="sw-untriaged" href={pathOf({ page: "triage", project: p.id }, names)} data-all-boards-untriaged={p.id} onClick={(e) => follow(e, pathOf({ page: "triage", project: p.id }, names))}>
                      {open.get(p.id)} not on a board
                    </a>
                  )}
                  <a class="sw-all" href={all} onClick={(e) => follow(e, all)} data-all-boards-project-link>
                    All boards <ChevronIcon dir="right" />
                  </a>
                </>
              }
            />
            <BoardItems p={p} list={list} data="data-project-boards" q={q} />
          </div>
        );
      })}
    </section>
  );
}

export function BoardsPage({ project, scroll }: { project: string; scroll?: boolean }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <BoardList p={p} scroll={scroll} />;
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

/** A quiet line under the header when some of the reader's machines have no Boards grant over the project (#313): their agents' first `board_read` would be refused. It only points at Settings → Agents, where the grant is given. */
function BlindMachines({ project, may }: { project: string; may: boolean }) {
  const line = blindLine(useBlindMachines(project, may));
  if (!line) return null;
  const to = `${pathOf({ page: "settings" }, names)}#agents`;
  return (
    <p class="dim" data-boards-blind>
      {line}{" "}
      <a href={to} data-boards-blind-link onClick={(e) => follow(e, to)}>
        Let them
      </a>
    </p>
  );
}

function BoardList({ p, scroll }: { p: PlatformProject; scroll?: boolean }) {
  const list = useBoards(p.id);
  const [title, setTitle] = useState("");
  const [q, setQ] = useState("");
  const tri = useTriage(p.id, () => boardsChanged(p.id));
  // On a phone the page is two tabs; `/triage` opens the second.
  const [tab, setTab] = useState<"boards" | "loose">(scroll ? "loose" : "boards");
  const open = (list ?? []).filter((b) => b.closed_at == null);
  const make = async (e: Event) => {
    e.preventDefault();
    if (!title.trim()) return;
    try {
      const v = await boards.create(p.id, title.trim());
      go(pathOf({ page: "board", project: p.id, board: v.board.id }, names));
    } catch (err) {
      toast(`Couldn't make the board: ${(err as Error).message}`);
    }
  };
  return (
    <section class="platform-section sw-page" data-boards-page data-tab={tab}>
      <PageHeader title="Boards" />
      <BlindMachines project={p.id} may={roleIn(p) !== "viewer"} />
      {list && (
        <div class="boards-tabs" role="tablist" aria-label="Boards page" data-boards-tabs>
          <button type="button" role="tab" id="boards-tab-boards" aria-selected={tab === "boards"} data-tab="boards" onClick={() => setTab("boards")}>
            Boards<span class="boards-tab-n">{open.length}</span>
          </button>
          <button type="button" role="tab" id="boards-tab-loose" aria-selected={tab === "loose"} data-tab="loose" onClick={() => setTab("loose")}>
            Not on a board{tri.page && <span class="boards-tab-n">{tri.page.total.toLocaleString()}</span>}
          </button>
        </div>
      )}
      {list === null && <BlockLoading inline what="boards" />}
      {list?.length === 0 && (
        <EmptyState where="page" title="No boards yet." why="A board follows a goal across its issues and PRs. Make one below." />
      )}
      {list && list.length > 0 && <BoardFilter q={q} setQ={setQ} />}
      {list && <BoardItems p={p} list={list} data="data-boards" q={q} drop={tri} between={<NotOnBoard p={p} tri={tri} open={open} scroll={scroll} />} />}
      {mayWrite(p) && (
        <form class="board-form" onSubmit={(e) => void make(e)}>
          <input type="text" value={title} maxLength={120} placeholder="a new board's title" aria-label="New board's title" data-board-new-title onInput={(e) => setTitle(e.currentTarget.value)} />
          <span class="board-form-buttons">
            <button type="submit" class="primary small" data-board-new disabled={!title.trim()}>
              Make a board
            </button>
          </span>
        </form>
      )}
    </section>
  );
}

export function BoardPage({ project, board }: { project: string; board: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return (
    <section class="platform-section" data-board-page={board}>
      <BoardPanel
        id={board}
        write={mayWrite(p)}
        platform={(path) => path}
        keepInUrl
        back={
          <p class="dim">
            {/* The swarm is the shell's, on control's page (#466). */}
            <a href={`${controlOrigin()}/#swarm=board:${board}`} data-board-workers title="This board's agents in the swarm, by the lane of their row">
              Workers
            </a>
          </p>
        }
      />
    </section>
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
      <BoardItems p={p} list={list} data="data-project-boards" q={q} />
    </>
  );
}

/** One board in a list, a compact card: its title (the whole card is the link), the goal's first line (a closed board has the done note's instead), the bar and its words. A Done board's has no bar. */
function BoardItem({ p, b, waiting, drop }: { p: PlatformProject; b: ProjectBoard; waiting?: boolean; drop?: Triage }) {
  const to = pathOf({ page: "board", project: p.id, board: b.id }, names);
  const closed = b.closed_at != null;
  const e = epicLine(b.progress);
  const line = firstLine(closed ? b.done_note : b.goal);
  return (
    <Card as="li" class={drop && !closed ? "drop-target" : undefined} data-board-card={b.id} {...(drop && !closed ? dropOn(drop, b) : {})}>
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

/** A project's boards: the ones people made, then the open ones that follow a Plane epic under "From Plane" (those whose epic is started first, the rest marked), and the closed ones folded under "Done (N)". `between` is drawn after the open ones and before the done; `drop` makes the open cards places to drop the section's rows on. `data` is the attribute the open list carries. */
function BoardItems({ p, list: all, data, q = "", between, drop }: { p: PlatformProject; list: ProjectBoard[]; data: "data-boards" | "data-project-boards"; q?: string; between?: ComponentChildren; drop?: Triage }) {
  const list = all.filter((b) => boardMatches(b, q));
  const open = list.filter((b) => b.closed_at == null);
  const done = list.filter((b) => b.closed_at != null);
  const made = open.filter((b) => !b.source);
  const plane = open.filter((b) => b.source);
  const started = plane.filter((b) => b.source?.started);
  const waiting = plane.filter((b) => !b.source?.started);
  return (
    <>
      <ul class="board-list card-grid" {...{ [data]: true }}>
        {made.map((b) => (
          <BoardItem key={b.id} p={p} b={b} drop={drop} />
        ))}
      </ul>
      {plane.length > 0 && (
        <section class="board-plane-group" aria-labelledby={`plane-boards-${p.id}`} data-boards-plane>
          <h3 id={`plane-boards-${p.id}`}>From Plane</h3>
          <ul class="board-list card-grid">
            {started.map((b) => (
              <BoardItem key={b.id} p={p} b={b} drop={drop} />
            ))}
            {waiting.map((b) => (
              <BoardItem key={b.id} p={p} b={b} waiting drop={drop} />
            ))}
          </ul>
        </section>
      )}
      {between}
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
