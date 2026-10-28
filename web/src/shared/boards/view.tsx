// One board (live status board, step 2), wherever it's drawn: the platform
// page's `/p/:project/boards/:board`, and a board block in a daemon
// workspace (a pin onto it, `shell/blocks/board.tsx`). Zones in order, each
// with a progress bar and its rows: a glyph for the row's own status, a
// link to what it points at with that item's state now (`live`), and a
// note. Owners and editors add, edit and reorder zones and rows; a viewer's
// writes are refused and say so.
//
// It reads the board as the person looking: now, again when the platform
// says it changed (the board's thread, chat's live stream), and when the
// page comes back into view. A write on an entry that moved on is refused
// (409): the view reads the board again and says so, and the change is theirs
// to make again.

import { useEffect, useId, useRef, useState } from "preact/hooks";
import type {
  BoardPointer,
  ProjectBoard,
  BoardRow,
  BoardView,
  BoardWorkItem,
  BoardZone,
} from "../proto";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { Markdown } from "../ui/markdown";
import { useSubscribe } from "../ui/hooks";
import { askText, openMenu, type MenuItem } from "../ui/menu";
import { issues } from "../issues/store";
import { day } from "../time";
import { BoardError, STALE_REV, boards } from "./store";
import "./boards.css";
import {
  BLOCK_LABEL,
  GLYPH,
  LIVE_LABEL,
  PICK_DELAY_MS,
  PICK_LIMIT,
  STATUSES,
  STATUS_LABEL,
  assigneesLine,
  blockClass,
  GOAL_MAX,
  blockFacts,
  debounced,
  epicLine,
  isCard,
  liveClass,
  pointerHref,
  pointerLabel,
  pointerText,
  pointerToSend,
  progress,
  proven,
  stepActive,
  stepBefore,
  workLabel,
} from "./text";

const STALE_SAID =
  "That changed while you were editing, so the board is read again. Make your change again if it still applies.";

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
  /** Whether to offer the board's own edits (title, goal, closing). False
   * for a viewer; a block can't tell, so it offers them and the platform's
   * 403 comes back as a toast. */
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

  if (missing && !view)
    return (
      <p class="board-none dim" data-board-missing>
        That board isn't one you can read ({missing}).
      </p>
    );
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

  const addZone = async (kind?: "proofs") => {
    const title = kind
      ? "PROOFS"
      : await askText("New zone", "", "its title, like Now");
    if (title?.trim())
      await act(() =>
        boards.put(id, {
          entry: "zone",
          title: title.trim(),
          kind: kind ?? null,
        }),
      );
  };

  /** A native issue in the board's project, and a row pointing at it in the zone. */
  const newIssue = async (zone: string) => {
    const title = await askText("New issue", "", "its title");
    if (!title?.trim()) return;
    let made;
    try {
      made = await issues.create(view.board.project, {
        title: title.trim(),
        body: "",
        labels: [],
        assignees: [],
      });
    } catch (e) {
      toast(`Couldn't make the issue: ${(e as Error).message}`);
      return;
    }
    const ok = await act(() =>
      boards.put(id, {
        entry: "row",
        zone,
        pointer: { kind: "issue", id: made.id },
      }),
    );
    if (!ok) toast(`Made #${made.number} but couldn't add it to the board`);
  };

  const rename = async () => {
    const t = await askText("Rename the board", view.board.title);
    if (t?.trim() && t.trim() !== view.board.title)
      await act(() => boards.edit(id, { title: t.trim() }));
  };

  const zoneIds = view.zones.map((z) => z.id);
  return (
    <div class="board" data-board={id}>
      {back}
      <h1 class="board-title" data-board-title>
        {write ? (
          <span
            role="button"
            tabIndex={0}
            class="board-rename"
            title="Rename the board"
            data-board-rename
            onClick={() => void rename()}
            onKeyDown={(e) =>
              (e.key === "Enter" || e.key === " ") &&
              (e.preventDefault(), void rename())
            }
          >
            {view.board.title}
          </span>
        ) : (
          view.board.title
        )}
      </h1>
      <Epic b={view.board} act={act} id={id} write={write} />
      {stale && (
        <p class="board-stale" role="status" data-board-stale>
          {STALE_SAID}
        </p>
      )}
      {view.zones.length === 0 && <p class="dim">No zones yet.</p>}
      {view.zones.map((z, i) => (
        <Zone
          key={z.id}
          z={z}
          view={view}
          index={i}
          zoneIds={zoneIds}
          editing={editing}
          setEditing={setEditing}
          act={act}
          id={id}
          platform={platform}
          newIssue={newIssue}
        />
      ))}
      <p class="board-foot">
        <button
          type="button"
          data-board-add-zone
          onClick={() => void addZone()}
        >
          + Add zone
        </button>
        {!view.zones.some((z) => z.kind === "proofs") && (
          <button
            type="button"
            data-board-add-proofs
            onClick={() => void addZone("proofs")}
          >
            + Add PROOFS
          </button>
        )}
      </p>
    </div>
  );
}

