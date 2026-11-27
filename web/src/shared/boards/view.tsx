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
// Where it goes (Arugula #1129): the board block draws it all in one column,
// as ever. The Boards page puts the controls (find and its
// Label, Person and Group menus, the Docs, the adds, Workers and ⋯) in a
// column at the right, under small headings, and a row's drawer takes that
// column while it's open. With under ~900px of room the column goes under the
// board (as a list page's aside does, #1127) and the drawer is a sheet again.
//
// It reads the board as the person looking: now, again when the platform
// says it changed (the board's thread, chat's live stream), and when the
// page comes back into view. A write on an entry that moved on is refused
// (409): the view reads the board again and says so, and the change is theirs
// to make again.

import { useEffect, useId, useRef, useState, useContext } from "preact/hooks";
import type { BoardLead, BoardAnsweredAsk, BoardGroup, BoardPointer, LayoutNewPane, LayoutPane, ProjectBoard, BoardRow, BoardView, BoardWorkItem, BoardZone } from "../proto";
import type { TabItem } from "../kit/tabs";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { sessionLabel } from "../chief-of-staff";
import { ChevronIcon, EditIcon, IconButton, MoreIcon, PlusIcon } from "../ui/icons";
import { ConfirmRemove } from "../ui/confirm";
import { StateChip, Chip } from "../ui/state-chip";
import { usePhone, useSubscribe } from "../ui/hooks";
import { DocActions, openTab } from "../docs/doc-actions";
import { DocTimes } from "../docs/doc-times";
import { BlockError, BlockLoading, EmptyLine, EmptyState } from "../ui/block-states";
import { PageHeader } from "../ui/page-header";
import { SearchField } from "../ui/search-field";
import { SectionHead } from "../ui/section-head";
import { askText, MenuHost, menuBelow, openMenu, type MenuItem } from "../ui/menu";
import { issues } from "../issues/store";
import { day } from "../time";
import { BoardError, STALE_REV, boards } from "./store";
import { DocLink, DocPicker } from "./docs";
import { AddRowSheet } from "./add-row";
import { AlertStrip, BoardNeeds, DeskTabs, LeadPanel, PrFold, askNeed, RowItems, rowItemWords, useBoardInbox } from "./desk";
import { boardPlace, ChangedLine, forYou, LANES, layoutItems, PaneBody, paneItem, paneMeta, prOf, rowOf, tabTitle } from "./arranged";
import { busyNow, HOME, usePlaceLayout } from "../place-layout";
import { LayoutSheet, useLayoutPrefs } from "../layout-prefs";
import { Panes, type PaneItem } from "../kit/panes/panes";
import { both, useBindings } from "../keys";
import { Avatar } from "../kit/avatar";
import { NeedRow } from "../kit/need-row";
import { Mark } from "../kit/state-mark";
import { Surface } from "../kit/surface";
import { CardSessions, Drawer, useRowSessions, type Act } from "./card";
import { useLiveRowSessions } from "./live-sessions";
import "./bar.css";
import "./boards.css";
import { groupFold, groupLaneFold, isFolded, laneFold, loadFolds, saveFolds, type Folds } from "./folds";
import { PICK_DELAY_MS, groupMove, PICK_LIMIT, GOAL_MAX, debounced, epicLine, groupLine, isCard, laneCount, marksOf, waitingOf, needsYou, pointerHref, pointerLabel, pointerText, pointerToSend, sections, sourceLine, sourceLinked, leftOutLine, stepActive, workLabel, findLine, openPrs, needsFirst, DONE_SHOWN, tileState, tileMeta, liveSessions, firstLine, progressWords, deskLine, type OpenPr, type BoardFilter, rowPasses, filtering, filterChoices, filterHash, parseFilterHash, groupChoices } from "./text";

/** The narrowest a pane gets before the next one stacks under it: at 1440, the board and two panes beside it, the third stacked. */
const PANE_MIN = 280;

const STALE_SAID = "That changed while you were editing, so the board is read again. Make your change again if it still applies.";

