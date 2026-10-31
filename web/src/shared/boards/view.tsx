// One board (boards, step 2; lanes, aru-125), wherever it's drawn: the
// platform page's `/p/:project/boards/:board`, and a board block in a daemon
// workspace (a pin onto it, `shell/blocks/board.tsx`). The four lanes in
// order (Next, Doing, Review, Done: nobody makes or moves them, the work
// does), each with its rows; a board whose author made groups (phases, waves)
// shows each as a section with its progress and the four lanes of its rows
// under it, the rows with no group last. A row's marks say what its lane
// doesn't: merged but not closed, waiting on another PR, earlier agents
// folded away. Each row has: a glyph for the row's lane, a link to what it
// points at with that item's state now (`live`), chips for what is attached
// to it, on a Done row the mark that says it is proven or not yet, and a
// note. Owners and editors add, edit and delete rows, and make, rename, move
// and delete groups; a viewer's writes are refused and say so. A row is a
// swarm card (platform/swarm.css, as the Issues page's): its bar is its
// lane's tone, and its title opens the drawer, a read-only side sheet with the
// item's body, comments, checks, attachments and proof (#369).
//
// It reads the board as the person looking: now, again when the platform
// says it changed (the board's thread, chat's live stream), and when the
// page comes back into view. A write on an entry that moved on is refused
// (409): the view reads the board again and says so, and the change is theirs
// to make again.

import { useEffect, useId, useRef, useState } from "preact/hooks";
import type { RelationKind, BoardAsk, BoardAnsweredAsk, BoardAttachment, BoardGroup, BoardPointer, ForgeText, ProjectBoard, BoardRow, BoardView, BoardWorkItem, BoardZone } from "../proto";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { Markdown } from "../ui/markdown";
import { ConfirmRemove } from "../ui/confirm";
import { StateChip } from "../ui/state-chip";
import { useSubscribe } from "../ui/hooks";
import { CopyLinkButton } from "../ui/copy-link";
import { askText, openMenu, type MenuItem } from "../ui/menu";
import { issues } from "../issues/store";
import { whoLabel } from "../issues/text";
import { agentShort } from "../agent-label";
import { day } from "../time";
import { BoardError, STALE_REV, boards, type ForgeChecks } from "./store";
import "./boards.css";
import { PICK_DELAY_MS, cardTone, checkTone, checkWord, groupMove, liveTone, prsOf, PICK_LIMIT, assigneesLine, blockClass, GOAL_MAX, answerReady, askWaitingLine, blockFacts, chipOf, settledFacts, debounced, earlierLine, epicLine, holdsRow, groupLine, isCard, laneCount, liveClass, liveWord, marksOf, needsYou, pointerHref, pointerLabel, pointerText, pointerToSend, proofMark, readyCount, relationLine, RELATION_KINDS, sections, stepActive, workLabel } from "./text";