/** A board's goal, progress and closing, under its title. The panel doesn't
 * know whether the person may write (the pin block has no project), so the
 * controls show and the platform's refusal comes back as `act`'s toast. */
function Epic({
  b,
  act,
  id,
  write,
}: {
  b: ProjectBoard;
  act: Act;
  id: string;
  write: boolean;
}) {
  const [goalOpen, setGoalOpen] = useState(false);
  const [goal, setGoal] = useState("");
  const [closing, setClosing] = useState(false);
  const [note, setNote] = useState("");
  const e = epicLine(b.progress);
  const closed = b.closed_at != null;
  const saveGoal = async () => {
    if (await act(() => boards.edit(id, { goal: goal.trim() })))
      setGoalOpen(false);
  };
  const close = async () => {
    if (await act(() => boards.close(id, note.trim()))) setClosing(false);
  };
  return (
    <div class="board-epic">
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
          />
          <span class="board-form-buttons">
            <button
              type="button"
              class="primary"
              data-board-goal-save
              onClick={() => void saveGoal()}
            >
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
              <button
                type="button"
                data-board-goal-edit
                onClick={() => {
                  setGoal(b.goal);
                  setGoalOpen(true);
                }}
              >
                {b.goal ? "Edit goal" : "Add a goal"}
              </button>
            </p>
          )}
        </>
      )}
      <div class="board-progress" data-board-progress>
        {b.progress.total > 0 && (
          <div
            class="board-bar"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={b.progress.total}
            aria-valuenow={b.progress.done}
            aria-label={e.words}
          >
            <i style={{ width: `${e.pct}%` }} />
          </div>
        )}
        <span class="dim">{e.words}</span>
      </div>
      {closed ? (
        <div class="board-done" data-board-done>
          <p>
            <span class="ws-tag board-done-tag">Done</span>{" "}
            <span class="dim">{day(b.closed_at!)}</span>
          </p>
          <p class="board-goal" data-board-done-note>
            {b.done_note}
          </p>
          {write && (
            <p class="board-epic-actions">
              <button
                type="button"
                data-board-reopen
                onClick={() => void act(() => boards.reopen(id))}
              >
                Reopen
              </button>
            </p>
          )}
        </div>
      ) : closing ? (
        <div class="board-form" data-board-close-form>
          <label>
            What can a person now do?
            <textarea
              value={note}
              rows={3}
              aria-label="What can a person now do?"
              data-board-close-note
              autoFocus
              onInput={(ev) => setNote(ev.currentTarget.value)}
            />
          </label>
          <span class="board-form-buttons">
            <button
              type="button"
              class="primary"
              data-board-close-save
              disabled={!note.trim()}
              onClick={() => void close()}
            >
              Close epic
            </button>
            <button type="button" onClick={() => setClosing(false)}>
              Cancel
            </button>
          </span>
        </div>
      ) : write ? (
        <p class="board-epic-actions">
          <button
            type="button"
            data-board-close
            onClick={() => {
              setNote("");
              setClosing(true);
            }}
          >
            Close epic…
          </button>
        </p>
      ) : null}
    </div>
  );
}

type Act = (write: () => Promise<BoardView>) => Promise<boolean>;