/** What a card carries while it's dragged to another group (#664). No lane takes it: a row's lane follows its work (Arugula #1383). */
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
  write = true,
  keepInUrl = false,
  onProject,
  onBoard,
  cardSessions = false,
  placement = "column",
  extra = [],
  docTab,
}: {
  id: string;
  /** False for a block that isn't on screen: it reads nothing then. */
  shown?: boolean;
  /** A path of the platform page (`/issues/iss_…`) as a link, if there is one. */
  platform: (path: string) => string | undefined;
  /** Whether to offer the board's own edits (title, goal, closing). False for a viewer; a block can't tell, so it offers them and the platform's 403 comes back as a toast. */
  write?: boolean;
  /** Keep the find text and the Label and Person menus in the URL's hash (#547), so a link keeps them. The shell's block keeps them in memory. */
  keepInUrl?: boolean;
  /** The board's project, once it's read: the platform's page puts its address right after a move (Arugula #972). */
  onProject?: (project: string) => void;
  /** A write here changed what a list of boards shows of it (its title, whether it's closed, its progress): the Boards page reads its list again (Arugula #1159). */
  onBoard?: (b: ProjectBoard) => void;
  /** Read the project's sessions by card, and show each card's on its row and in its drawer (Arugula #1082): the platform's page, which the Sessions page folded into. */
  cardSessions?: boolean;
  /** How it's drawn: `column`, the controls over the lanes (the board block's); `desk`, the Boards page's (Lift 5a, ./desk.tsx): tabs, the alert strip, the PR fold and the title block on the left two thirds, Needs you and the lead on the right third. */
  placement?: "column" | "desk";
  /** The page's own items for the desk's ⋯ (Workers). */
  extra?: MenuItem[];
  /** A project Doc's tab, as the page draws a Doc. */
  docTab?: (doc: string, more: MenuItem[]) => preact.ComponentChildren;
}) {
  const { toast } = usePlatformApi();
  const [view, setView] = useState<BoardView | null>(null);
  const [missing, setMissing] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  /** The row whose drawer is open. */
  const [drawer, setDrawer] = useState<string | null>(null);
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
  /** The close-epic form is open and what it has been given: the trigger is in the view switch's row, the form under the progress bar. */
  const [epicClosing, setEpicClosing] = useState(false);
  const [epicNote, setEpicNote] = useState("");
  /** The find box's text (#546); the bar is always there (#614), so empty is no find. */
  const [find, setFind] = useState<string>(() => (keepInUrl ? (parseFilterHash(location.hash)?.q ?? "") : ""));
  /** The Label and Person menus' values (#547): any of a menu's matches, and every menu and the text have to. */
  const [labels, setLabels] = useState<string[]>(() => (keepInUrl ? (parseFilterHash(location.hash)?.labels ?? []) : []));
  const [people, setPeople] = useState<string[]>(() => (keepInUrl ? (parseFilterHash(location.hash)?.people ?? []) : []));
  const [inGroups, setInGroups] = useState<string[]>(() => (keepInUrl ? (parseFilterHash(location.hash)?.groups ?? []) : []));
  const findBox = useRef<HTMLInputElement>(null);
  // Another board starts with no filter; the first one keeps what its link said.
  const was = useRef(id);
  useEffect(() => {
    if (was.current === id) return;
    was.current = id;
    setFind("");
    setLabels([]);
    setPeople([]);
    setInGroups([]);
  }, [id]);
  /** Clear and Esc: no text, no menu values. */
  const clearFind = () => (setFind(""), setLabels([]), setPeople([]), setInGroups([]));
  const filter: BoardFilter = { q: find, labels, people, groups: inGroups };
  useEffect(() => {
    if (!keepInUrl || !shown) return;
    const h = filterHash(filter);
    if (location.hash.replace(/^#/, "") !== h) history.replaceState(history.state, "", location.pathname + location.search + (h ? `#${h}` : ""));
  }, [keepInUrl, shown, find, labels, people, inGroups]);
  // Chat says when the thread's object changed; it's on while chat is.
  useSubscribe((fn) => chat.subscribe(fn));
  const seen = useRef(0);
  // The project's Plane connections, once the board says its project (#359): New issue… may make the item in Plane.
  const [plane, setPlane] = useState<{ id: string; identifier: string }[]>([]);
  const project = view?.board.project;
  useEffect(() => {
    if (shown && project) void boards.plane(project).then(setPlane);
    if (project) onProject?.(project);
  }, [project, shown]);
  /** The goal's text while its form is open; null while it's shut. */
  const [goalDraft, setGoalDraft] = useState<string | null>(null);
  const desk = placement === "desk";
  const phone = usePhone();
  /** The desk's tab now: the board's own (home), or one opened beside it. Only you switch it (Lift 5b): an agent's tab never becomes current. */
  const [tab, setTabNow] = useState(HOME);
  const setTab = (t: string) => (busyNow(), setTabNow(t));
  /** The pane you focused (a click), and the one you maximised (⤢, esc back). */
  const [focused, setFocused] = useState<string | null>(null);
  const [max, setMax] = useState<string | null>(null);
  /** Tabs you've looked at since they opened: their dot goes. */
  const [looked, setLooked] = useState<ReadonlySet<string>>(new Set());
  const arranged = usePlaceLayout(desk && shown ? boardPlace(id) : null, (why) => toast(`Couldn't change the layout: ${why}`));
  const layout = arranged.layout;
  /** The sentences the lead follows (§3.10, `,`); none where the platform keeps none. */
  const prefs = useLayoutPrefs((why) => toast(`Couldn't keep it: ${why}`));
  const [prefsOpen, setPrefsOpen] = useState(false);
  const tabNow = layout.tabs.find((t) => t.id === tab) ?? layout.tabs[0]!;
  useEffect(() => {
    if (tabNow.id !== tab) setTabNow(HOME);
    setLooked((s) => (s.has(tabNow.id) ? s : new Set([...s, tabNow.id])));
  }, [tabNow.id]);
  /** The desk's find bar is shown (Filter, `f`, `/`); a find in use keeps it. */
  const [filterOpen, setFilterOpen] = useState(false);
  /** Link a Doc's picker is open (the desk's ⋯). */
  const [picking, setPicking] = useState(false);
  /** Add row's Sheet is open (the desk's Add row, `a`). */
  const [adding, setAdding] = useState(false);
  const prs = view ? openPrs(view) : [];
  const tabs: TabItem[] = layout.tabs.map((t, i) => ({ id: t.id, title: tabTitle(t, view?.board.title ?? "Board"), count: i + 1, closes: t.id !== HOME, dot: t.id !== tab && ((t.for_you && !looked.has(t.id)) || t.panes.some((p) => arranged.quiet.has(p.id))) }));
  // The desk's keys (Lift 5a): its tabs by number, a row at a time, Filter, Add row and the lead.
  const tile = (by: number) => {
    const all = [...document.querySelectorAll<HTMLElement>(`[data-board="${id}"] [data-board-row]`)];
    const at = all.indexOf(document.activeElement as HTMLElement);
    all[Math.max(0, Math.min(all.length - 1, at < 0 ? 0 : at + by))]?.focus();
  };
  const openFind = () => (setTab(HOME), setFilterOpen(true), requestAnimationFrame(() => findBox.current?.focus()));
  useBindings(
    desk && shown && view
      ? [
          { id: "board.tab", label: "A tab of the board", chip: "tabs", keys: both("Digit1", "Digit2", "Digit3", "Digit4"), hint: { mac: "1–4", other: "1–4" }, run: (e) => { const t = tabs[Number(e.code.slice(5)) - 1]; if (t) setTab(t.id); } },
          // Leaving a maximised pane is Esc's back; the panes' other actions are their buttons (no new keys until #1534's list).
          { id: "board.pane.back", label: "Back from a maximised pane", group: "panes", keys: both("key:escape"), hint: { mac: "esc", other: "esc" }, when: () => !!max, run: () => setMax(null) },
          { id: "board.next", label: "The next row", chip: "move", keys: both("key:j"), hint: { mac: "j k", other: "j k" }, run: () => tile(1) },
          { id: "board.prev", label: "The row before", keys: both("key:k"), run: () => tile(-1) },
          { id: "board.find", label: "Filter the rows", keys: both("key:f", "key:/"), run: openFind },
          { id: "board.add", label: "Add a row", keys: both("key:a"), run: () => (setTab(HOME), setAdding(true)) },
          { id: "board.lead", label: "Write to the lead", chip: "lead", keys: both("key:l"), run: () => document.querySelector<HTMLElement>("[data-board-thread] textarea")?.focus() },
        ]
      : [],
    [desk, shown, !!view, tabs.map((t) => t.id).join(), id, max],
  );
  // The agents' items on the board's issues, from the project's Inbox (Arugula #1469).
  const inbox = useBoardInbox(placement === "desk" ? view : null);
  const itemWords = rowItemWords(inbox);
  const byCard = useLiveRowSessions(project, view?.zones.flatMap((z) => z.rows) ?? [], { shown, cards: !!cardSessions }, () => again());

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

  // A link from a block or a session names a row, `#row=row_…` (#372),
  // or what it points at (`#row=iss_…`, from a link that knows a card's
  // board but not its row): the card's drawer opens on it
  // (decision 9), as a click on it would. Read as the board opens:
  // keeping the filter in the URL replaces the hash.
  const [named] = useState(() => location.hash.match(/^#row=([A-Za-z0-9_-]+)/)?.[1]);
  const rowKey = view && named ? (view.zones.flatMap((z) => z.rows).find((r) => r.id === named || ("id" in r.pointer && r.pointer.id === named))?.id ?? named) : undefined;
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
      const was = view.board;
      const now = await write();
      setView(now);
      setStale(false);
      if (listed(was) !== listed(now.board)) onBoard?.(now.board);
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
  /** A row dropped on a group (#664): it moves there with its lane unchanged. */
  const dropRow = (row: string, group: string | null) => {
    const r = view.zones.flatMap((z) => z.rows).find((x) => x.id === row);
    if (r && (r.group ?? null) !== group) moveRow(r, group);
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
  const closeEpic = write && view.board.closed_at == null && !epicClosing ? () => (setEpicNote(""), setEpicClosing(true)) : undefined;
  const more = boardMenu(write, () => void rename(), () => setGoalDraft(view.board.goal), closeEpic);
  const copyLink = () =>
    navigator.clipboard.writeText(shareLink ?? "").then(
      () => toast("Copied the board's link"),
      () => toast("Couldn't copy the link"),
    );
  // The desk's ⋯: the sidebar's adds and Docs, then the board's own, then the page's.
  const deskMore: MenuItem[] = [
    ...(shareLink ? [{ label: "Copy link", run: () => void copyLink(), attrs: { "data-copy-link": "", "data-link": shareLink } }] : []),
    { label: "Add an existing item…", run: () => setEditing("new"), attrs: { "data-board-add-existing": "" } },
    ...(write ? [{ label: "Link a Doc…", run: () => setPicking(true), attrs: { "data-board-doc-add": "" } }] : []),
    ...(write ? [{ label: "Add a group…", run: () => void addGroup(), attrs: { "data-board-add-group": "" } }] : []),
    ...plane.map((c) => ({ label: `New issue in Plane (${c.identifier})…`, run: () => void newPlaneIssue(c), attrs: { "data-board-new-plane-issue": c.identifier } })),
    ...(more.length ? ["separator" as const, ...more] : []),
    ...(extra.length ? ["separator" as const, ...extra] : []),
  ];
  const grouped = !!view.groups?.length;


  const findBar = (
    <div class="board-find" role="search" data-board-find>
      {/* The page's search answers `/`; a block's doesn't (the shell's sidebar does). */}
      <SearchField
        inputRef={findBox}
        value={find}
        onValue={setFind}
        label="Find a row"
        placeholder="Find a row: #541, ARUGU-12 or words"
        slash={keepInUrl && shown && !desk}
        data-board-find-input
        onKeyDown={(e) => {
          if (e.key === "Escape") (e.preventDefault(), clearFind(), e.currentTarget.blur());
          else if (e.key === "Enter" && firstMatch) (e.preventDefault(), setDrawer(firstMatch.id));
        }}
      />
      <FilterMenu name="label" title="Label" choices={filterChoices(allRows, "labels")} picked={labels} set={setLabels} />
      <FilterMenu name="person" title="Person" choices={filterChoices(allRows, "assignees")} picked={people} set={setPeople} />
      {grouped && <FilterMenu name="group" title="Group" choices={groupChoices(view.groups ?? [], allRows)} picked={inGroups} set={setInGroups} />}
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
  );
  const rowForm = <RowForm project={view.board.project} row={null} save={(f) => act(() => boards.put(id, { entry: "row", note: f.note, pointer: f.pointer ?? null }))} done={() => setEditing(null)} />;
  const adds = (
    <>
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
    </>
  );
  const drawn = opened && (
    <Drawer
      place="sheet"
      r={opened}
      platform={platform}
      close={() => setDrawer(null)}
      project={view.board.project}
      others={allRows}
      write={write}
      act={act}
      id={id}
    />
  );

  /** A group of the board (its section: the heading and its lanes), or the lanes of a board with none. */
  const drawSection = (s: ReturnType<typeof sections>[number]) => {
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
          <Lane key={z.id} z={z} fold={{ key: laneKey, folded: !narrows && isFolded(folds, laneKey), toggle: () => toggleFold(laneKey) }} project={view.board.project} editing={editing} setEditing={setEditing} act={act} id={id} platform={platform} groups={write ? (view.groups ?? []) : []} moveRow={moveRow} open={setDrawer} opened={drawer} asks={!desk} />
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
  };

  // The desk's panes (Lift 5b): the board's groups are its default panes;
  // what the lead and you opened is beside them, from the layout record.
  // What you open takes the focus; what the lead opens never does.
  const defaultName = (sec: ReturnType<typeof sections>[number]) => (grouped ? (sec.group?.id ?? "none") : LANES);
  const defaultTitle = (name: string) => (name === LANES ? "Board" : name === "none" ? "No group" : ((view.groups ?? []).find((g) => g.id === name)?.title ?? name));
  const show = async (pane: LayoutNewPane) => {
    const r = await arranged.change({ op: "show", pane, intent: "beside", tab: tabNow.id, in_place_of: null });
    if (r?.pane) setFocused(r.pane);
  };
  const openPr = (p: OpenPr) => void show({ kind: "pr", ref: p.live.repo ? `${p.live.repo}#${p.live.number}` : p.label, title: `#${p.live.number} ${p.live.title}`, machine: null, state: null });
  const linked = (view.docs ?? []).filter((d) => d.pointer.kind === "doc");
  const addPane: MenuItem[] = [
    ...linked.map((d) => ({ label: d.title, run: () => void show({ kind: "doc", ref: "id" in d.pointer ? d.pointer.id : d.id, title: d.title, machine: null, state: null }), attrs: { "data-board-add-pane": d.id } })),
    ...prs.map((p) => ({ label: `${p.label} ${p.live.title}`, run: () => openPr(p), attrs: { "data-board-add-pane": p.id } })),
  ];
  const closePane = (pane: string) => {
    void arranged.change({ op: "close", pane });
    if (focused === pane) setFocused(null);
    if (max === pane) setMax(null);
  };
  const maxPane = (pane: string, opened: boolean) => {
    if (max === pane) return setMax(null);
    setMax(pane);
    setFocused(pane);
    if (opened) void arranged.change({ op: "maximise", pane });
  };
  const reset = () => (setMax(null), setFocused(null), void arranged.change({ op: "reset" }));
  /** The board's own panes: a Doc, a PR's or a card's drawer. */
  const own = (p: LayoutPane) => {
    if (p.kind === "doc") {
      const d = linked.find((x) => "id" in x.pointer && x.pointer.id === p.ref);
      const unlink = d && write ? () => (closePane(p.id), void act(() => boards.removeDoc(id, d.id))) : undefined;
      const menu = unlink && d ? [{ label: "Take off this board", run: unlink, attrs: { "data-board-doc-remove": d.id } }] : [];
      if (d?.missing) return <DocLink d={d} project={view.board.project} platform={platform} remove={unlink} />;
      return docTab ? docTab(p.ref, menu) : null;
    }
    const r = p.kind === "pr" ? prOf(p.ref, prs)?.row : p.kind === "card" ? rowOf(p.ref, view) : undefined;
    return r ? <Drawer place="column" r={r} platform={platform} close={() => closePane(p.id)} project={view.board.project} others={allRows} write={write} act={act} id={id} /> : null;
  };
  const item = (p: LayoutPane): PaneItem =>
    paneItem(p, {
      body: <PaneBody p={p} project={view.board.project} platform={platform} own={own} />,
      meta: paneMeta(p, defaultTitle),
      dot: arranged.quiet.has(p.id),
      arriving: arranged.arrived.has(p.id),
      mine: !p.opened_by.machine,
      move: tabNow.id === HOME ? () => void arranged.change({ op: "move", pane: p.id, tab: null, index: null }) : undefined,
      max: () => maxPane(p.id, true),
      close: () => closePane(p.id),
      restore: p.in_place_of ? { name: defaultTitle(p.in_place_of), run: () => closePane(p.id) } : undefined,
    });
  const home = layout.tabs[0]!;
  const defaults: PaneItem[] =
    tabNow.id !== HOME
      ? []
      : sections(narrowed).flatMap((sec): PaneItem[] => {
          const name = defaultName(sec);
          if (layout.closed_defaults.includes(name)) return [];
          // The lead shows something in its place for a while: ↺ puts it back.
          const instead = home.panes.find((p) => p.in_place_of === name);
          if (instead) return [item(instead)];
          const body = drawSection(sec);
          return body ? [{ id: name, title: defaultTitle(name), default: true, size: layout.default_sizes[name], max: () => maxPane(name, false), close: () => closePane(name), body, data: { "data-board-default": name } }] : [];
        });
  const openedPanes = tabNow.panes.filter((p) => tabNow.id !== HOME || !p.in_place_of).map(item);
  const panes = (
    <Panes
      label={tabNow.id === HOME ? view.board.title : tabTitle(tabNow, view.board.title)}
      defaults={defaults}
      opened={openedPanes}
      focused={focused}
      max={max}
      min={PANE_MIN}
      onFocus={(pane) => (pane !== focused && setFocused(pane), arranged.seen(pane))}
      onResize={(pane, size) => void arranged.change({ op: "resize", pane, size })}
    />
  );

  const board = (
    <div class="board" data-board={id}>
      {desk ? (
        <>
          <AlertStrip project={view.board.project} platform={platform} />
          <PrFold prs={prs} open={openPr} />
          <DeskHead
            view={view}
            write={write}
            more={[...deskMore, ...(prefs.prefs ? [{ label: "Layout…", run: () => setPrefsOpen(true), attrs: { "data-board-layout-prefs": "" } }] : []), ...layoutItems(layout, reset, (pane) => void arranged.change({ op: "reopen", pane }))]}
            prs={prs}
            actions={
              <>
                <ChangedLine layout={layout} reset={reset} />
                {addPane.length > 0 && (
                  <button type="button" class="secondary small" aria-haspopup="menu" data-board-add-pane-menu onClick={(e) => menuAt(e, addPane)}>
                    <PlusIcon size="xs" />
                    pane
                  </button>
                )}
                <button type="button" class="secondary small" aria-pressed={filterOpen || narrows} aria-keyshortcuts="f" data-board-filter onClick={() => (filterOpen && !narrows ? setFilterOpen(false) : openFind())}>
                  Filter
                </button>
                <button type="button" class="secondary small" aria-keyshortcuts="a" data-board-add-row onClick={() => setAdding(true)}>
                  <PlusIcon size="xs" />
                  Add row
                </button>
              </>
            }
            goal={goalDraft}
            setGoal={setGoalDraft}
            act={act}
            id={id}
          />
          {picking && <DocPicker project={view.board.project} taken={new Set((view.docs ?? []).flatMap((d) => (d.pointer.kind === "doc" ? [d.pointer.id] : [])))} pick={(p) => act(() => boards.addDoc(id, p))} close={() => setPicking(false)} />}
          {(filterOpen || narrows) && findBar}
          {editing === "new" && rowForm}
          {adding && <AddRowSheet project={view.board.project} board={id} close={() => setAdding(false)} done={() => (setAdding(false), again())} />}
        </>
      ) : (
        <>
          <BoardHead view={view} write={write} shareLink={shareLink} more={more} goal={goalDraft} setGoal={setGoalDraft} act={act} id={id} platform={platform} />
          {findBar}
        </>
      )}
      <Epic b={view.board} closing={epicClosing} setClosing={setEpicClosing} note={epicNote} setNote={setEpicNote} act={act} id={id} write={write} view={view} desk={desk} />
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
          why={`A row is one piece of work: an issue or a PR. Its lane follows that work, so nobody sets it.${write ? "" : " Its writers add rows."}`}
          action={
            write && (
              <button type="button" class="secondary small" data-board-add-row onClick={() => (desk ? setAdding(true) : setEditing("new"))}>
                <PlusIcon size="xs" />
                Add row
              </button>
            )
          }
        />
      )}
      {desk ? panes : sections(narrowed).map(drawSection)}
      {view.said && (
        <p class="board-said" role="status" data-board-said>
          {view.said}
        </p>
      )}
      {!desk && (editing === "new" ? rowForm : <p class="board-foot">{adds}</p>)}
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
      {drawn}
    </div>
  );
  if (!desk) return <CardSessions.Provider value={byCard}>{board}</CardSessions.Provider>;

  // While panes are open beside the board, its right column narrows to a quarter.
  const beside = openedPanes.length > 0 || tabNow.id !== HOME;
  return (
    <CardSessions.Provider value={byCard}>
      <RowItems.Provider value={itemWords}>
        <div class={beside ? "board-desk is-beside" : "board-desk"} data-board-desk={id} data-board-beside={beside ? "" : undefined}>
          <section class="board-desk-main" aria-label={view.board.title}>
            <DeskTabs tabs={tabs} current={tabNow.id} pick={setTab} close={(t) => (void arranged.change({ op: "close_tab", tab: t }), t === tab && setTab(HOME))} />
            {tabNow.id === HOME ? (
              board
            ) : (
              <div class="board-desk-tab" data-board-tab={tabNow.id}>
                {panes}
              </div>
            )}
          </section>
          {prefsOpen && prefs.prefs && <LayoutSheet prefs={prefs.prefs} flip={prefs.flip} close={() => setPrefsOpen(false)} />}
          <div class="board-desk-side">
            <BoardNeeds view={view} answer={(r, a) => act(() => boards.answer(id, r.ask!.id, a))} inbox={inbox} platform={platform} tabs={forYou(inbox?.items ?? [], boardPlace(id)).map((i) => ({ item: i, n: layout.tabs.findIndex((t) => t.id === i.subject.tab) + 1, go: () => setTab(i.subject.tab!) }))} />
            <LeadPanel view={view} id={id} phone={phone} />
          </div>
        </div>
      </RowItems.Provider>
    </CardSessions.Provider>
  );
}

/** What a list of boards shows of one: a write that changes it is told to the list. */
const listed = (b: ProjectBoard) => JSON.stringify([b.title, b.closed_at ?? null, b.progress]);

/** The board's ⋯: Rename and Edit goal for a writer, and Close epic while it can be closed. */
function boardMenu(write: boolean, rename: () => void, editGoal: () => void, close?: () => void): MenuItem[] {
  return [
    ...(write
      ? [
          { label: "Rename…", run: rename, attrs: { "data-board-rename": "" } },
          { label: "Edit goal…", run: editGoal, attrs: { "data-board-goal-menu": "" } },
        ]
      : []),
    ...(close ? [{ label: "Close epic…", run: close, attrs: { "data-board-close": "" } }] : []),
  ];
}

/** The board's header, as a Doc's (#929): the title and, at its right, Copy link, Open in browser and ⋯ (`more`, none when the aside has it); when it was made and last edited; the goal with the Docs beside it (`docs`, false when the aside has them); and the progress as its bottom rule. `goal` is the goal's text while its form is open. The panel doesn't know whether the person may write (the pin block has no project), so without `write` the controls still show and the platform's refusal comes back as `act`'s toast. */
function BoardHead({ view, back, write, shareLink, more: items, goal: draft, setGoal: setDraft, act, id, platform }: { view: BoardView; back?: preact.ComponentChildren; write: boolean; shareLink: string | null; more: MenuItem[]; goal: string | null; setGoal: (g: string | null) => void; act: Act; id: string; platform: (path: string) => string | undefined }) {
  const b = view.board;
  const { toast } = usePlatformApi();
  const e = epicLine(b.progress);
  const copy = async (): Promise<"ok" | false> => {
    try {
      await navigator.clipboard.writeText(shareLink ?? "");
      return "ok";
    } catch {
      toast("Couldn't copy the link");
      return false;
    }
  };
  const docs = true;
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
      {b.lead && <LeadLine lead={b.lead} id={id} />}
      <GrantLine view={view} id={id} write={write} act={act} />
      {(write || b.goal || (docs && ((view.docs ?? []).length > 0 || write))) && (
        <div class="board-head-body">
          <div class="board-goal-col">
            <Goal b={b} write={write} draft={draft} setDraft={setDraft} act={act} id={id} />
          </div>
          <DocsStrip view={view} act={act} id={id} write={write} platform={platform} />
        </div>
      )}
      <ProgressBar b={b} words={e.words} />
    </PageHeader>
  );
}

/** The board's progress as a bar of its lanes' shares. */
function ProgressBar({ b, words }: { b: ProjectBoard; words: string }) {
  const e = epicLine(b.progress);
  return b.progress.total > 0 ? (
    <div class="board-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.progress.total} aria-valuenow={b.progress.done} aria-label={words} data-board-progress>
      {e.segments.map((s) => (
        <i key={s.lane} data-lane={s.lane} style={{ width: `${s.pct}%` }} />
      ))}
    </div>
  ) : (
    <div class="board-bar" aria-hidden="true" data-board-progress />
  );
}

/** The desk's title block (Lift 5a): the title, the meta line (`140/191 · 2 ask you · #2868 queued`) and the actions (Filter, Add row, ⋯), with the lead and its machine at the right; the grant, the goal, and the progress line under it. */
function DeskHead({ view, write, more: items, actions, prs, goal: draft, setGoal: setDraft, act, id }: { view: BoardView; write: boolean; more: MenuItem[]; actions: preact.ComponentChildren; prs: OpenPr[]; goal: string | null; setGoal: (g: string | null) => void; act: Act; id: string }) {
  const api = usePlatformApi();
  const b = view.board;
  const mine = view.zones.flatMap((z) => z.rows).filter((r) => r.ask?.mine && !r.ask.raised).length;
  const machine = b.lead ? b.lead.machine_name || api.machine(b.lead.machine)?.name || "" : "";
  const words = progressWords(b.progress);
  return (
    <header class="board-desk-head" data-board-head>
      <div class="board-desk-title">
        <div class="board-desk-name">
          <h1 data-board-heading>{b.title}</h1>
          <p class="m-meta board-desk-meta" data-board-meta>
            {deskLine(b.progress, mine, prs).map((w, i) => (
              <span key={i} class={i === 1 && mine ? "k-needs" : undefined}>
                {w}
              </span>
            ))}
          </p>
        </div>
        {b.lead && (
          <dl class="board-desk-facts" data-board-lead-facts={b.lead.session}>
            <div>
              <dt class="m-meta">lead</dt>
              <dd>
                <Avatar name={sessionLabel(b.lead.session, api.projectName)} agent /> {sessionLabel(b.lead.session, api.projectName)}
              </dd>
            </div>
            {machine && (
              <div>
                <dt class="m-meta">machine</dt>
                <dd>{machine}</dd>
              </div>
            )}
          </dl>
        )}
        <div class="board-desk-actions">
          {actions}
          {items.length > 0 && (
            <IconButton label="More" class="bar-btn sq" aria-haspopup="menu" data-board-more data-page-more onClick={(ev) => openMenu(ev, items, { from: ev.currentTarget, end: true })}>
              <MoreIcon size="sm" />
            </IconButton>
          )}
        </div>
      </div>
      <GrantLine view={view} id={id} write={write} act={act} />
      <Goal b={b} write={write} draft={draft} setDraft={setDraft} act={act} id={id} />
      <div class="board-desk-progress">
        <ProgressBar b={b} words={words} />
        <span class="m-meta" data-board-progress-words>
          {words}
        </span>
      </div>
    </header>
  );
}

/** The board's goal: click to edit for a writer (`draft` is its text while the form is open), plain for a reader, nothing when there's none to show. */
function Goal({ b, write, draft, setDraft, act, id }: { b: ProjectBoard; write: boolean; draft: string | null; setDraft: (g: string | null) => void; act: Act; id: string }) {
  const goalOpen = draft !== null;
  const goal = draft ?? "";
  const setGoal = (g: string) => setDraft(g);
  const setGoalOpen = (on: boolean) => setDraft(on ? b.goal : null);
  const editGoal = () => setGoalOpen(true);
  const saveGoal = async () => {
    if (await act(() => boards.edit(id, { goal: goal.trim() }))) setGoalOpen(false);
  };
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
  return goalOpen ? (
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
    );
}

/** Who leads the board and on what machine, in its header (Arugula
 * iss_89b78ece43ac3d6271f78d797cdbdc61): Talk opens the board's thread,
 * which its lead reads; Open, its pane, where it's in the fleet this page
 * reaches and the reader's own (a teammate's lead is talked to, not
 * opened). */
function LeadLine({ lead, id }: { lead: BoardLead; id: string }) {
  const api = usePlatformApi();
  useSubscribe((fn) => api.ownPane?.watch(fn) ?? (() => {}));
  const machine = lead.machine_name || api.machine(lead.machine)?.name || "";
  const open = lead.pane !== undefined && !!api.ownPane?.here(lead.machine, lead.pane);
  const talk = () =>
    boards.thread(id).then(
      (room) => api.nav.openRoom(room.id),
      (e: Error) => api.toast(`Couldn't open the board's thread: ${e.message}`),
    );
  return (
    <p class="doc-meta board-lead" data-board-lead={lead.session}>
      <span>
        Led by <b>{sessionLabel(lead.session, api.projectName)}</b>
        {machine && ` on ${machine}`}
      </span>
      <button type="button" class="link" title="The board's thread: its lead reads it" data-board-lead-talk onClick={() => void talk()}>
        Talk
      </button>
      {open && (
        <button type="button" class="link" title="Its pane, on its machine" data-board-lead-open onClick={() => api.goToPane(lead.machine, String(lead.pane), null)}>
          Open
        </button>
      )}
    </p>
  );
}

/** The board's grant to start agents, under who leads it (Arugula #1416):
 * where its lead runs, which machines its issue agents may use, how many at
 * once and until when, with Revoke for the person who gave it. A board with
 * no lead and no grant offers Run this board, whose card goes in the
 * reader's Needs you. */
function GrantLine({ view, id, write, act }: { view: BoardView; id: string; write: boolean; act: Act }) {
  const { toast } = usePlatformApi();
  const [busy, setBusy] = useState(false);
  const b = view.board;
  const g = view.grant;
  if (!g) {
    if (b.lead || b.closed_at != null || !write) return null;
    const run = async () => {
      setBusy(true);
      try {
        const r = await boards.run(id);
        toast(r.again ? "Its Run this board card is waiting in Needs you." : "Run this board: approve its card in Needs you.");
      } catch (e) {
        toast(`Couldn't ask to run it: ${(e as Error).message}`);
      } finally {
        setBusy(false);
      }
    };
    return (
      <p class="doc-meta board-grant" data-board-grant="">
        <button type="button" class="link" disabled={busy} title="Its card goes in your Needs you: Approve starts its lead and grants it agents" data-board-run onClick={() => void run()}>
          Run this board
        </button>
      </p>
    );
  }
  const revoke = async () => {
    setBusy(true);
    await act(() => boards.revokeGrant(id));
    setBusy(false);
  };
  const until = g.lapses_at ? `until ${new Date(g.lapses_at).toLocaleDateString(undefined, { month: "short", day: "numeric" })}` : "until the board closes";
  const by = g.mine ? "" : g.person_name ? ` (${g.person_name}'s)` : "";
  return (
    <p class="doc-meta board-grant" data-board-grant={g.id}>
      <span data-board-grant-line>
        Grant{by}: lead on {g.lead_machine.name} · agents on {g.machines.map((m) => m.name).join(", ")} · up to {g.cap} · {until}
      </span>
      {g.mine && (
        <button type="button" class="link" disabled={busy} title="Nothing more starts under it; its lead and agents run on" data-board-grant-revoke onClick={() => void revoke()}>
          Revoke
        </button>
      )}
    </p>
  );
}

/** A board's source, who needs you, and closing, under its header. */
function Epic({ b, closing, setClosing, note, setNote, act, id, write, view, desk }: { b: ProjectBoard; closing: boolean; setClosing: (on: boolean) => void; note: string; setNote: (note: string) => void; act: Act; id: string; write: boolean; view: BoardView; desk?: boolean }) {
  // The desk has its Needs you list on the right.
  const waiting = desk ? [] : needsYou(view);
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
  moveRow,
  open,
  opened,
  asks,
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
  /** The groups a row may move to, by its menu or by dragging; none for a reader who can't write. */
  groups: BoardGroup[];
  moveRow: (r: BoardRow, zone: string | null) => void;
  open: (row: string) => void;
  /** The row whose drawer is open: its card is the selected one. */
  opened: string | null;
  /** Each row's open ask is a card on it (the block's); the desk has them in its Needs you. */
  asks: boolean;
}) {
  const n = laneCount(z);
  /** Done shows its first rows, then "N more" (Lift 5a). */
  const [all, setAll] = useState(false);
  const items = useContext(RowItems);
  const rows = needsFirst(z.rows, (r) => !!items.get(r.id)?.mine);
  const cut = z.id === "lane_done" && !all && rows.length > DONE_SHOWN + 1 ? DONE_SHOWN : rows.length;
  // No lane takes a dropped card: a row's lane follows its work (Arugula #1383). A card dragged to another group drops on the group (#664).
  return (
    <section class="board-zone" data-board-zone={z.id} data-zone-kind={z.kind ?? undefined}>
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
        <ul class="board-rows">
          {rows.slice(0, cut).map((r) => (
            <BoardItemRow key={r.id} r={r} editing={editing === r.id} setEditing={setEditing} act={act} id={id} project={project} platform={platform} groups={groups} moveRow={moveRow} open={open} opened={opened} asks={asks} />
          ))}
          {cut < rows.length && (
            <li>
              <button type="button" class="link board-more-rows" data-board-lane-more={rows.length - cut} onClick={() => setAll(true)}>
                {rows.length - cut} more
              </button>
            </li>
          )}
        </ul>
      )}
    </section>
  );
}

