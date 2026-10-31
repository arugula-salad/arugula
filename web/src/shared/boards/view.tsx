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
// note. Owners and editors add, edit and delete rows; a viewer's writes are
// refused and say so. (The real page is B5's.)
//
// It reads the board as the person looking: now, again when the platform
// says it changed (the board's thread, chat's live stream), and when the
// page comes back into view. A write on an entry that moved on is refused
// (409): the view reads the board again and says so, and the change is theirs
// to make again.

import { useEffect, useId, useRef, useState } from "preact/hooks";
import type { BoardAsk, BoardAnsweredAsk, BoardAttachment, BoardPointer, ProjectBoard, BoardRow, BoardView, BoardWorkItem, BoardZone } from "../proto";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { Markdown } from "../ui/markdown";
import { useSubscribe } from "../ui/hooks";
import { askText, openMenu, type MenuItem } from "../ui/menu";
import { issues } from "../issues/store";
import { whoLabel } from "../issues/text";
import { agentShort } from "../agent-label";
import { day } from "../time";
import { BoardError, STALE_REV, boards } from "./store";
import "./boards.css";
import { BLOCK_LABEL, PICK_DELAY_MS, PICK_LIMIT, assigneesLine, blockClass, GOAL_MAX, answerReady, askWaitingLine, blockFacts, chipOf, debounced, earlierLine, epicLine, groupLine, isCard, laneCount, liveClass, liveWord, marksOf, needsYou, needsYouLine, pointerHref, pointerLabel, pointerText, pointerToSend, proofMark, rowGlyph, sections, stepActive, workLabel } from "./text";

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
}) {
  const { toast } = usePlatformApi();
  const [view, setView] = useState<BoardView | null>(null);
  const [missing, setMissing] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
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

  return (
    <div class="board" data-board={id}>
      {back}
      <h1 class="board-title" data-board-title>
        {write ? (
          <span role="button" tabIndex={0} class="board-rename" title="Rename the board" data-board-rename onClick={() => void rename()} onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), void rename())}>
            {view.board.title}
          </span>
        ) : (
          view.board.title
        )}
      </h1>
      <Epic b={view.board} act={act} id={id} write={write} />
      <NeedsYou view={view} />
      {stale && (
        <p class="board-stale" role="status" data-board-stale>
          {STALE_SAID}
        </p>
      )}
      {sections(view).map((s) => {
        const lanes = s.lanes.map((z) => <Lane key={z.id} z={z} project={view.board.project} editing={editing} setEditing={setEditing} act={act} id={id} platform={platform} />);
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
    </div>
  );
}

/** A board's goal, progress and closing, under its title. The panel doesn't know whether the person may write (the pin block has no project), so without `write` the controls still show and the platform's refusal comes back as `act`'s toast. */
function Epic({ b, act, id, write }: { b: ProjectBoard; act: Act; id: string; write: boolean }) {
  const [goalOpen, setGoalOpen] = useState(false);
  const [goal, setGoal] = useState("");
  const [closing, setClosing] = useState(false);
  const [note, setNote] = useState("");
  const e = epicLine(b.progress);
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

/** "Needs you (N)": the asks that name the reader, and those the board raised on rows they can see. Clicking scrolls to the first. */
function NeedsYou({ view }: { view: BoardView }) {
  const rows = needsYou(view);
  if (!rows.length) return null;
  const go = () => {
    const el = document.querySelector(`[data-board-row="${rows[0].id}"]`);
    if (el) (el as HTMLElement).scrollIntoView({ behavior: "smooth", block: "center" });
  };
  return (
    <p class="board-needs-you" data-board-needs-you={rows.length}>
      <button type="button" class="board-needs-you-button" onClick={go}>
        {needsYouLine(rows.length)}
      </button>
    </p>
  );
}

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
}: {
  z: BoardZone;
  project: string;
  editing: string | null;
  setEditing: (e: string | null) => void;
  act: Act;
  id: string;
  platform: (path: string) => string | undefined;
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
      <ul class="board-rows">
        {z.rows.map((r) => (
          <Row key={r.id} r={r} editing={editing === r.id} setEditing={setEditing} act={act} id={id} project={project} platform={platform} />
        ))}
      </ul>
    </section>
  );
}

function Chip({ a, platform }: { a: BoardAttachment; platform: (path: string) => string | undefined }) {
  const c = chipOf(a);
  const href = pointerHref({ pointer: a.pointer, live: a.live }, platform);
  return (
    <span class={`board-chip ${a.role}`} data-board-attachment={a.id} data-attach-role={a.role}>
      {a.role === "proof" && <span class="dim">proof </span>}
      {href ? (
        <a class="board-ptr" href={href} target={a.pointer.kind === "forge_item" || a.pointer.kind === "link" ? "_blank" : undefined} rel="noopener noreferrer">
          {c.label}
        </a>
      ) : (
        <span class="board-ptr">{c.label}</span>
      )}
      {c.state && <span class={`ws-tag board-live ${a.live ? liveClass(a.live.state) : a.block ? blockClass(a.block.state) : ""}`}>{c.state}</span>}
    </span>
  );
}