function Zone({
  z,
  view,
  index,
  zoneIds,
  editing,
  setEditing,
  act,
  id,
  platform,
  newIssue,
}: {
  z: BoardZone;
  view: BoardView;
  index: number;
  zoneIds: string[];
  editing: string | null;
  setEditing: (e: string | null) => void;
  act: Act;
  id: string;
  platform: (path: string) => string | undefined;
  newIssue: (zone: string) => Promise<void>;
}) {
  const [gone, setGone] = useState(false);
  const p = progress(z);
  const proofs = z.kind === "proofs";
  const rowIds = z.rows.map((r) => r.id);
  const menu = (e: MouseEvent) => {
    const up = stepBefore(zoneIds, index, true);
    const down = stepBefore(zoneIds, index, false);
    menuAt(e, [
      {
        label: "Rename…",
        disabled: proofs,
        run: async () => {
          const t = await askText("Rename the zone", z.title);
          if (t?.trim() && t.trim() !== z.title)
            await act(() =>
              boards.put(id, { id: z.id, rev: z.rev, title: t.trim() }),
            );
        },
      },
      {
        label: "Move up",
        disabled: !up || proofs,
        run: () =>
          void act(() => boards.move(id, z.id, { before: up!.before })),
      },
      {
        label: "Move down",
        disabled:
          !down ||
          proofs ||
          zoneIds[index + 1] ===
            view.zones.find((x) => x.kind === "proofs")?.id,
        run: () =>
          void act(() => boards.move(id, z.id, { before: down!.before })),
      },
      { label: "New issue…", disabled: proofs, run: () => void newIssue(z.id) },
      "separator",
      {
        label: "Delete the zone and its rows",
        danger: true,
        run: () => setGone(true),
      },
    ]);
  };
  return (
    <section
      class="board-zone"
      data-board-zone={z.id}
      data-zone-kind={z.kind ?? undefined}
    >
      <header class="board-zone-head">
        <h2>{z.title}</h2>
        <span class="board-count dim" data-board-count>
          {p.done}/{p.total}
        </span>
        <button
          type="button"
          class="board-more"
          aria-label={`Actions for ${z.title}`}
          data-board-zone-menu
          onClick={menu}
        >
          ⋯
        </button>
      </header>
      <div
        class="board-bar"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={p.pct}
        aria-label={`${z.title}: ${p.done} of ${p.total} done`}
      >
        <i style={{ width: `${p.pct}%` }} />
      </div>
      <ul class="board-rows">
        {z.rows.map((r, i) => (
          <Row
            key={r.id}
            r={r}
            zone={z}
            view={view}
            index={i}
            rowIds={rowIds}
            editing={editing === r.id}
            setEditing={setEditing}
            act={act}
            id={id}
            platform={platform}
          />
        ))}
      </ul>
      {editing === `new:${z.id}` ? (
        <RowForm
          project={view.board.project}
          row={null}
          save={(f) =>
            act(() =>
              boards.put(id, {
                entry: "row",
                zone: z.id,
                note: f.note,
                pointer: f.pointer ?? null,
              }),
            )
          }
          done={() => setEditing(null)}
        />
      ) : (
        <button
          type="button"
          class="board-add"
          data-board-add-row
          onClick={() => setEditing(`new:${z.id}`)}
        >
          + Add row
        </button>
      )}
      {gone && (
        <p class="board-confirm" data-board-confirm>
          Delete “{z.title}” and its {z.rows.length} rows?{" "}
          <button
            type="button"
            class="danger"
            data-board-confirm-delete
            onClick={() => void act(() => boards.remove(id, z.id))}
          >
            Delete
          </button>{" "}
          <button type="button" onClick={() => setGone(false)}>
            Keep
          </button>
        </p>
      )}
    </section>
  );
}