/** A filter menu of the find bar (#547): its values with how many rows have each, several at once. */
function FilterMenu({ name, title, choices, picked, set }: { name: string; title: string; choices: { value: string; label?: string; count: number }[]; picked: string[]; set: (v: string[]) => void }) {
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
  const items: MenuItem[] = shown.map((c: { value: string; label?: string; count: number }) => ({
    label: c.label ?? c.value,
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

function BoardItemRow({
  r,
  project,
  editing,
  setEditing,
  act,
  id,
  platform,
  groups,
  moveRow,
  open,
  opened,
  asks,
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
  open: (row: string) => void;
  opened: string | null;
  asks: boolean;
}) {
  const phone = usePhone();
  const label = pointerLabel(r.pointer, r.live);
  const href = pointerHref(r, platform);
  const card = isCard(r.live);
  // Who works on it: the row's own live session first, else one the card's sessions list (Arugula #1082).
  const cardSessions = useRowSessions(r);
  const own = liveSessions(r, waitingOf(r), marksOf(r).stalled?.words)[0];
  const other = cardSessions.find((x) => x.needs) ?? cardSessions[0];
  const who = own && !(other?.needs && !own.needs) ? { name: own.name, word: own.word } : other ? { name: other.name, word: other.state ?? "" } : undefined;
  const item = useContext(RowItems).get(r.id);
  const st = tileState(r, Date.now(), !!own?.needs || !!other?.needs, item);
  const meta = tileMeta(r, who);
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
      "separator",
      { label: "Delete the row", danger: true, run: () => void act(() => boards.remove(id, r.id)) },
    ]);
  // The tile's empty space opens the drawer (Enter on the focused tile too); its links, buttons and forms do their own.
  const body = (e: MouseEvent) => {
    if (e.button !== 0 || (e.target as Element).closest("a, button, summary, input, textarea, select, form, details, [data-board-ask]")) return;
    if (window.getSelection()?.toString()) return;
    open(r.id);
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === "Enter" && e.target === e.currentTarget) (e.preventDefault(), open(r.id));
  };
  const title = card ? r.live!.title : r.note ? firstLine(r.note) : label || "Note";
  const external = r.pointer.kind === "forge_item" || r.pointer.kind === "link";
  return (
    <Surface
      kind="card"
      as="li"
      class="board-tile"
      data-board-row={r.id}
      data-lane={r.lane}
      data-status={r.status}
      data-tile={st.mark}
      data-needs-you={st.needs ? "" : undefined}
      current={opened === r.id}
      draggable={groups.length > 0 && !phone && !editing}
      onDragStart={(e: DragEvent) => {
        e.dataTransfer?.setData(ROW_DRAG, r.id);
        e.dataTransfer?.setData(dragGroupType(r.group ?? null), r.group ?? "none"); // a type with no data is dropped
      }}
      tabIndex={editing ? undefined : 0}
      onClick={editing ? undefined : body}
      onKeyDown={editing ? undefined : key}
    >
      {editing ? (
        <RowForm
          project={project}
          row={r}
          save={(f) => act(() => boards.put(id, { id: r.id, rev: r.rev, note: f.note, ...(f.pointer ? { pointer: f.pointer } : {}) }))}
          done={() => setEditing(null)}
        />
      ) : (
        <>
          <div class="board-tile-head">
            <Mark state={st.mark} />
            {label && (
              <span class="board-ptr m-id" data-board-ptr>
                {label}
              </span>
            )}
            {st.word && (
              <span class={st.needs ? "board-tile-word k-needs" : "board-tile-word m-meta"} data-board-tile-word>
                {st.word}
              </span>
            )}
            <IconButton label="Row actions" class="bar-btn sq board-more" data-board-row-menu onClick={more}>
              <MoreIcon size="sm" />
            </IconButton>
          </div>
          {href ? (
            <a class="board-tile-title" href={href} target={external ? "_blank" : undefined} rel="noopener noreferrer" data-board-title data-board-item-link>
              {title}
            </a>
          ) : (
            <span class="board-tile-title" data-board-title>
              {title}
            </span>
          )}
          {meta.length > 0 && (
            <p class="board-tile-meta m-meta" data-board-tile-meta>
              {meta.join(" · ")}
            </p>
          )}
          {asks && r.ask && !r.ask.raised && (
            <div data-board-ask={r.ask.id}>
              <NeedRow need={askNeed(r, r.ask, Date.now())} open onAnswer={(o) => void act(() => boards.answer(id, r.ask!.id, o.id ? { option: o.id } : { text: o.label }))} />
            </div>
          )}
          {asks && r.asked && r.asked.length > 0 && <Answered asked={r.asked} />}
        </>
      )}
    </Surface>
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