/** What a machine reported of an attached agent block. */
function BlockLine({ a }: { a: BoardAttachment }) {
  const b = a.block!;
  return (
    <div class="board-line board-block" data-board-block={b.state}>
      <span class={`ws-tag ${blockClass(b.state)}`}>{BLOCK_LABEL[b.state]}</span>
      {blockFacts(b).map((f) => (
        <span class="board-fact dim" key={f}>{f}</span>
      ))}
    </div>
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
}: {
  r: BoardRow;
  editing: boolean;
  setEditing: (e: string | null) => void;
  act: Act;
  id: string;
  project: string;
  platform: (path: string) => string | undefined;
}) {
  const label = pointerLabel(r.pointer, r.live);
  const href = pointerHref(r, platform);
  const card = isCard(r.live);
  const who = card ? assigneesLine(r.live!.assignees) : "";
  const mark = proofMark(r);
  const marks = marksOf(r);
  const earlier = r.attachments.filter((a) => marks.earlier.has(a.id));
  const current = r.attachments.filter((a) => !marks.earlier.has(a.id));
  const proofTag = mark && (
    <span class={`ws-tag board-proof ${mark.proven ? "proven" : "unproven"}`} data-board-proof={mark.proven ? (r.proof?.by ?? "proven") : "not-proven"}>
      {mark.words}
    </span>
  );
  const more = (e: MouseEvent) =>
    menuAt(e, [
      { label: "Edit…", run: () => setEditing(r.id) },
      "separator",
      { label: "Delete the row", danger: true, run: () => void act(() => boards.remove(id, r.id)) },
    ]);
  return (
    <li class="board-row" data-board-row={r.id} data-lane={r.lane} data-status={r.status}>
      <span class={`board-glyph s-${r.status}`} title={r.status === "needs_input" ? "Needs input" : r.lane} data-board-glyph>
        {rowGlyph(r)}
      </span>
      {editing ? (
        <RowForm
          project={project}
          row={r}
          save={(f) => act(() => boards.put(id, { id: r.id, rev: r.rev, note: f.note, ...(f.pointer ? { pointer: f.pointer } : {}) }))}
          done={() => setEditing(null)}
        />
      ) : (
        <div class="board-main">
          {card && (
            <div class="board-line board-card-title" data-board-title>
              <span class="board-title">{r.live!.title}</span>
              {proofTag}
            </div>
          )}
          <div class={card ? "board-line board-sub" : "board-line"}>
            {label && (href ? <a class="board-ptr" href={href} target={r.pointer.kind === "forge_item" || r.pointer.kind === "link" ? "_blank" : undefined} rel="noopener noreferrer" data-board-ptr>{label}</a> : <span class="board-ptr" data-board-ptr>{label}</span>)}
            {r.live && (
              <span class={`ws-tag board-live ${liveClass(r.live.state)}`} data-board-live={r.live.state}>
                {liveWord(r.live)}
              </span>
            )}
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
            {r.live && !card && <span class="board-item dim">{r.live.title}</span>}
            {!card && proofTag}
          </div>
          {(marks.merged || marks.after) && (
            <div class="board-line board-marks" data-board-marks>
              {marks.merged && (
                <span class="ws-tag board-mark" data-board-mark="merged_not_closed" title="A PR of this row merged, but its issue is still open">
                  merged, not closed
                </span>
              )}
              {marks.after && (
                <span class="ws-tag waits board-mark" data-board-mark="after" title={marks.after.repo ? `Stacked on a PR of ${marks.after.repo}` : "Stacked on another PR"}>
                  {marks.after.words}
                </span>
              )}
            </div>
          )}
          {current.length > 0 && (
            <div class="board-line board-chips" data-board-attachments>
              {current.map((a) => (
                <Chip key={a.id} a={a} platform={platform} />
              ))}
            </div>
          )}
          {current.map((a) => (a.block ? <BlockLine key={a.id} a={a} /> : null))}
          {earlier.length > 0 && (
            <details class="board-earlier" data-board-earlier={earlier.length}>
              <summary class="dim">{earlierLine(earlier.length)}</summary>
              <div class="board-line board-chips">
                {earlier.map((a) => (
                  <Chip key={a.id} a={a} platform={platform} />
                ))}
              </div>
              {earlier.map((a) => (a.block ? <BlockLine key={a.id} a={a} /> : null))}
            </details>
          )}
          {r.ask && <AskCard a={r.ask} answer={(a) => act(() => boards.answer(id, r.ask!.id, a))} />}
          {r.asked && r.asked.length > 0 && <Answered asked={r.asked} />}
          {r.note && (
            <div class="board-note" data-board-note>
              <Markdown text={r.note} refs={null} />
            </div>
          )}
        </div>
      )}
      <button type="button" class="board-more" aria-label="Row actions" data-board-row-menu onClick={more}>
        ⋯
      </button>
    </li>
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