function Row({
  r,
  zone,
  view,
  index,
  rowIds,
  editing,
  setEditing,
  act,
  id,
  platform,
}: {
  r: BoardRow;
  zone: BoardZone;
  view: BoardView;
  index: number;
  rowIds: string[];
  editing: boolean;
  setEditing: (e: string | null) => void;
  act: Act;
  id: string;
  platform: (path: string) => string | undefined;
}) {
  const label = pointerLabel(r.pointer, r.live);
  const href = pointerHref(r, platform);
  const card = isCard(r.live);
  const who = card ? assigneesLine(r.live!.assignees) : "";
  const proofTag = zone.kind === "proofs" && (
    <span
      class={`ws-tag board-proof ${proven(r) ? "proven" : "unproven"}`}
      data-board-proof={proven(r) ? "proven" : "not-proven"}
    >
      {proven(r) ? "proven" : "not proven yet"}
    </span>
  );
  const statusMenu = (e: MouseEvent) =>
    menuAt(
      e,
      STATUSES.map((s) => ({
        label: STATUS_LABEL[s],
        checked: r.status === s,
        run: () => void act(() => boards.put(id, { id: r.id, status: s })),
      })),
    );
  const more = (e: MouseEvent) => {
    const up = stepBefore(rowIds, index, true);
    const down = stepBefore(rowIds, index, false);
    const others = view.zones.filter((z) => z.id !== zone.id);
    menuAt(e, [
      { label: "Edit…", run: () => setEditing(r.id) },
      {
        label: "Move up",
        disabled: !up,
        run: () =>
          void act(() => boards.move(id, r.id, { before: up!.before })),
      },
      {
        label: "Move down",
        disabled: !down,
        run: () =>
          void act(() => boards.move(id, r.id, { before: down!.before })),
      },
      ...others.map((z) => ({
        label: `Move to ${z.title}`,
        run: () => void act(() => boards.move(id, r.id, { zone: z.id })),
      })),
      "separator",
      {
        label: "Delete the row",
        danger: true,
        run: () => void act(() => boards.remove(id, r.id)),
      },
    ]);
  };
  return (
    <li class="board-row" data-board-row={r.id} data-status={r.status}>
      <button
        type="button"
        class={`board-glyph s-${r.status}`}
        title={STATUS_LABEL[r.status]}
        aria-label={`Status: ${STATUS_LABEL[r.status]}`}
        data-board-glyph
        onClick={statusMenu}
      >
        {GLYPH[r.status]}
      </button>
      {editing ? (
        <RowForm
          project={view.board.project}
          row={r}
          save={(f) =>
            act(() =>
              boards.put(id, {
                id: r.id,
                rev: r.rev,
                note: f.note,
                ...(f.pointer ? { pointer: f.pointer } : {}),
              }),
            )
          }
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
            {label &&
              (href ? (
                <a
                  class="board-ptr"
                  href={href}
                  target={
                    r.pointer.kind === "forge_item" || r.pointer.kind === "link"
                      ? "_blank"
                      : undefined
                  }
                  rel="noopener noreferrer"
                  data-board-ptr
                >
                  {label}
                </a>
              ) : (
                <span class="board-ptr" data-board-ptr>
                  {label}
                </span>
              ))}
            {r.live && (
              <span
                class={`ws-tag board-live ${liveClass(r.live.state)}`}
                data-board-live={r.live.state}
              >
                {LIVE_LABEL[r.live.state]}
              </span>
            )}
            {card && who && (
              <span class="board-who dim" data-board-who>
                {who}
              </span>
            )}
            {r.live && !card && (
              <span class="board-item dim">{r.live.title}</span>
            )}
            {!card && proofTag}
          </div>
          {r.block && (
            <div
              class="board-line board-block"
              data-board-block={r.block.state}
            >
              <span class={`ws-tag ${blockClass(r.block.state)}`}>
                {BLOCK_LABEL[r.block.state]}
              </span>
              {blockFacts(r.block).map((f) => (
                <span class="board-fact dim" key={f}>
                  {f}
                </span>
              ))}
            </div>
          )}
          {r.note && (
            <div class="board-note" data-board-note>
              <Markdown text={r.note} refs={null} />
            </div>
          )}
        </div>
      )}
      <button
        type="button"
        class="board-more"
        aria-label="Row actions"
        data-board-row-menu
        onClick={more}
      >
        ⋯
      </button>
    </li>
  );
}

/** A row's note and pointer, new or edited. The pointer is sent only when
 * it was changed (an agent block's isn't typed, and stays). "Points at" is a
 * search over the project's work items; a result picked is the pointer, and
 * text typed and not picked is an id or link as before. */
function RowForm({
  project,
  row,
  save,
  done,
}: {
  project: string;
  row: BoardRow | null;
  save: (f: { note: string; pointer?: BoardPointer }) => Promise<boolean>;
  done: () => void;
}) {
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
      <input
        type="text"
        value={note}
        maxLength={280}
        placeholder="note"
        aria-label="Note"
        data-board-note-input
        autoFocus
        onInput={(e) => setNote(e.currentTarget.value)}
      />
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
          placeholder="points at: search, or iss_…, fi_…, run_…, hw_… or https://…"
          aria-label="Points at"
          aria-invalid={bad}
          data-board-ptr-input
          onInput={(e) => type(e.currentTarget.value)}
          onKeyDown={keys}
        />
        <ul
          class="board-pick-list"
          role="listbox"
          id={listId}
          aria-label="Work items"
          hidden={!found.length}
          data-board-pick-list
        >
          {found.map((w, i) => (
            <li
              key={w.pointer.kind + ("id" in w.pointer ? w.pointer.id : "")}
              id={optionId(i)}
              role="option"
              aria-selected={i === active}
              class={i === active ? "active" : undefined}
              data-board-pick={i}
              onMouseDown={(e) => (e.preventDefault(), pick(w))}
            >
              <span class="board-ptr">{workLabel(w)}</span>
              <span class="board-pick-title">{w.title}</span>
              <span class="board-pick-state dim">{w.state}</span>
            </li>
          ))}
        </ul>
      </div>
      {bad && (
        <span class="board-bad">
          Expected an id that starts iss_, fi_, run_ or hw_, or an https://
          link.
        </span>
      )}
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
