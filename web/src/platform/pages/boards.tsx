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
import { useSubscribe } from "../../shared/ui/hooks";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { SectionHead } from "../section-head";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";
import { Missing } from "./project";

/** The project's boards, `null` while they load, or `"off"` when the
 * `boards` flag isn't on for the reader (the routes answer 404). */
export function useBoards(project: string): ProjectBoard[] | null | "off" {
  const [list, setList] = useState<ProjectBoard[] | null | "off">(null);
  useEffect(() => {
    let live = true;
    boards.list(project).then(
      (l) => live && setList(l.boards),
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
      <ul class="platform-list board-list" data-boards>
        {list?.map((b) => {
          const to = pathOf({ page: "board", project: p.id, board: b.id }, names);
          return (
            <li key={b.id}>
              <a href={to} data-board-link={b.id} onClick={(e) => follow(e, to)}>
                {b.title}
              </a>
            </li>
          );
        })}
      </ul>
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

/** The project home's list of boards: nothing while the flag is off. */
export function ProjectBoards({ p }: { p: PlatformProject }) {
  const list = useBoards(p.id);
  if (list === null || list === "off") return null;
  const all = pathOf({ page: "boards", project: p.id }, names);
  return (
    <>
      <SectionHead title="Boards" all={all} data="project-boards-link" />
      {list.length === 0 && <p class="dim">No boards yet.</p>}
      <ul class="platform-list" data-project-boards>
        {list.map((b) => {
          const to = pathOf({ page: "board", project: p.id, board: b.id }, names);
          return (
            <li key={b.id}>
              <a href={to} onClick={(e) => follow(e, to)}>
                {b.title}
              </a>
            </li>
          );
        })}
      </ul>
    </>
  );
}
