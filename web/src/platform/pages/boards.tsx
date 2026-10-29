// `/p/:project/boards` and `/p/:project/boards/:board` (live status board,
// step 2): a project's boards, and one. A board is the platform's object
// (crates/platform/src/boards.rs): zones of rows, each pointing at an issue,
// a PR, a run, a workspace or an agent block, with a note and a status.
// The view is shared with the board block (../../shared/boards/view). The
// whole of it is behind the `boards` Labs flag: while it's off for the
// reader the routes answer 404 and the page says boards are off, and the
// project's home doesn't mention them.

import { useEffect, useState } from "preact/hooks";
import type { ProjectBoard } from "../../shared/proto";
import { BoardPanel } from "../../shared/boards/view";
import { BoardError, boards } from "../../shared/boards/store";
import { epicLine, firstLine } from "../../shared/boards/text";
import { useSubscribe } from "../../shared/ui/hooks";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { SectionHead } from "../section-head";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";
import { Missing } from "./project";

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

/** The project's boards, `null` while they load, or `"off"` when the
 * `boards` flag isn't on for the reader (the routes answer 404). Asked of
 * no project (`undefined`), it reads nothing. */
export function useBoards(project: string | undefined): ProjectBoard[] | null | "off" {
  const [list, setList] = useState<ProjectBoard[] | null | "off">(null);
  useEffect(() => {
    setList(null);
    if (!project) return;
    let live = true;
    readBoards(project).then(
      (l) => live && setList(l),
      (e: Error) => live && setList(e instanceof BoardError && e.status === 404 ? "off" : []),
    );
    return () => void (live = false);
  }, [project]);
  return list;
}

function BoardsOff() {
  return (
    <section class="platform-section" data-boards-off>
      <h1>Boards</h1>
      <p class="dim">Boards are a Labs feature, and they're off for you. A team owner turns them on in the team's Labs, on control's page.</p>
    </section>
  );
}

export function BoardsPage({ project }: { project: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <BoardList p={p} />;
}

function BoardList({ p }: { p: PlatformProject }) {
  const list = useBoards(p.id);
  const [title, setTitle] = useState("");
  if (list === "off") return <BoardsOff />;
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
    <section class="platform-section" data-boards-page>
      <h1>Boards</h1>
      {list === null && <p class="dim">Loading…</p>}
      {list?.length === 0 && <p class="dim">No boards yet.</p>}
      {list && <BoardItems p={p} list={list} data="data-boards" />}
      {mayWrite(p) && (
        <form class="board-form" onSubmit={(e) => void make(e)}>
          <input type="text" value={title} maxLength={120} placeholder="a new board's title" aria-label="New board's title" data-board-new-title onInput={(e) => setTitle(e.currentTarget.value)} />
          <span class="board-form-buttons">
            <button type="submit" class="primary" data-board-new disabled={!title.trim()}>
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
  const list = useBoards(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  if (list === "off") return <BoardsOff />;
  const all = pathOf({ page: "boards", project }, names);
  return (
    <section class="platform-section" data-board-page={board}>
      <BoardPanel
        id={board}
        write={mayWrite(p)}
        platform={(path) => path}
        back={
          <p class="dim">
            <a href={all} onClick={(e) => follow(e, all)}>
              {p.name}'s boards
            </a>
          </p>
        }
      />
    </section>
  );
}

/** The project home's list of boards (`list`, from `useBoards`): nothing
 * while the flag is off. */
export function ProjectBoards({ p, list }: { p: PlatformProject; list: ProjectBoard[] | null | "off" }) {
  if (list === null || list === "off") return null;
  const all = pathOf({ page: "boards", project: p.id }, names);
  return (
    <>
      <SectionHead title="Boards" all={all} data="project-boards-link" />
      {list.length === 0 && <p class="dim">No boards yet.</p>}
      <BoardItems p={p} list={list} data="data-project-boards" />
    </>
  );
}

/** One board in a list: its title, the goal's first line and its progress (a closed board has the done note's first line instead). */
function BoardItem({ p, b }: { p: PlatformProject; b: ProjectBoard }) {
  const to = pathOf({ page: "board", project: p.id, board: b.id }, names);
  const closed = b.closed_at != null;
  const e = epicLine(b.progress);
  const line = firstLine(closed ? b.done_note : b.goal);
  return (
    <li class="board-item-row">
      <a href={to} data-board-link={b.id} onClick={(ev) => follow(ev, to)}>
        {b.title}
      </a>
      {line && <span class="board-item-line dim">{line}</span>}
      {!closed && (
        <span class="board-item-progress dim" data-board-progress>
          {e.words}
        </span>
      )}
    </li>
  );
}

/** A project's boards, open ones first and the closed ones folded under "Done (N)". `data` is the attribute the open list carries. */
function BoardItems({ p, list, data }: { p: PlatformProject; list: ProjectBoard[]; data: "data-boards" | "data-project-boards" }) {
  const open = list.filter((b) => b.closed_at == null);
  const done = list.filter((b) => b.closed_at != null);
  return (
    <>
      <ul class="platform-list board-list" {...{ [data]: true }}>
        {open.map((b) => (
          <BoardItem key={b.id} p={p} b={b} />
        ))}
      </ul>
      {done.length > 0 && (
        <details class="board-done-list" data-boards-done>
          <summary>Done ({done.length})</summary>
          <ul class="platform-list board-list">
            {done.map((b) => (
              <BoardItem key={b.id} p={p} b={b} />
            ))}
          </ul>
        </details>
      )}
    </>
  );
}