const STALE_SAID = "That changed while you were editing, so the board is read again. Make your change again if it still applies.";

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
  menu,
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
  /** The Copy link button's ▾ items, given the board's team link (the block adds "Open in browser ↗"). */
  menu?: (link: string) => MenuItem[];
}) {
  const { toast } = usePlatformApi();
  const [view, setView] = useState<BoardView | null>(null);
  const [missing, setMissing] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  /** The row whose drawer is open. */
  const [drawer, setDrawer] = useState<string | null>(null);
  /** The group a Delete waits on. */
  const [dropping, setDropping] = useState<BoardGroup | null>(null);
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

  // A link from a block or a session names a row, `#row=row_…` (#372).
  const rowKey = view ? location.hash.match(/^#row=(row_[0-9a-f]+)/)?.[1] : undefined;
  useEffect(() => {
    if (rowKey) document.querySelector(`[data-board-row="${rowKey}"]`)?.scrollIntoView({ block: "center" });
  }, [rowKey, !!view]);

  if (missing && !view) return <p class="board-none dim" data-board-missing>That board isn't one you can read ({missing}).</p>;
  if (!view) return <p class="board-none dim">Loading…</p>;

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
  const opened = drawer ? view.zones.flatMap((z) => z.rows).find((r) => r.id === drawer) : undefined;

  return (
    <div class="board" data-board={id}>
      {back}
      <div class="board-head">
      <h1 class="board-title" data-board-title>
        {write ? (
          <span role="button" tabIndex={0} class="board-rename" title="Rename the board" data-board-rename onClick={() => void rename()} onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), void rename())}>
            {view.board.title}
          </span>
        ) : (
          view.board.title
        )}
      </h1>
      <CopyLinkButton link={shareLink} menu={shareLink ? menu?.(shareLink) : undefined} />
      </div>
      <Epic b={view.board} act={act} id={id} write={write} view={view} />
      {stale && (
        <p class="board-stale" role="status" data-board-stale>
          {STALE_SAID}
        </p>
      )}
      {sections(view).map((s) => {
        const lanes = (
          <div class="board-lanes">
            {s.lanes.map((z) => (
              <Lane key={z.id} z={z} project={view.board.project} editing={editing} setEditing={setEditing} act={act} id={id} platform={platform} groups={write ? (view.groups ?? []) : []} moveRow={moveRow} open={setDrawer} />
            ))}
          </div>
        );
        // No groups: the lanes alone, as before.
        if (!view.groups?.length) return lanes;
        return (
          <section class="board-group" key={s.group?.id ?? "none"} data-board-group={s.group?.id ?? "none"}>
            <header class="board-group-head">
              <h2 data-board-group-title>{s.group?.title ?? "No group"}</h2>
              {s.group && (
                <span class="board-group-progress dim" data-board-group-progress>
                  {groupLine(s.group.progress)}
                </span>
              )}
              {s.group && write && (
                <button type="button" class="board-more" aria-label={`Group actions: ${s.group.title}`} data-board-group-menu={s.group.id} onClick={(e) => groupMenu(e, s.group!)}>
                  ⋯
                </button>
              )}
            </header>
            {lanes}
          </section>
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
          <button type="button" class="board-add" data-board-add-row onClick={() => setEditing("new")}>
            + Add row
          </button>
          {write && (
            <button type="button" class="board-add" data-board-add-group onClick={() => void addGroup()}>
              + Add a group
            </button>
          )}
          <button type="button" class="board-add" data-board-new-issue onClick={() => void newIssue()}>
            + New issue
          </button>
          {plane.map((c) => (
            <button key={c.id} type="button" class="board-add" data-board-new-plane-issue={c.identifier} onClick={() => void newPlaneIssue(c)}>
              + New issue in Plane ({c.identifier})
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
      {opened && <Drawer r={opened} platform={platform} close={() => setDrawer(null)} project={view.board.project} others={view.zones.flatMap((z) => z.rows)} write={write} act={act} id={id} />}
    </div>
  );
}

/** A board's goal, progress and closing, under its title. The panel doesn't know whether the person may write (the pin block has no project), so without `write` the controls still show and the platform's refusal comes back as `act`'s toast. */
function Epic({ b, act, id, write, view }: { b: ProjectBoard; act: Act; id: string; write: boolean; view: BoardView }) {
  const [goalOpen, setGoalOpen] = useState(false);
  const [goal, setGoal] = useState("");
  const [closing, setClosing] = useState(false);
  const [note, setNote] = useState("");
  const e = epicLine(b.progress);
  const waiting = needsYou(view);
  const { teams } = usePlatformApi();
  const nameOf = (account: string) => teams().flatMap((t) => t.roster.members).find((m) => m.account === account)?.name;
  const saveGoal = async () => {
    if (await act(() => boards.edit(id, { goal: goal.trim() }))) setGoalOpen(false);
  };
  const close = async () => {
    if (await act(() => boards.close(id, note.trim()))) setClosing(false);
  };
  return (
    <div class="board-epic">
      {goalOpen ? (
        <div class="board-form" data-board-goal-form>
          <textarea value={goal} rows={4} maxLength={GOAL_MAX} aria-label="Goal" data-board-goal-input autoFocus onInput={(ev) => setGoal(ev.currentTarget.value)} />
          <span class="board-form-buttons">
            <button type="button" class="primary" data-board-goal-save onClick={() => void saveGoal()}>
              Save
            </button>
            <button type="button" onClick={() => setGoalOpen(false)}>
              Cancel
            </button>
          </span>
        </div>
      ) : (
        <>
          {b.goal && (
            <p class="board-goal" data-board-goal>
              {b.goal}
            </p>
          )}
          {write && (
            <p class="board-epic-actions">
              <button type="button" data-board-goal-edit onClick={() => (setGoal(b.goal), setGoalOpen(true))}>
                {b.goal ? "Edit goal" : "Add a goal"}
              </button>
            </p>
          )}
        </>
      )}
      {b.created_by_agent && (
        <p class="dim" data-board-made-by>
          Made by {whoLabel(b.created_by, b.created_by_agent, nameOf)}
        </p>
      )}
      <div class="sw-stats board-stats" data-board-stats>
        <span>
          <b>{b.progress.done}</b> done
        </span>
        <span>
          <b>{b.progress.proven}</b> proven
        </span>
        <span data-board-ready={readyCount(view)}>
          <b>{readyCount(view)}</b> ready
        </span>
        {waiting.length > 0 && (
          <button type="button" class="needs board-needs-you-button" data-board-needs-you={waiting.length} onClick={() => document.querySelector(`[data-board-row="${waiting[0].id}"]`)?.scrollIntoView({ behavior: "smooth", block: "center" })}>
            <b>{waiting.length}</b> Needs you
          </button>
        )}
      </div>
      <div class="board-progress" data-board-progress>
        {b.progress.total > 0 && (
          <div class="board-bar" role="progressbar" aria-valuemin={0} aria-valuemax={b.progress.total} aria-valuenow={b.progress.done} aria-label={e.words}>
            <i style={{ width: `${e.pct}%` }} />
          </div>
        )}
        <span class="dim">{e.words}</span>
      </div>
      {b.closed_at != null ? (
        <div class="board-done" data-board-done>
          <p>
            <span class="ws-tag board-done-tag">Done</span> <span class="dim">{day(b.closed_at)}</span>
            {b.closed_by_agent && <span class="dim" data-board-done-by> by {whoLabel(b.closed_by, b.closed_by_agent, nameOf)}</span>}
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
            <button type="button" class="primary" data-board-close-save disabled={!note.trim()} onClick={() => void close()}>
              Close epic
            </button>
            <button type="button" onClick={() => setClosing(false)}>
              Cancel
            </button>
          </span>
        </div>
      ) : write ? (
        <p class="board-epic-actions">
          <button type="button" data-board-close onClick={() => (setNote(""), setClosing(true))}>
            Close epic…
          </button>
        </p>
      ) : null}
    </div>
  );
}

type Act = (write: () => Promise<BoardView>) => Promise<boolean>;

/** A row's open question: the question, its why folded, the options as buttons with the recommendation marked, and a text box when there are none. Only a person who may answer gets the buttons; a raised ask has none to give. */
function AskCard({ a, answer }: { a: BoardAsk; answer: (a: { option: string } | { text: string }) => Promise<boolean> }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const send = async (a: { option: string } | { text: string }) => {
    setBusy(true);
    await answer(a);
    setBusy(false);
  };
  return (
    <div class={`board-ask${a.raised ? " raised" : ""}${a.mine ? " mine" : ""}`} data-board-ask={a.raised ? "raised" : a.id}>
      <div class="board-line board-ask-question">
        <span class="ws-tag waits">{a.raised ? "Board" : "Asks"}</span>
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
            <button key={o} type="button" class={`board-ask-option${o === a.recommend ? " recommended" : ""}`} disabled={busy} data-board-ask-option={o} title={o === a.recommend ? "Recommended" : undefined} onClick={() => void send({ option: o })}>
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

function Lane({
  z,
  project,
  editing,
  setEditing,
  act,
  id,
  platform,
  groups,
  moveRow,
  open,
}: {
  z: BoardZone;
  project: string;
  editing: string | null;
  setEditing: (e: string | null) => void;
  act: Act;
  id: string;
  platform: (path: string) => string | undefined;
  /** The groups a row may move to; none for a reader who can't write. */
  groups: BoardGroup[];
  moveRow: (r: BoardRow, zone: string | null) => void;
  open: (row: string) => void;
}) {
  const n = laneCount(z);
  return (
    <section class="board-zone" data-board-zone={z.id} data-zone-kind={z.kind ?? undefined}>
      <header class="board-zone-head">
        <h2>{z.title}</h2>
        <span class="board-count dim" data-board-count>
          {n.total}
          {n.proven > 0 && ` · ${n.proven} proven`}
        </span>
      </header>
      <ul class="board-rows sw-cards">
        {z.rows.map((r) => (
          <Row key={r.id} r={r} editing={editing === r.id} setEditing={setEditing} act={act} id={id} project={project} platform={platform} groups={groups} moveRow={moveRow} open={open} />
        ))}
      </ul>
    </section>
  );
}

/** An attachment as a chip (`sw-chip`): what it points at, and its state. An agent block is one chip, its title and its state (no second status line), a link to its pane where the reader can reach it: control's page on the platform page, the pane itself in the shell. `facts` says its branch and the rest after the chip, dim: `settled`, once it has stopped; `all`, in the drawer. */
function Chip({ a, platform, facts = "settled" }: { a: BoardAttachment; platform: (path: string) => string | undefined; facts?: "settled" | "all" }) {
  const api = usePlatformApi();
  const c = chipOf(a);
  const p = a.pointer;
  const href = pointerHref({ pointer: p, live: a.live }, platform);
  // The pane: a link to control's page here; in the shell, when the machine is one of the reader's.
  const pane = p.kind === "block" ? (api.controlHref ? { href: api.controlHref(`pane=${p.machine}.${p.pane}`) } : api.machine(p.machine) ? { go: () => api.goToPane(p.machine, p.pane, null) } : null) : null;
  const name = pane?.href ? (
    <a class="board-ptr" href={pane.href} target="_blank" rel="noopener noreferrer" title="Open its pane" data-board-block-open>
      {c.label}
    </a>
  ) : pane?.go ? (
    <a class="board-ptr" href="#" title="Open its pane" data-board-block-open onClick={(e) => (e.preventDefault(), pane.go!())}>
      {c.label}
    </a>
  ) : href ? (
    <a class="board-ptr" href={href} target={p.kind === "forge_item" || p.kind === "link" ? "_blank" : undefined} rel="noopener noreferrer">
      {c.label}
    </a>
  ) : (
    <span class="board-ptr">{c.label}</span>
  );
  const more = a.block ? (facts === "all" ? blockFacts(a.block) : settledFacts(a.block)) : [];
  return (
    <>
      <span class={`sw-chip board-chip ${a.role}`} data-board-attachment={a.id} data-attach-role={a.role} data-board-block={a.block?.state}>
        {a.role === "proof" && <span class="dim">proof </span>}
        {name}
        {c.state && <span class={`ws-tag board-live ${a.live ? liveClass(a.live.state) : a.block ? blockClass(a.block.state) : ""}`}>{c.state}</span>}
      </span>
      {more.length > 0 && (
        <span class="board-fact dim" data-board-block-facts>
          {more.join(" · ")}
        </span>
      )}
    </>
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
  open,
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
}) {
  const label = pointerLabel(r.pointer, r.live);
  const href = pointerHref(r, platform);
  const card = isCard(r.live);
  const who = card ? assigneesLine(r.live!.assignees) : "";
  const mark = proofMark(r);
  const marks = marksOf(r);
  const earlier = r.attachments.filter((a) => marks.earlier.has(a.id));
  const current = r.attachments.filter((a) => !marks.earlier.has(a.id));
  const tone = cardTone(r);
  const proofTag = mark && (
    <span class={`ws-tag board-proof ${mark.proven ? "proven" : "unproven"}`} data-board-proof={mark.proven ? (r.proof?.by ?? "proven") : "not-proven"}>
      {mark.words}
    </span>
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
    <li class={`board-card sw-card sw-${tone}`} data-board-row={r.id} data-lane={r.lane} data-status={r.status} data-tone={tone} data-needs-you={r.ask?.mine ? "" : undefined} tabIndex={editing ? undefined : 0} onClick={editing ? undefined : body} onKeyDown={editing ? undefined : key}>
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
              {href ? (
                <a class="board-card-name board-title" href={href} target={external ? "_blank" : undefined} rel="noopener noreferrer" data-board-title data-board-item-link>
                  {title}
                </a>
              ) : (
                <span class="board-card-name board-title" data-board-title>
                  {title}
                </span>
              )}
              {r.live && (
                <StateChip tone={liveTone(r.live.state)} data-board-live={r.live.state}>
                  {liveWord(r.live)}
                </StateChip>
              )}
            </div>
            <button type="button" class="board-more" aria-label="Row actions" data-board-row-menu onClick={more}>
              ⋯
            </button>
          </div>
          <div class="board-line board-sub dim">
            {label && <span class="board-ptr" data-board-ptr>{label}</span>}
            {card && who && (
              <span class="board-who dim" data-board-who>
                {who}
              </span>
            )}
            {r.agent && (
              <span class="board-agent dim" data-row-agent>
                {agentShort(r.agent)}
              </span>
            )}
            {proofTag}
          </div>
          {(marks.merged || marks.cancelled || marks.after || marks.stalled || marks.blocked.length > 0) && (
            <div class="board-line board-marks" data-board-marks>
              {marks.merged && (
                <span class="ws-tag board-mark" data-board-mark="merged_not_closed" title="A PR of this row merged, but its issue is still open">
                  merged, not closed
                </span>
              )}
              {marks.cancelled && (
                <span class="ws-tag board-mark" data-board-mark="cancelled" title="This item was canceled, so the row is done and not proven">
                  cancelled
                </span>
              )}
              {marks.after && (
                <span class="ws-tag waits board-mark" data-board-mark="after" title={marks.after.repo ? `Stacked on a PR of ${marks.after.repo}` : "Stacked on another PR"}>
                  {marks.after.words}
                </span>
              )}
              {marks.stalled && (
                <span class="ws-tag warn board-mark" data-board-mark="stalled" title="Every agent on this row has been idle past the project's limit, and no pull request is open">
                  {marks.stalled.words}
                </span>
              )}
              {marks.blocked.map((m, i) => (
                <span key={i} class="ws-tag waits board-mark" data-board-mark="blocked_by" title={m.title ? `${m.title}${m.repo ? ` (${m.repo})` : ""}` : "You can't read the item that blocks this one"}>
                  {m.words}
                </span>
              ))}
            </div>
          )}
          {current.length > 0 && (
            <div class="board-line board-chips" data-board-attachments>
              {current.map((a) => (
                <Chip key={a.id} a={a} platform={platform} />
              ))}
            </div>
          )}
          {earlier.length > 0 && (
            <details class="board-earlier" data-board-earlier={earlier.length}>
              <summary class="dim">{earlierLine(earlier.length)}</summary>
              <div class="board-line board-chips">
                {earlier.map((a) => (
                  <Chip key={a.id} a={a} platform={platform} />
                ))}
              </div>
            </details>
          )}
          {r.note && (
            <div class="board-note dim" data-board-note>
              <Markdown text={r.note} refs={null} />
            </div>
          )}
          {r.ask && <AskCard a={r.ask} answer={(a) => act(() => boards.answer(id, r.ask!.id, a))} />}
          {r.asked && r.asked.length > 0 && <Answered asked={r.asked} />}
        </div>
      )}
    </li>
  );
}

/** What the drawer read of a row's item. `body` null: the reader may not read it here. */
interface Read {
  body: string | null;
  comments: { who: string; text: string; at: number }[];
  checks: { label: string; checks: ForgeChecks["checks"] }[];
}

/** The most comments the drawer shows. */
const DRAWER_COMMENTS = 3;

const commentOf = (t: ForgeText) => ({ who: t.author ?? "", text: t.body, at: t.created_at });

/** A row's item as the drawer reads it, with the APIs the pages use: a native issue's body and thread, a mirrored item's content, the checks of each PR behind the row. Any read that fails leaves its part out. */
async function readRow(r: BoardRow): Promise<Read> {
  const out: Read = { body: null, comments: [], checks: [] };
  const p = r.pointer;
  const reads: Promise<void>[] = [];
  if (p.kind === "issue") {
    reads.push(boards.issue(p.id).then((i) => void (out.body = i.body)).catch(() => {}));
    reads.push(
      chat
        .openObject(p.id)
        .then(async (room) => {
          await chat.load(room.id);
          out.comments = (chat.messages(room.id)?.list ?? []).filter((m) => m.text).slice(-DRAWER_COMMENTS).map((m) => ({ who: m.name, text: m.text, at: m.at }));
        })
        .catch(() => {}),
    );
  } else if (p.kind === "forge_item" && r.live?.provider !== "plane") {
    reads.push(
      boards
        .content(p.id)
        .then((c) => {
          out.body = c.body?.body ?? "";
          out.comments = c.comments.filter((t) => t.kind === "comment" || !t.path).slice(-DRAWER_COMMENTS).map(commentOf);
        })
        .catch(() => {}),
    );
  }
  const prs = prsOf(r).slice(0, 3);
  const checks = await Promise.all(prs.map((x) => boards.checks(x.id).then((c) => ({ label: x.label, checks: c.checks })).catch(() => null)));
  await Promise.all(reads);
  out.checks = checks.flatMap((c) => (c ? [c] : []));
  return out;
}

/** The drawer: a row's item, read-only, beside the board (a sheet on a phone). Its title, state and link out; the body, the latest comments, the PRs' checks, the attachments and the proof's evidence. */
function Drawer({ r, platform, close, project, others, write, act, id }: { r: BoardRow; platform: (path: string) => string | undefined; close: () => void; project: string; others: BoardRow[]; write: boolean; act: (write: () => Promise<BoardView>) => Promise<boolean>; id: string }) {
  const [read, setRead] = useState<Read | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const href = pointerHref(r, platform);
  const key = `${r.id}:${r.updated_at}`;
  useEffect(() => {
    setRead(null);
    let live = true;
    void readRow(r).then((v) => live && setRead(v));
    return () => void (live = false);
  }, [key]);
  useEffect(() => {
    ref.current?.focus({ preventScroll: true });
    const esc = (e: KeyboardEvent) => e.key === "Escape" && close();
    document.addEventListener("keydown", esc);
    return () => document.removeEventListener("keydown", esc);
  }, []);
  const title = isCard(r.live) ? r.live.title : pointerLabel(r.pointer, r.live) || "Note";
  const evidence = r.attachments.filter((a) => a.role === "proof");
  const readable = r.pointer.kind === "issue" || (r.pointer.kind === "forge_item" && r.live?.provider !== "plane");
  const external = r.pointer.kind === "forge_item" || r.pointer.kind === "link";
  return (
    <div ref={ref} class="board-drawer" role="dialog" aria-label={title} tabIndex={-1} data-board-drawer={r.id}>
      <header class="board-drawer-head">
        <h2 class="board-drawer-title" data-board-drawer-title>
          {title}
        </h2>
        <button type="button" class="board-more" aria-label="Close" data-board-drawer-close onClick={close}>
          ×
        </button>
      </header>
      <div class="board-line">
        {r.live && <StateChip tone={liveTone(r.live.state)}>{liveWord(r.live)}</StateChip>}
        {href && (
          <a class="board-ptr" href={href} target={external ? "_blank" : undefined} rel="noopener noreferrer" data-board-drawer-link>
            {pointerLabel(r.pointer, r.live)} ↗
          </a>
        )}
      </div>
      {r.note && <p class="dim">{r.note}</p>}
      {readable && (
        <section data-board-drawer-body>
          {!read ? (
            <p class="dim">Loading…</p>
          ) : read.body === null ? (
            <p class="dim">{href ? <a href={href} target="_blank" rel="noopener noreferrer">Open on GitHub</a> : "Not available here."}</p>
          ) : read.body.trim() ? (
            <Markdown text={read.body} refs={null} />
          ) : (
            <p class="dim">No description.</p>
          )}
        </section>
      )}
      {read && read.comments.length > 0 && (
        <section data-board-drawer-comments>
          <h3>Latest comments</h3>
          {read.comments.map((c, i) => (
            <div key={i} class="board-drawer-comment">
              <div class="dim">
                {c.who} · {day(c.at)}
              </div>
              <Markdown text={c.text} refs={null} />
            </div>
          ))}
        </section>
      )}
      {read && read.checks.length > 0 && (
        <section data-board-drawer-checks>
          <h3>Checks</h3>
          {read.checks.map((c) => (
            <div key={c.label}>
              {read.checks.length > 1 && <div class="dim">{c.label}</div>}
              {c.checks.length === 0 && <p class="dim">No checks reported.</p>}
              <ul class="board-checks">
                {c.checks.map((k) => (
                  <li key={k.name} data-board-check={k.name}>
                    <CheckChip tone={checkTone(k)} word={checkWord(k)} />
                    {k.url && /^https:\/\//.test(k.url) ? (
                      <a href={k.url} target="_blank" rel="noopener noreferrer">
                        {k.name}
                      </a>
                    ) : (
                      k.name
                    )}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </section>
      )}
      {(r.relations?.length || (write && itemId(r))) && <Relations r={r} project={project} others={others} write={write} act={act} id={id} />}
      {r.attachments.length > 0 && (
        <section data-board-drawer-attachments>
          <h3>Attached</h3>
          {r.attachments.map((a) => (
            <div key={a.id} class="board-drawer-attached">
              <Chip a={a} platform={platform} facts="all" />
              {write && holdsRow(a) && (
                <button type="button" class="secondary small" data-board-release={a.id} title="Take this agent off the row; it keeps running, and another may take the row" onClick={() => void act(() => boards.detach(id, a.id))}>
                  Release
                </button>
              )}
            </div>
          ))}
        </section>
      )}
      {(r.proof || evidence.length > 0) && (
        <section data-board-drawer-proof>
          <h3>Proof</h3>
          {r.proof && (
            <p>
              {r.proof.by === "ci" ? "Proven by the merged PR, with its checks green." : "Proven by evidence."}
            </p>
          )}
          {evidence.map((a) => (
            <Chip key={a.id} a={a} platform={platform} />
          ))}
        </section>
      )}
    </div>
  );
}

/** The id of the issue or mirrored item a row is on, if it is on one. */
const itemId = (r: BoardRow): string | undefined => (r.pointer.kind === "issue" || r.pointer.kind === "forge_item" ? r.pointer.id : undefined);

/** A row's item's relations both ways (#403), with add and remove for those who may write. The item at the other end is picked among the board's other rows. A relation Plane or GitHub keeps can't be removed here. */
function Relations({ r, project, others, write, act, id }: { r: BoardRow; project: string; others: BoardRow[]; write: boolean; act: (write: () => Promise<BoardView>) => Promise<boolean>; id: string }) {
  const [kind, setKind] = useState<RelationKind>("blocks");
  const [dir, setDir] = useState<"out" | "in">("out");
  const [to, setTo] = useState("");
  const mine = itemId(r);
  const words = RELATION_KINDS.find((k) => k.kind === kind)!;
  const choices = others.filter((o) => o.id !== r.id && itemId(o));
  /** A relation write, then the board as it is. */
  const then = (write: () => Promise<unknown>) => async () => {
    await write();
    return boards.get(id);
  };
  const add = async () => {
    const other = choices.find((o) => o.id === to);
    if (!mine || !other) return;
    const [from, target] = dir === "out" ? [mine, itemId(other)!] : [itemId(other)!, mine];
    if (await act(then(() => boards.relate(project, { from, to: target, kind })))) setTo("");
  };
  return (
    <section data-board-drawer-relations>
      <h3>Relations</h3>
      {(r.relations ?? []).map((x) => (
        <div key={x.id} class="board-drawer-relation" data-board-relation={x.kind} data-board-relation-source={x.source}>
          <span>{relationLine(x)}</span>
          {x.source !== "person" && x.source !== "agent" && <span class="dim"> · from {x.source === "plane" ? "Plane" : "GitHub"}</span>}
          {x.removable && (
            <button type="button" class="board-more" aria-label="Remove relation" data-board-relation-remove onClick={() => void act(then(() => boards.unrelate(project, x.id)))}>
              ×
            </button>
          )}
        </div>
      ))}
      {write && mine && choices.length > 0 && (
        <div class="board-line board-relation-add" data-board-relation-add>
          <select aria-label="This item" value={dir} onChange={(e) => setDir(e.currentTarget.value as "out" | "in")}>
            <option value="out">{words.out}</option>
            {words.in !== words.out && <option value="in">{words.in}</option>}
          </select>
          <select aria-label="Kind" value={kind} onChange={(e) => setKind(e.currentTarget.value as RelationKind)}>
            {RELATION_KINDS.map((k) => (
              <option key={k.kind} value={k.kind}>
                {k.kind.replace("_", " ")}
              </option>
            ))}
          </select>
          <select aria-label="Other item" value={to} onChange={(e) => setTo(e.currentTarget.value)}>
            <option value="">Pick a row…</option>
            {choices.map((o) => (
              <option key={o.id} value={o.id}>
                {isCard(o.live) ? `#${o.live.number} ${o.live.title}` : pointerLabel(o.pointer, o.live)}
              </option>
            ))}
          </select>
          <button type="button" class="btn" disabled={!to} data-board-relation-submit onClick={() => void add()}>
            Add
          </button>
        </div>
      )}
    </section>
  );
}

/** A check as a chip. */
function CheckChip({ tone, word }: { tone: Parameters<typeof StateChip>[0]["tone"]; word: string }) {
  return <StateChip tone={tone}>{word}</StateChip>;
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
              <span class="board-pick-state dim">{w.state}</span>
            </li>
          ))}
        </ul>
      </div>
      {bad && <span class="board-bad">Expected an id that starts iss_ or fi_, or an https:// link. A run or an agent block is attached to a row.</span>}
      <span class="board-form-buttons">
        <button type="submit" class="primary" data-board-save>
          Save
        </button>
        <button type="button" onClick={done}>
          Cancel
        </button>
      </span>
    </form>
  );
}
