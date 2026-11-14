// One board (boards, step 2; lanes, aru-125), wherever it's drawn: the
// platform page's `/p/:project/boards/:board`, and a board block in a daemon
// workspace (a pin onto it, `shell/blocks/board.tsx`). The four lanes in
// order (Doing, Review, Next, Done: nobody makes or moves them, the work
// does), each with its rows; a board whose author made groups (phases, waves)
// shows each as a section with its progress and the four lanes of its rows
// under it, the rows with no group last. A row's marks say what its lane
// doesn't: marked not finished (or merged and still open), waiting on another PR, earlier agents
// folded away. Each row has: a glyph for the row's lane, a link to what it
// points at with that item's state now (`live`), chips for what is attached
// to it, on a Done row the mark that says it is proven or not yet, and a
// note. Owners and editors add, edit and delete rows, and make, rename, move
// and delete groups; a viewer's writes are refused and say so. A row is a
// swarm card (platform/swarm.css, as the Issues page's): its bar is its
// lane's tone, and its title opens the drawer (./card.tsx), a read-only side
// sheet with the item's body, comments, checks, attachments and proof (#369).
//
// It reads the board as the person looking: now, again when the platform
// says it changed (the board's thread, chat's live stream), and when the
// page comes back into view. A write on an entry that moved on is refused
// (409): the view reads the board again and says so, and the change is theirs
// to make again.

import { useEffect, useId, useRef, useState } from "preact/hooks";
import type { BoardLane, BoardAsk, BoardAnsweredAsk, BoardGroup, BoardPointer, ProjectBoard, BoardRow, BoardView, BoardWorkItem, BoardZone } from "../proto";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { Card } from "../ui/card";
import { Markdown } from "../ui/markdown";
import { CheckCircleIcon, ChevronIcon, EditIcon, IconButton, LinksIcon, MoreIcon, PlusIcon } from "../ui/icons";
import { ConfirmRemove } from "../ui/confirm";
import { StateChip, Chip } from "../ui/state-chip";
import { usePhone, useSubscribe } from "../ui/hooks";
import { DocActions, openTab } from "../docs/doc-actions";
import { DocTimes } from "../docs/doc-times";
import { BlockError, BlockLoading, EmptyLine, EmptyState } from "../ui/block-states";
import { PageHeader } from "../ui/page-header";
import { SectionHead } from "../ui/section-head";
import { askText, MenuHost, menuBelow, openMenu, type MenuItem } from "../ui/menu";
import { issues } from "../issues/store";
import { agentShort } from "../agent-label";
import { day } from "../time";
import { BoardError, STALE_ORDER, STALE_REV, boards } from "./store";
import { StepsView } from "./steps-view";
import { loadDone, loadLinks, loadMode, saveDone, saveLinks, saveMode, stepPlace, type BoardMode, type Reorder } from "./steps";
import { DocLink, DocPicker } from "./docs";
import { AttachmentChip, Drawer, Labels, Owners, SessionChip, notFinishedTitle, type Act } from "./card";
import "./bar.css";
import "./boards.css";
import { groupFold, groupLaneFold, isFolded, laneFold, loadFolds, saveFolds, type Folds } from "./folds";
import { MOVE_LANES, closeName, closedWithLine, doneRefusal, laneOfZone, laneSetLine, PICK_DELAY_MS, cardTone, groupMove, PICK_LIMIT, GOAL_MAX, answerReady, askWaitingLine, debounced, earlierLine, epicLine, holdsRow, heldChip, groupLine, isCard, laneCount, marksOf, waitingOf, notFinishedLine, needsDot, needsYou, pointerHref, pointerLabel, pointerText, pointerToSend, proofMark, sections, sourceLine, sourceLinked, leftOutLine, stepActive, workLabel, findLine, type BoardFilter, rowPasses, filtering, filterChoices, filterHash, parseFilterHash } from "./text";

const STALE_SAID = "That changed while you were editing, so the board is read again. Make your change again if it still applies.";

/** What a card carries while it's dragged to another lane (#494). */
const ROW_DRAG = "text/x-arugula-board-row";
// The dragged row's group rides in the type's name, since a drop target can read only `types` while the drag is over it (types are lowercased).
const ROW_GROUP = "text/x-arugula-board-row-group-";
const dragGroupType = (group: string | null) => `${ROW_GROUP}${group ?? "none"}`.toLowerCase();
/** Whether the drag is a board row from the group `group` (null: no group). Absent for any other drag. */
const dragFrom = (e: DragEvent, group: string | null): boolean | undefined => {
  const types = e.dataTransfer?.types;
  if (!types?.includes(ROW_DRAG)) return undefined;
  return types.includes(dragGroupType(group));
};

const menuAt = (e: MouseEvent, items: MenuItem[]) => {
  const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
  openMenu({ clientX: at.left, clientY: at.bottom + 4, preventDefault: () => e.preventDefault() }, items);
};

export function BoardPanel({
  id,
  shown = true,
  platform,
  back,
  write = true,
  keepInUrl = false,
}: {
  id: string;
  /** False for a block that isn't on screen: it reads nothing then. */
  shown?: boolean;
  /** A path of the platform page (`/issues/iss_…`) as a link, if there is one. */
  platform: (path: string) => string | undefined;
  /** Over the title: the way back to a list. */
  back?: preact.ComponentChildren;
  /** Whether to offer the board's own edits (title, goal, closing). False for a viewer; a block can't tell, so it offers them and the platform's 403 comes back as a toast. */
  write?: boolean;
  /** Keep the find text and the Label and Person menus in the URL's hash (#547), so a link keeps them. The shell's block keeps them in memory. */
  keepInUrl?: boolean;
}) {
  const { toast, teams } = usePlatformApi();
  const nameOf = (account: string) => teams().flatMap((t) => t.roster.members).find((m) => m.account === account)?.name;
  const [view, setView] = useState<BoardView | null>(null);
  const [missing, setMissing] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  /** The row whose drawer is open. */
  const [drawer, setDrawer] = useState<string | null>(null);
  /** Lanes or Steps, as this viewer last picked (Steps on a phone until they do). */
  const [mode, setModeNow] = useState<BoardMode>(() => loadMode());
  const setMode = (m: BoardMode) => (setModeNow(m), saveMode(m), setBehind(null));
  /** Whether the Steps view draws done rows (#831), as this viewer last picked. */
  const [showDone, setShowDoneNow] = useState(loadDone);
  const setShowDone = (on: boolean) => (setShowDoneNow(on), saveDone(on));
  const [showLinks, setShowLinksNow] = useState(loadLinks);
  const setShowLinks = (on: boolean) => (setShowLinksNow(on), saveLinks(on));
  const phone = usePhone();
  /** The row whose reorder was refused because someone else moved its group first (Steps). */
  const [behind, setBehind] = useState<string | null>(null);
  useEffect(() => {
    if (!behind) return;
    const t = setTimeout(() => setBehind(null), 10_000);
    return () => clearTimeout(t);
  }, [behind]);
  /** The headings this viewer folded, kept in this browser per board. */
  const [folds, setFolds] = useState<Folds>(() => loadFolds(id));
  useEffect(() => setFolds(loadFolds(id)), [id]);
  const toggleFold = (key: string) => {
    const next = { ...folds, [key]: !isFolded(folds, key) };
    setFolds(next);
    saveFolds(id, next);
  };
  /** The group a Delete waits on. */
  const [dropping, setDropping] = useState<BoardGroup | null>(null);
  /** The row a move to Done waits on: its issue is closed once the person confirms (#494). */
  const [closing, setClosing] = useState<BoardRow | null>(null);
  /** The close-epic form is open and what it has been given: the trigger is in the view switch's row, the form under the progress bar. */
  const [epicClosing, setEpicClosing] = useState(false);
  const [epicNote, setEpicNote] = useState("");
  const [closeReason, setCloseReason] = useState("completed");
  /** The find box's text (#546); the bar is always there (#614), so empty is no find. */
  const [find, setFind] = useState<string>(() => (keepInUrl ? (parseFilterHash(location.hash)?.q ?? "") : ""));
  /** The Label and Person menus' values (#547): any of a menu's matches, and every menu and the text have to. */
  const [labels, setLabels] = useState<string[]>(() => (keepInUrl ? (parseFilterHash(location.hash)?.labels ?? []) : []));
  const [people, setPeople] = useState<string[]>(() => (keepInUrl ? (parseFilterHash(location.hash)?.people ?? []) : []));
  const findBox = useRef<HTMLInputElement>(null);
  // Another board starts with no filter; the first one keeps what its link said.
  const was = useRef(id);
  useEffect(() => {
    if (was.current === id) return;
    was.current = id;
    setFind("");
    setLabels([]);
    setPeople([]);
  }, [id]);
  /** Clear and Esc: no text, no menu values. */
  const clearFind = () => (setFind(""), setLabels([]), setPeople([]));
  // `/` focuses the find box, from anywhere that isn't a field.
  useEffect(() => {
    if (!shown) return;
    const slash = (e: KeyboardEvent) => {
      if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return;
      if ((e.target as Element | null)?.closest("input, textarea, select, [contenteditable]")) return;
      e.preventDefault();
      findBox.current?.focus();
    };
    window.addEventListener("keydown", slash);
    return () => window.removeEventListener("keydown", slash);
  }, [shown]);
  const filter: BoardFilter = { q: find, labels, people };
  useEffect(() => {
    if (!keepInUrl || !shown) return;
    const h = filterHash(filter);
    if (location.hash.replace(/^#/, "") !== h) history.replaceState(history.state, "", location.pathname + location.search + (h ? `#${h}` : ""));
  }, [keepInUrl, shown, find, labels, people]);
  // Chat says when the thread's object changed; it's on while chat is.
  useSubscribe((fn) => chat.subscribe(fn));
  const seen = useRef(0);
  // The project's Plane connections, once the board says its project (#359): New issue… may make the item in Plane.
  const [plane, setPlane] = useState<{ id: string; identifier: string }[]>([]);
  const project = view?.board.project;
  useEffect(() => {
    if (shown && project) void boards.plane(project).then(setPlane);
  }, [project, shown]);

  const again = () => {
    const n = ++seen.current;
    boards.get(id).then(
      (v) => n === seen.current && (setView(v), setMissing(null)),
      (e: Error) => n === seen.current && setMissing(e.message),
    );
  };
  useEffect(() => {
    if (!shown) return;
    again();
    // The thread is what the platform pushes a change on: open it so there is one.
    boards.thread(id).catch(() => {});
    const off = chat.onObject(id, again);
    const back = () => document.visibilityState === "visible" && again();
    document.addEventListener("visibilitychange", back);
    return () => {
      off();
      document.removeEventListener("visibilitychange", back);
    };
  }, [id, shown]);

  // A link from a block or a session names a row, `#row=row_…` (#372):
  // the card's drawer opens on it (decision 9), as a click on it would.
  // Read as the board opens: keeping the filter in the URL replaces the hash.
  const [named] = useState(() => location.hash.match(/^#row=(row_[0-9a-f]+)/)?.[1]);
  const rowKey = view ? named : undefined;
  useEffect(() => {
    if (!rowKey) return;
    document.querySelector(`[data-board-row="${rowKey}"]`)?.scrollIntoView({ block: "center" });
    if (view?.zones.some((z) => z.rows.some((r) => r.id === rowKey))) setDrawer(rowKey);
  }, [rowKey, !!view]);

  if (missing && !view) return <BlockError inline data-board-missing title="Can't read this board." detail={missing} retry={again} />;
  if (!view) return <BlockLoading inline what="the board" />;

  /** Run a write; on 409 read again and say so. */
  const act = async (write: () => Promise<BoardView>): Promise<boolean> => {
    try {
      setView(await write());
      setStale(false);
      return true;
    } catch (e) {
      if (e instanceof BoardError && e.reason === STALE_REV) {
        setStale(true);
        setEditing(null);
        again();
      } else toast(`Couldn't change it: ${(e as Error).message}`);
      return false;
    }
  };

  /** A native issue in the board's project, and a row pointing at it. */
  const newIssue = async () => {
    const title = await askText("New issue", "", "its title");
    if (!title?.trim()) return;
    let made;
    try {
      made = await issues.create(view.board.project, { title: title.trim(), body: "", labels: [], assignees: [] });
    } catch (e) {
      toast(`Couldn't make the issue: ${(e as Error).message}`);
      return;
    }
    const ok = await act(() => boards.put(id, { entry: "row", pointer: { kind: "issue", id: made.id } }));
    if (!ok) toast(`Made #${made.number} but couldn't add it to the board`);
  };

  /** A work item in a Plane connection of the board's project, and a row pointing at it in the zone. */
  const newPlaneIssue = async (c: { id: string; identifier: string }) => {
    const title = await askText(`New issue in Plane (${c.identifier})`, "", "its title");
    if (!title?.trim()) return;
    let made;
    try {
      made = await boards.planeCreate(view.board.project, c.id, title.trim());
    } catch (e) {
      toast(`Couldn't make the issue in Plane: ${(e as Error).message}`);
      return;
    }
    const ok = await act(() => boards.put(id, { entry: "row", pointer: { kind: "forge_item", id: made.item ?? "" } }));
    if (!ok) toast(`Made ${made.reference ?? "the item"} in Plane but couldn't add it to the board`);
  };

  const rename = async () => {
    const t = await askText("Rename the board", view.board.title);
    if (t?.trim() && t.trim() !== view.board.title) await act(() => boards.edit(id, { title: t.trim() }));
  };

  // The team link: the board's page on the platform, as the page or block names it.
  const path = platform(`/p/${view.board.project}/boards/${id}`);
  const shareLink = path ? new URL(path, location.href).href : null;

  const addGroup = async () => {
    const t = await askText("Add a group", "", "a phase, a wave or an area");
    if (t?.trim()) await act(() => boards.put(id, { entry: "zone", title: t.trim() }));
  };
  const renameGroup = async (g: BoardGroup) => {
    const t = await askText("Rename the group", g.title);
    if (t?.trim() && t.trim() !== g.title) await act(() => boards.put(id, { id: g.id, rev: g.rev, entry: "zone", title: t.trim() }));
  };
  const moveGroup = (g: BoardGroup, down: boolean) => {
    const to = groupMove(view.groups ?? [], g.id, down);
    if (to) void act(() => boards.move(id, g.id, g.rev, to));
  };
  const moveRow = (r: BoardRow, zone: string | null) => void act(() => boards.move(id, r.id, r.rev, { zone }));
  /** A reorder inside a step (Steps): it names the group's rows as this view read them. If someone moved them first, nothing moves; the board is read again and the row says so. It isn't made again by itself. */
  const reorderRow = async (r: BoardRow, m: Reorder) => {
    setBehind(null);
    try {
      setView(await boards.reorder(id, r.id, m.before, m.seen));
      setStale(false);
    } catch (e) {
      if (e instanceof BoardError && e.reason === STALE_ORDER) {
        setBehind(r.id);
        again();
      } else toast(`Couldn't move it: ${(e as Error).message}`);
    }
  };
  /** A row dropped on a group (#664): it moves there with its lane unchanged. */
  const dropRow = (row: string, group: string | null) => {
    const r = view.zones.flatMap((z) => z.rows).find((x) => x.id === row);
    if (r && (r.group ?? null) !== group) moveRow(r, group);
  };
  /** Move a row to a lane by hand (#494). Done closes its native issue after a confirm; any other item is closed where it lives, and the row says so. */
  const moveLane = (row: string, lane: BoardLane) => {
    const r = view.zones.flatMap((z) => z.rows).find((x) => x.id === row);
    if (!r || r.lane === lane) return;
    if (lane !== "done") return void act(() => boards.setLane(id, r.id, lane));
    const why = doneRefusal(r);
    if (why) return void toast(why);
    setCloseReason("completed");
    setClosing(r);
  };
  const groupMenu = (e: MouseEvent, g: BoardGroup) => {
    const all = view.groups ?? [];
    const at = all.findIndex((x) => x.id === g.id);
    menuAt(e, [
      { label: "Rename…", run: () => void renameGroup(g) },
      { label: "Move up", disabled: at <= 0, run: () => moveGroup(g, false) },
      { label: "Move down", disabled: at >= all.length - 1, run: () => moveGroup(g, true) },
      "separator",
      { label: "Delete the group…", danger: true, run: () => setDropping(g) },
    ]);
  };
  // A find narrows the view to the rows it matches; the saved folds stay as they are.
  const query = find.trim();
  const narrows = filtering(filter);
  const narrowed: BoardView = narrows ? { ...view, zones: view.zones.map((z) => ({ ...z, rows: z.rows.filter((r) => rowPasses(r, filter)) })) } : view;
  const every = view.zones.reduce((n, z) => n + z.rows.length, 0);
  const allRows = view.zones.flatMap((z) => z.rows);
  const found = narrowed.zones.flatMap((z) => z.rows);
  const firstMatch = sections(narrowed).flatMap((s) => s.lanes.flatMap((z) => z.rows))[0];
  const opened = drawer ? view.zones.flatMap((z) => z.rows).find((r) => r.id === drawer) : undefined;

  return (
    <div class="board" data-board={id}>
      <BoardHead
        view={view}
        back={back}
        write={write}
        shareLink={shareLink}
        rename={() => void rename()}
        close={write && view.board.closed_at == null && !epicClosing ? () => (setEpicNote(""), setEpicClosing(true)) : undefined}
        act={act}
        id={id}
        platform={platform}
      />
      <div class="board-switch">
        <div class="seg small board-views" role="group" aria-label="View" data-board-views>
          {(["lanes", "steps"] as const).map((m) => (
            <button key={m} type="button" aria-pressed={mode === m} data-board-view={m} onClick={() => setMode(m)}>
              {m === "lanes" ? "Lanes" : "Steps"}
            </button>
          ))}
        </div>
      </div>
      <div class="board-find" role="search" data-board-find>
        <input
          ref={findBox}
          type="text"
          inputMode="search"
          value={find}
          placeholder="Find a row: #541, ARUGU-12 or words"
          aria-label="Find a row"
          data-board-find-input
          autoComplete="off"
          onInput={(e) => setFind(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") (e.preventDefault(), clearFind(), e.currentTarget.blur());
            else if (e.key === "Enter" && firstMatch) (e.preventDefault(), setDrawer(firstMatch.id));
          }}
        />
        <FilterMenu name="label" title="Label" choices={filterChoices(allRows, "labels")} picked={labels} set={setLabels} />
        <FilterMenu name="person" title="Person" choices={filterChoices(allRows, "assignees")} picked={people} set={setPeople} />
        {narrows && (
          <span class="board-find-count dim" role="status" data-board-find-count>
            {findLine(found.length, every)}
          </span>
        )}
        {narrows && (
          <button type="button" class="board-find-clear" data-board-find-clear onClick={clearFind}>
            Clear
          </button>
        )}
      </div>
      <Epic b={view.board} closing={epicClosing} setClosing={setEpicClosing} note={epicNote} setNote={setEpicNote} act={act} id={id} write={write} view={view} />
      {stale && (
        <p class="board-stale" role="status" data-board-stale>
          {STALE_SAID}
        </p>
      )}
      {narrows && found.length === 0 && (
        <EmptyLine data-board-find-none>{query ? `No row matches “${query}”.` : "No row matches the filter."}</EmptyLine>
      )}
      {!narrows && allRows.length === 0 && editing !== "new" && (
        <EmptyState
          where="page"
          data-board-empty
          title="No rows yet."
          why={`A row is one piece of work: an issue or a PR. Its lane follows the work, so you never move it.${write ? "" : " Its writers add rows."}`}
          action={
            write && (
              <button type="button" class="secondary small" data-board-add-row onClick={() => setEditing("new")}>
                <PlusIcon size="xs" />
                Add row
              </button>
            )
          }
        />
      )}
      {mode === "steps" && allRows.length > 0 && !(narrows && found.length === 0) && (
        <>
          <div class="steps-toggles" data-steps-toggles>
            <button type="button" class="secondary small steps-toggle" aria-pressed={showDone} data-board-done title={showDone ? "Hide done rows" : "Show done rows"} aria-label={phone ? (showDone ? "Hide done rows" : "Show done rows") : undefined} onClick={() => setShowDone(!showDone)}>
              <CheckCircleIcon />
              {!phone && "Done"}
            </button>
            <button type="button" class="secondary small steps-toggle" aria-pressed={showLinks} data-board-links title={showLinks ? "Hide links" : "Show links"} aria-label={phone ? (showLinks ? "Hide links" : "Show links") : undefined} onClick={() => setShowLinks(!showLinks)}>
              <LinksIcon />
              {!phone && "Links"}
            </button>
          </div>
          <StepsView view={narrowed} write={write} open={setDrawer} opened={drawer} move={(r, m) => void reorderRow(r, m)} behind={behind} showDone={showDone} showLinks={showLinks} />
        </>
      )}
      {mode === "lanes" && sections(narrowed).map((s) => {
        const grouped = !!view.groups?.length;
        const key = groupFold(s.group?.id ?? null);
        const held = s.lanes.reduce((n, z) => n + z.rows.length, 0);
        // A find hides what doesn't match, and opens a heading that holds a match without saving it.
        if (narrows && held === 0) return null;
        const folded = grouped && !narrows && isFolded(folds, key);
        const lanes = (
          <div class="board-lanes">
            {s.lanes.map((z) => {
              if (narrows && z.rows.length === 0) return null;
              const laneKey = grouped ? groupLaneFold(s.group?.id ?? null, z.id) : laneFold(z.id);
              return (
              <Lane key={z.id} z={z} fold={{ key: laneKey, folded: !narrows && isFolded(folds, laneKey), toggle: () => toggleFold(laneKey) }} project={view.board.project} editing={editing} setEditing={setEditing} act={act} id={id} platform={platform} groups={write ? (view.groups ?? []) : []} group={s.group?.id ?? null} moveRow={moveRow} moveLane={write ? moveLane : undefined} open={setDrawer} opened={drawer} />
              );
            })}
          </div>
        );
        // No groups: the lanes alone, as before.
        if (!grouped) return lanes;
        const title = s.group?.title ?? "No group";
        const rows = held;
        return (
          <GroupDrop key={s.group?.id ?? "none"} group={s.group?.id ?? null} take={write ? dropRow : undefined}>
            <SectionHead
              rule
              titleData="board-group-title"
              title={
                <button type="button" class="board-fold" aria-expanded={!folded} data-board-fold={key} onClick={() => toggleFold(key)}>
                  <ChevronIcon open={!folded} />
                  {title}
                  {folded && ` · ${rows}`}
                </button>
              }
              count={folded ? undefined : rows}
              action={
                <>
                  {s.group && (
                    <span class="board-group-progress dim" data-board-group-progress>
                      {groupLine(s.group.progress)}
                    </span>
                  )}
                  {s.group && write && (
                    <IconButton label={`Group actions: ${s.group.title}`} class="bar-btn sq board-more" data-board-group-menu={s.group.id} onClick={(e) => groupMenu(e, s.group!)}>
                      <MoreIcon size="sm" />
                    </IconButton>
                  )}
                </>
              }
            />
            {!folded && lanes}
          </GroupDrop>
        );
      })}
      {view.said && (
        <p class="board-said" role="status" data-board-said>
          {view.said}
        </p>
      )}
      {editing === "new" ? (
        <RowForm project={view.board.project} row={null} save={(f) => act(() => boards.put(id, { entry: "row", note: f.note, pointer: f.pointer ?? null }))} done={() => setEditing(null)} />
      ) : (
        <p class="board-foot">
          {allRows.length > 0 && (
            <button type="button" class="secondary small" data-board-add-row onClick={() => setEditing("new")}>
              <PlusIcon size="xs" />
              Add row
            </button>
          )}
          {write && (
            <button type="button" class="secondary small" data-board-add-group onClick={() => void addGroup()}>
              <PlusIcon size="xs" />
              Add a group
            </button>
          )}
          <button type="button" class="secondary small" data-board-new-issue onClick={() => void newIssue()}>
            <PlusIcon size="xs" />
            New issue
          </button>
          {plane.map((c) => (
            <button key={c.id} type="button" class="secondary small" data-board-new-plane-issue={c.identifier} onClick={() => void newPlaneIssue(c)}>
              <PlusIcon size="xs" />
              New issue in Plane ({c.identifier})
            </button>
          ))}
        </p>
      )}
      {dropping && (
        <ConfirmRemove
          title={`Delete the group ${dropping.title}?`}
          label="Delete"
          go={() => (setDropping(null), void act(() => boards.remove(id, dropping.id)))}
          cancel={() => setDropping(null)}
        >
          <p>Its rows go with it. Their issues and PRs stay where they are.</p>
        </ConfirmRemove>
      )}
      {closing && (
        <ConfirmRemove
          title={`Close ${closeName(closing)}?`}
          label="Close"
          go={() => (setClosing(null), void act(() => boards.setLane(id, closing.id, "done", closeReason)))}
          cancel={() => setClosing(null)}
        >
          <p>Moving a row to Done closes its issue. Moving it out of Done opens it again.</p>
          <label class="dim">
            Reason{" "}
            <select data-board-close-reason value={closeReason} onChange={(e) => setCloseReason(e.currentTarget.value)}>
              <option value="completed">Completed</option>
              <option value="not_planned">Not planned</option>
              <option value="duplicate">Duplicate</option>
            </select>
          </label>
        </ConfirmRemove>
      )}
      {opened && <Drawer steps={mode === "steps" && write ? { place: stepPlace(view, opened.id), move: (m) => void reorderRow(opened, m) } : undefined} r={opened} platform={platform} close={() => setDrawer(null)} project={view.board.project} others={view.zones.flatMap((z) => z.rows)} write={write} act={act} id={id} moveLane={moveLane} nameOf={nameOf} />}
    </div>
  );
}

/** The board's header, as a Doc's (#929): the title and, at its right, Copy link, Open in browser and ⋯; when it was made and last edited; the goal with the Docs beside it; and the progress as its bottom rule. The panel doesn't know whether the person may write (the pin block has no project), so without `write` the controls still show and the platform's refusal comes back as `act`'s toast. */
function BoardHead({ view, back, write, shareLink, rename, close, act, id, platform }: { view: BoardView; back?: preact.ComponentChildren; write: boolean; shareLink: string | null; rename: () => void; close?: () => void; act: Act; id: string; platform: (path: string) => string | undefined }) {
  const b = view.board;
  const { toast } = usePlatformApi();
  const [goalOpen, setGoalOpen] = useState(false);
  const [goal, setGoal] = useState("");
  const e = epicLine(b.progress);
  const editGoal = () => (setGoal(b.goal), setGoalOpen(true));
  const saveGoal = async () => {
    if (await act(() => boards.edit(id, { goal: goal.trim() }))) setGoalOpen(false);
  };
  const copy = async (): Promise<"ok" | false> => {
    try {
      await navigator.clipboard.writeText(shareLink ?? "");
      return "ok";
    } catch {
      toast("Couldn't copy the link");
      return false;
    }
  };
  const items: MenuItem[] = [
    ...(write
      ? [
          { label: "Rename…", run: rename, attrs: { "data-board-rename": "" } },
          { label: "Edit goal…", run: editGoal, attrs: { "data-board-goal-menu": "" } },
        ]
      : []),
    ...(close ? [{ label: "Close epic…", run: close, attrs: { "data-board-close": "" } }] : []),
  ];
  // The goal's button takes the focus back when its form shuts.
  const goalButton = useRef<HTMLParagraphElement>(null);
  const wasOpen = useRef(false);
  useEffect(() => {
    if (wasOpen.current && !goalOpen) goalButton.current?.focus({ preventScroll: true });
    wasOpen.current = goalOpen;
  }, [goalOpen]);
  const press = (ev: KeyboardEvent) => (ev.key === "Enter" || ev.key === " ") && (ev.preventDefault(), editGoal());
  const pencil = (
    <span class="board-goal-pencil" aria-hidden="true">
      <EditIcon size="xs" />
    </span>
  );
  return (
    <PageHeader
      named
      class="board-head"
      back={back}
      title={<span data-board-heading>{b.title}</span>}
      link={
        <DocActions
          none={!shareLink}
          copy={copy}
          open={() => shareLink && openTab(shareLink)}
          hooks={{ copy: { "data-doc-copy-link": "", "data-copy-link": "", "data-link": shareLink ?? undefined } }}
        >
          {items.length > 0 && (
            <IconButton label="More" class="bar-btn sq" aria-haspopup="menu" data-board-more data-page-more onClick={(ev) => openMenu(ev, items, { from: ev.currentTarget, end: true })}>
              <MoreIcon size="sm" />
            </IconButton>
          )}
        </DocActions>
      }
    >
      <p class="doc-meta board-meta" data-board-meta>
        <DocTimes doc={{ made: b.created_at, updated: b.updated_at }} />
      </p>
      {(write || b.goal || (view.docs ?? []).length > 0) && (
        <div class="board-head-body">
          <div class="board-goal-col">
            {goalOpen ? (
              <div class="board-form" data-board-goal-form>
                <textarea
                  value={goal}
                  rows={4}
                  maxLength={GOAL_MAX}
                  aria-label="Goal"
                  data-board-goal-input
                  autoFocus
                  onInput={(ev) => setGoal(ev.currentTarget.value)}
                  onKeyDown={(ev) => {
                    if (ev.key === "Escape") (ev.preventDefault(), ev.stopPropagation(), setGoalOpen(false));
                    else if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) (ev.preventDefault(), void saveGoal());
                  }}
                />
                <span class="board-form-buttons">
                  <button type="button" class="primary small" data-board-goal-save onClick={() => void saveGoal()}>
                    Save
                  </button>
                  <button type="button" class="secondary small" onClick={() => setGoalOpen(false)}>
                    Cancel
                  </button>
                </span>
              </div>
            ) : write ? (
              <p ref={goalButton} class={b.goal ? "board-goal editable" : "board-goal editable empty"} role="button" tabIndex={0} title="Edit goal" data-board-goal={b.goal ? "" : undefined} data-board-goal-edit onClick={editGoal} onKeyDown={press}>
                {b.goal || "Add a goal…"}
                {pencil}
              </p>
            ) : (
              b.goal && (
                <p class="board-goal" data-board-goal>
                  {b.goal}
                </p>
              )
          )}
        </div>
        <DocsStrip view={view} act={act} id={id} write={write} platform={platform} />
      </div>
      )}
      {b.progress.total > 0 ? (
        <div class="board-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.progress.total} aria-valuenow={b.progress.done} aria-label={e.words} data-board-progress>
          {e.segments.map((s) => (
            <i key={s.lane} data-lane={s.lane} style={{ width: `${s.pct}%` }} />
          ))}
        </div>
      ) : (
        <div class="board-bar" aria-hidden="true" data-board-progress />
      )}
    </PageHeader>
  );
}

/** A board's source, who needs you, and closing, under its header. */
function Epic({ b, closing, setClosing, note, setNote, act, id, write, view }: { b: ProjectBoard; closing: boolean; setClosing: (on: boolean) => void; note: string; setNote: (note: string) => void; act: Act; id: string; write: boolean; view: BoardView }) {
  const waiting = needsYou(view);
  const close = async () => {
    if (await act(() => boards.close(id, note.trim()))) setClosing(false);
  };
  return (
    <div class="board-epic">
      {b.source && (
        <p class="dim board-source" data-board-source={b.source.key}>
          {sourceLinked(b.source) ? (
            <a href={b.source.url} target="_blank" rel="noreferrer">
              {sourceLine(b.source)}
            </a>
          ) : (
            sourceLine(b.source)
          )}
          {b.source.left_out > 0 && <span data-board-left-out> · {leftOutLine(b.source.left_out)}</span>}
        </p>
      )}
      {waiting.length > 0 && (
        <div class="sw-stats board-stats" data-board-stats>
          <button type="button" class="needs board-needs-you-button" data-board-needs-you={waiting.length} onClick={() => document.querySelector(`[data-board-row="${waiting[0].id}"]`)?.scrollIntoView({ behavior: "smooth", block: "center" })}>
            <b>{waiting.length}</b> Needs you
          </button>
        </div>
      )}
      {b.closed_at != null ? (
        <div class="board-done" data-board-done>
          <p>
            <Chip kind="tag" tone="ok">Done</Chip> <span class="dim">{day(b.closed_at)}</span>
          </p>
          <p class="board-goal" data-board-done-note>
            {b.done_note}
          </p>
          {write && (
            <p class="board-epic-actions">
              <button type="button" data-board-reopen onClick={() => void act(() => boards.reopen(id))}>
                Reopen
              </button>
            </p>
          )}
        </div>
      ) : closing ? (
        <div class="board-form" data-board-close-form>
          <label>
            What can a person now do?
            <textarea value={note} rows={3} aria-label="What can a person now do?" data-board-close-note autoFocus onInput={(ev) => setNote(ev.currentTarget.value)} />
          </label>
          <span class="board-form-buttons">
            <button type="button" class="primary small" data-board-close-save disabled={!note.trim()} onClick={() => void close()}>
              Close epic
            </button>
            <button type="button" class="secondary small" onClick={() => setClosing(false)}>
              Cancel
            </button>
          </span>
        </div>
      ) : null}
    </div>
  );
}

/** The board's Docs, in a column beside its goal (under it on a narrow board): each Doc's title on a line, and Link a Doc for a writer. A reader who can't write sees the titles; with no Docs and no right to link one, nothing. */
function DocsStrip({ view, act, id, write, platform }: { view: BoardView; act: Act; id: string; write: boolean; platform: (path: string) => string | undefined }) {
  const [picking, setPicking] = useState(false);
  const docs = view.docs ?? [];
  if (!docs.length && !write) return null;
  const taken = new Set(docs.flatMap((d) => (d.pointer.kind === "doc" ? [d.pointer.id] : [])));
  return (
    <nav class="board-docs" aria-label="Docs" data-board-docs>
      <h2 class="board-docs-head">Docs</h2>
      {docs.map((d) => (
        <DocLink key={d.id} list d={d} project={view.board.project} platform={platform} remove={write ? () => void act(() => boards.removeDoc(id, d.id)) : undefined} />
      ))}
      {write && !picking && (
        <button type="button" class="board-doc-link" data-board-doc-add onClick={() => setPicking(true)}>
          <PlusIcon size="xs" />
          Link a Doc
        </button>
      )}
      {picking && <DocPicker project={view.board.project} taken={taken} pick={(p) => act(() => boards.addDoc(id, p))} close={() => setPicking(false)} />}
    </nav>
  );
}

/** A row's open question: the question, its why folded, the options as buttons with the recommendation marked, and a text box when there are none. Only a person who may answer gets the buttons. The board's own flags (`raised`) have no card: the chip in trouble carries them (#903). */
function AskCard({ a, answer }: { a: BoardAsk; answer: (a: { option: string } | { text: string }) => Promise<boolean> }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const send = async (a: { option: string } | { text: string }) => {
    setBusy(true);
    await answer(a);
    setBusy(false);
  };
  return (
    <div class={`board-ask${a.mine ? " mine" : ""}`} data-board-ask={a.id}>
      <div class="board-line board-ask-question">
        <StateChip tone="accent">Asks</StateChip>
        <span data-board-ask-question>{a.question}</span>
      </div>
      {a.why && (
        <details class="board-ask-why">
          <summary class="dim">Why</summary>
          <p>{a.why}</p>
        </details>
      )}
      {a.can_answer && a.options.length > 0 && (
        <div class="board-line board-ask-options">
          {a.options.map((o) => (
            <button key={o} type="button" class="secondary small" disabled={busy} data-board-ask-option={o} title={o === a.recommend ? "Recommended" : undefined} onClick={() => void send({ option: o })}>
              {o}
              {o === a.recommend && <span class="dim"> · recommended</span>}
            </button>
          ))}
        </div>
      )}
      {a.can_answer && a.options.length === 0 && (
        <form
          class="board-line board-ask-text"
          onSubmit={(e) => {
            e.preventDefault();
            if (answerReady(text)) void send({ text: text.trim() }).then(() => setText(""));
          }}
        >
          <input type="text" value={text} maxLength={2000} aria-label="Your answer" placeholder="your answer" data-board-ask-text onInput={(e) => setText(e.currentTarget.value)} />
          <button type="submit" class="primary" disabled={busy || !answerReady(text)} data-board-ask-send>
            Send
          </button>
        </form>
      )}
      {!a.can_answer && (
        <div class="board-line dim" data-board-ask-waiting>
          {askWaitingLine(a)}
        </div>
      )}
    </div>
  );
}

/** The last answers on a row, folded. */
function Answered({ asked }: { asked: BoardAnsweredAsk[] }) {
  return (
    <details class="board-asked" data-board-asked={asked.length}>
      <summary class="dim">Answered ({asked.length})</summary>
      {asked.map((q) => (
        <p key={q.id} class="board-asked-one">
          {q.question} <b>{q.answer}</b> <span class="dim">· {q.answered_by}, {day(q.answered_at)}</span>
        </p>
      ))}
    </details>
  );
}

/** A group's section, which takes a row dragged from another group (#664). */
function GroupDrop({ group, take, children }: { group: string | null; take?: (row: string, group: string | null) => void; children: preact.ComponentChildren }) {
  const [over, setOver] = useState(false);
  const takes = (e: DragEvent) => !!take && dragFrom(e, group) === false;
  return (
    <section
      class={`board-group drop-target${over ? " drop-over" : ""}`}
      data-board-group={group ?? "none"}
      data-drop-over={over ? "" : undefined}
      onDragOver={(e) => {
        if (!takes(e)) return;
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
        setOver(true);
      }}
      onDragLeave={(e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && setOver(false)}
      onDrop={(e) => {
        if (!takes(e)) return;
        e.preventDefault();
        setOver(false);
        const row = e.dataTransfer?.getData(ROW_DRAG);
        if (row) take!(row, group);
      }}
    >
      {children}
    </section>
  );
}

function Lane({
  z,
  fold,
  project,
  editing,
  setEditing,
  act,
  id,
  platform,
  groups,
  group,
  moveRow,
  moveLane,
  open,
  opened,
}: {
  z: BoardZone;
  /** The lane heading as a button; inside a group its key carries the group. */
  fold: { key: string; folded: boolean; toggle: () => void };
  project: string;
  editing: string | null;
  setEditing: (e: string | null) => void;
  act: Act;
  id: string;
  platform: (path: string) => string | undefined;
  /** The groups a row may move to; none for a reader who can't write. */
  groups: BoardGroup[];
  /** The group this lane sits in (null: no group). */
  group: string | null;
  moveRow: (r: BoardRow, zone: string | null) => void;
  /** Move a row to a lane by hand (#494); none for a reader who can't write. */
  moveLane?: (row: string, lane: BoardLane) => void;
  open: (row: string) => void;
  /** The row whose drawer is open: its card is the selected one. */
  opened: string | null;
}) {
  const n = laneCount(z);
  // A card dragged over the lane (#494): the lane takes it, unless it comes from another group, which takes it whole (#664).
  const [over, setOver] = useState(false);
  const lane = laneOfZone(z.id);
  const takes = (e: DragEvent) => !!moveLane && !!lane && dragFrom(e, group) === true;
  const drop = (e: DragEvent) => {
    if (!takes(e)) return;
    e.preventDefault();
    setOver(false);
    const row = e.dataTransfer?.getData(ROW_DRAG);
    if (row && lane) moveLane!(row, lane);
  };
  return (
    <section
      class={`board-zone drop-target${over ? " drop-over" : ""}`}
      data-board-zone={z.id}
      data-zone-kind={z.kind ?? undefined}
      data-drop-over={over ? "" : undefined}
      onDragOver={(e) => {
        if (!takes(e)) return;
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
        setOver(true);
      }}
      onDragLeave={(e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && setOver(false)}
      onDrop={drop}
    >
      <header class="board-zone-head">
        <h2>
          <button type="button" class="board-fold" aria-expanded={!fold.folded} data-board-fold={fold.key} onClick={fold.toggle}>
            <ChevronIcon open={!fold.folded} />
            {z.title}
          </button>
        </h2>
        <span class="board-count dim" data-board-count>
          {n.total}
          {n.proven > 0 && ` · ${n.proven} proven`}
        </span>
      </header>
      {!fold.folded && (
        <ul class="board-rows card-list">
          {z.rows.map((r) => (
            <Row key={r.id} r={r} editing={editing === r.id} setEditing={setEditing} act={act} id={id} project={project} platform={platform} groups={groups} moveRow={moveRow} moveLane={moveLane} open={open} opened={opened} />
          ))}
        </ul>
      )}
    </section>
  );
}

/** A filter menu of the find bar (#547): its values with how many rows have each, several at once. */
function FilterMenu({ name, title, choices, picked, set }: { name: string; title: string; choices: { value: string; count: number }[]; picked: string[]; set: (v: string[]) => void }) {
  const button = useRef<HTMLButtonElement>(null);
  // Drawn in the facet itself, so it is part of the board it filters.
  const [open, setOpen] = useState(false);
  // The open menu's items were made when it opened and it stays open while you
  // tick, so a tick works on what is picked now.
  const now = useRef({ picked, set });
  now.current = { picked, set };
  // A value picked from a link that no row has now still shows, so it can be cleared.
  const have = new Set(choices.map((c) => c.value.toLowerCase()));
  const shown = [...choices, ...picked.filter((p) => !have.has(p.toLowerCase())).map((value) => ({ value, count: 0 }))];
  if (!shown.length) return null;
  const on = (v: string) => picked.some((p) => p.toLowerCase() === v.toLowerCase());
  const toggle = (v: string) => {
    const { picked: was, set: setTo } = now.current;
    setTo(was.some((p) => p.toLowerCase() === v.toLowerCase()) ? was.filter((p) => p.toLowerCase() !== v.toLowerCase()) : [...was, v]);
  };
  const items: MenuItem[] = shown.map((c) => ({
    label: c.value,
    detail: String(c.count),
    checked: on(c.value),
    multi: true,
    run: () => toggle(c.value),
    attrs: { "data-board-facet-value": c.value },
  }));
  return (
    <span class="board-facet" data-board-facet={name}>
      <button
        ref={button}
        type="button"
        class="secondary small"
        aria-haspopup="menu"
        aria-expanded={open}
        data-board-facet-button={name}
        onClick={() => setOpen(true)}
      >
        {title}
        {picked.length > 0 && <span class="board-facet-n"> · {picked.length}</span>}
      </button>
      {open && (
        <MenuHost
          show={() => !!button.current && openMenu(menuBelow(button.current), items, { from: button.current, label: title, attrs: { "data-board-facet-menu": name }, onClose: () => setOpen(false) })}
          close={() => setOpen(false)}
        />
      )}
    </span>
  );
}

function Row({
  r,
  project,
  editing,
  setEditing,
  act,
  id,
  platform,
  groups,
  moveRow,
  moveLane,
  open,
  opened,
}: {
  r: BoardRow;
  editing: boolean;
  setEditing: (e: string | null) => void;
  act: Act;
  id: string;
  project: string;
  platform: (path: string) => string | undefined;
  groups: BoardGroup[];
  moveRow: (r: BoardRow, zone: string | null) => void;
  moveLane?: (row: string, lane: BoardLane) => void;
  open: (row: string) => void;
  opened: string | null;
}) {
  const phone = usePhone();
  const { teams } = usePlatformApi();
  const setBy = laneSetLine(r, (a) => teams().flatMap((t) => t.roster.members).find((m) => m.account === a)?.name);
  const label = pointerLabel(r.pointer, r.live);
  const href = pointerHref(r, platform);
  const card = isCard(r.live);
  const mark = proofMark(r);
  const marks = marksOf(r);
  // A stalled row says so on its agent's chip; with no such chip the mark stays.
  const waiting = waitingOf(r);
  const stalledMark = r.attachments.some(holdsRow) || waiting ? null : marks.stalled;
  const stalledWords = marks.stalled?.words;
  // A row that waits on a person says who on its agent's chip, else as a mark of its own (#764).
  const waitingMark = r.attachments.some(holdsRow) ? null : waiting;
  const earlier = r.attachments.filter((a) => marks.earlier.has(a.id));
  const current = r.attachments.filter((a) => !marks.earlier.has(a.id));
  const held = heldChip(r);
  const tone = cardTone(r);
  // Proof attached to the row is its "Proof" link (`AttachmentChip`); the tag stands for proof that has none (a merged PR, CI), and for none.
  const proofLink = mark && (!mark.proven || !r.attachments.some((a) => a.role === "proof")) && (
    <Chip kind="tag" tone={mark.proven ? "ok" : "muted"} title={mark.title} data-board-proof={mark.proven ? (r.proof?.by ?? "proven") : "not-proven"}>
      {mark.words}
    </Chip>
  );
  const more = (e: MouseEvent) =>
    menuAt(e, [
      { label: "Details", run: () => open(r.id) },
      { label: "Edit…", run: () => setEditing(r.id) },
      ...(groups.length
        ? [
            {
              label: "Move to group…",
              submenu: [
                ...groups.map((g) => ({ label: g.title, checked: r.group === g.id, run: () => r.group !== g.id && moveRow(r, g.id) })),
                ...(r.group ? [{ label: "No group", run: () => moveRow(r, null) }] : []),
              ],
            },
          ]
        : []),
      ...(moveLane
        ? [{ label: "Move to", submenu: MOVE_LANES.filter((l) => l.lane !== r.lane).map((l) => ({ label: l.title, run: () => moveLane(r.id, l.lane) })) }]
        : []),
      "separator",
      { label: "Delete the row", danger: true, run: () => void act(() => boards.remove(id, r.id)) },
    ]);
  // The card's empty space opens the drawer (Enter on the focused card too); its links, chips, buttons and forms do their own.
  const body = (e: MouseEvent) => {
    if (e.button !== 0 || (e.target as Element).closest("a, button, summary, input, textarea, select, form, details, .board-ask")) return;
    if (window.getSelection()?.toString()) return;
    open(r.id);
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === "Enter" && e.target === e.currentTarget) (e.preventDefault(), open(r.id));
  };
  const title = card ? r.live!.title : label || "Note";
  const external = r.pointer.kind === "forge_item" || r.pointer.kind === "link";
  return (
    <Card as="li" tone={tone} class="board-row" data-board-row={r.id} data-lane={r.lane} data-status={r.status} data-tone={tone} data-needs-you={needsDot(r) ? "" : undefined} aria-current={opened === r.id ? "true" : undefined} data-lane-set={r.lane_set ? r.lane_set.lane : undefined} draggable={!!moveLane && !phone && !editing} onDragStart={(e) => {
      e.dataTransfer?.setData(ROW_DRAG, r.id);
      e.dataTransfer?.setData(dragGroupType(r.group ?? null), r.group ?? "none"); // a type with no data is dropped
    }} tabIndex={editing ? undefined : 0} onClick={editing ? undefined : body} onKeyDown={editing ? undefined : key}>
      {editing ? (
        <RowForm
          project={project}
          row={r}
          save={(f) => act(() => boards.put(id, { id: r.id, rev: r.rev, note: f.note, ...(f.pointer ? { pointer: f.pointer } : {}) }))}
          done={() => setEditing(null)}
        />
      ) : (
        <div class="board-main">
          <div class="board-card-head board-line board-card-title">
            <div class="board-card-lead">
              {needsDot(r) && <span class="board-needs-dot" data-board-needs-dot title="Needs you" role="img" aria-label="Needs you" />}
              {href ? (
                <a class="board-card-name board-title" href={href} target={external ? "_blank" : undefined} rel="noopener noreferrer" data-board-title data-board-item-link>
                  {title}
                </a>
              ) : (
                <span class="board-card-name board-title" data-board-title>
                  {title}
                </span>
              )}
            </div>
            <IconButton label="Row actions" class="bar-btn sq board-more" data-board-row-menu onClick={more}>
              <MoreIcon size="sm" />
            </IconButton>
          </div>
          <div class="board-line board-sub dim">
            {label && <span class="board-ptr" data-board-ptr>{label}</span>}
            {card && <Owners who={r.live!.assignees ?? []} />}
            {r.agent && (
              <span class="board-agent dim" data-row-agent>
                {agentShort(r.agent)}
              </span>
            )}
            {proofLink}
            {setBy && (
              <Chip kind="tag" data-board-lane-set title="A person moved this row. It follows its work again once the work changes.">
                {setBy}
              </Chip>
            )}
          </div>
          {card && <Labels labels={r.live!.labels ?? []} />}
          {(marks.notFinished || marks.cancelled || marks.after || stalledMark || waitingMark || marks.blocked.length > 0) && (
            <div class="board-line board-marks" data-board-marks>
              {marks.notFinished && (
                <StateChip tone="warn" class="board-mark-nf" data-board-mark="not_finished" title={notFinishedTitle(marks.notFinished)}>
                  {notFinishedLine(marks.notFinished)}
                </StateChip>
              )}
              {marks.cancelled && (
                <Chip kind="tag" data-board-mark="cancelled" title="This item was canceled, so the row is done and not proven">
                  cancelled
                </Chip>
              )}
              {marks.after && (
                <StateChip tone="accent" data-board-mark="after" title={marks.after.repo ? `Stacked on a PR of ${marks.after.repo}` : "Stacked on another PR"}>
                  {marks.after.words}
                </StateChip>
              )}
              {stalledMark && (
                <StateChip tone="warn" data-board-mark="stalled" title="Every agent on this row has been idle past the project's limit, and no pull request is open">
                  {stalledMark.words}
                </StateChip>
              )}
              {waitingMark && (
                <StateChip tone={waitingMark.mine ? "accent" : "muted"} data-board-mark="waiting" title="This row has an open question for a person">
                  {waitingMark.words}
                </StateChip>
              )}
              {marks.blocked.map((m, i) => (
                <StateChip key={i} tone="accent" data-board-mark="blocked_by" title={m.title ? `${m.title}${m.repo ? ` (${m.repo})` : ""}` : "You can't read the item that blocks this one"}>
                  {m.words}
                </StateChip>
              ))}
            </div>
          )}
          {closedWithLine(r.live) && (
            <div class="board-line board-sub dim" data-board-closed-with title="The merged pull request that closed this issue, and the words of it that did">
              {closedWithLine(r.live)}
            </div>
          )}
          {(current.length > 0 || (r.sessions ?? []).length > 0 || held) && (
            <div class="board-line board-chips" data-board-attachments>
              {current.map((a) => (
                <AttachmentChip key={a.id} a={a} platform={platform} stalled={stalledWords} waiting={waiting} />
              ))}
              {held && (
                <Chip class="dim" title={held.title} data-board-held={r.held?.session}>
                  {held.text}
                </Chip>
              )}
              {(r.sessions ?? []).map((x) => (
                <SessionChip key={x.session} s={x} project={project} />
              ))}
            </div>
          )}
          {(r.docs ?? []).length > 0 && (
            <div class="board-line board-chips" data-board-row-docs>
              {(r.docs ?? []).map((d) => (
                <DocLink key={d.id} d={d} project={project} platform={platform} />
              ))}
            </div>
          )}
          {earlier.length > 0 && (
            <details class="board-earlier" data-board-earlier={earlier.length}>
              <summary class="dim">{earlierLine(earlier.length)}</summary>
              <div class="board-line board-chips">
                {earlier.map((a) => (
                  <AttachmentChip key={a.id} a={a} platform={platform} />
                ))}
              </div>
            </details>
          )}
          {r.note && (
            <div class="board-note dim" data-board-note>
              <Markdown text={r.note} refs={null} />
            </div>
          )}
          {r.ask && !r.ask.raised && <AskCard a={r.ask} answer={(a) => act(() => boards.answer(id, r.ask!.id, a))} />}
          {r.asked && r.asked.length > 0 && <Answered asked={r.asked} />}
        </div>
      )}
    </Card>
  );
}

/** A row's note and pointer, new or edited. The pointer is sent only when
 * it was changed (an agent block's isn't typed, and stays). "Points at" is a
 * search over the project's work items; a result picked is the pointer, and
 * text typed and not picked is an id or link as before. */
function RowForm({ project, row, save, done }: { project: string; row: BoardRow | null; save: (f: { note: string; pointer?: BoardPointer }) => Promise<boolean>; done: () => void }) {
  const start = row ? pointerText(row.pointer) : "";
  const [note, setNote] = useState(row?.note ?? "");
  const [ptr, setPtr] = useState(start);
  const [picked, setPicked] = useState<BoardPointer | null>(null);
  const [bad, setBad] = useState(false);
  const [found, setFound] = useState<BoardWorkItem[]>([]);
  const [active, setActive] = useState(-1);
  const asked = useRef(0);
  const search = useRef(
    debounced((q: string) => {
      const n = ++asked.current;
      boards.work(project, q, PICK_LIMIT).then(
        (w) => n === asked.current && (setFound(w.items), setActive(-1)),
        () => n === asked.current && setFound([]),
      );
    }, PICK_DELAY_MS),
  ).current;
  useEffect(() => () => search.cancel(), []);
  const listId = useId();
  const optionId = (i: number) => `${listId}-${i}`;

  const type = (text: string) => {
    setPtr(text);
    setPicked(null);
    setBad(false);
    search.cancel();
    // A search that was out is stale now.
    asked.current++;
    if (text.trim() && text.trim() !== start) search(text.trim());
    else setFound([]);
  };
  const pick = (w: BoardWorkItem) => {
    search.cancel();
    asked.current++;
    setPicked(w.pointer);
    setPtr(workLabel(w));
    setFound([]);
    setActive(-1);
  };
  const keys = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (!found.length) return;
      e.preventDefault();
      setActive(stepActive(active, found.length, e.key === "ArrowDown"));
    } else if (e.key === "Enter" && active >= 0 && found[active]) {
      e.preventDefault();
      pick(found[active]);
    } else if (e.key === "Escape" && found.length) {
      e.preventDefault();
      e.stopPropagation();
      setFound([]);
      setActive(-1);
    }
  };
  const submit = async (e: Event) => {
    e.preventDefault();
    const sent = pointerToSend(ptr, start, picked);
    if (!sent) return setBad(true);
    const ok = await save({ note: note.trim(), ...sent });
    if (ok) done();
  };
  return (
    <form class="board-form" data-board-form onSubmit={(e) => void submit(e)}>
      <input type="text" value={note} maxLength={280} placeholder="note" aria-label="Note" data-board-note-input autoFocus onInput={(e) => setNote(e.currentTarget.value)} />
      <div class="board-pick">
        <input
          type="text"
          role="combobox"
          aria-expanded={found.length > 0}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={active >= 0 ? optionId(active) : undefined}
          autocomplete="off"
          value={ptr}
          placeholder="item: search issues and PRs, or iss_…, fi_… or https://…"
          aria-label="Item"
          aria-invalid={bad}
          data-board-ptr-input
          onInput={(e) => type(e.currentTarget.value)}
          onKeyDown={keys}
        />
        <ul class="board-pick-list" role="listbox" id={listId} aria-label="Work items" hidden={!found.length} data-board-pick-list>
          {found.map((w, i) => (
            <li key={w.pointer.kind + ("id" in w.pointer ? w.pointer.id : "")} id={optionId(i)} role="option" aria-selected={i === active} class={i === active ? "active" : undefined} data-board-pick={i} onMouseDown={(e) => (e.preventDefault(), pick(w))}>
              <span class="board-ptr">{workLabel(w)}</span>
              <span class="board-pick-title">{w.title}</span>
              <StateChip tone="muted">{w.state}</StateChip>
            </li>
          ))}
        </ul>
      </div>
      {bad && <span class="board-bad">Expected an id that starts iss_ or fi_, or an https:// link. A run or an agent block is attached to a row.</span>}
      <span class="board-form-buttons">
        <button type="submit" class="primary small" data-board-save>
          Save
        </button>
        <button type="button" class="secondary small" onClick={done}>
          Cancel
        </button>
      </span>
    </form>
  );
}
