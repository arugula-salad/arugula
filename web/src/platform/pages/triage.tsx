// A project's "Not on a board" (#292, #656): its open issues on no board, in
// the Boards page between the open boards and Done. A row is selected by
// clicking it, added to a board with ＋, or dragged onto a board's card.
// `/p/:project/triage` is that page, scrolled here. The reads and the adds are
// untriaged.ts's; the issues list stays at All issues.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { ProjectBoard } from "../../shared/proto";
import { boards } from "../../shared/boards/store";
import { ago } from "../../shared/time";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { names } from "../links";
import { follow, pathOf } from "../router";
import type { PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";
import { WaitingWrites } from "./tracker-writes";
import { TRIAGE_NONE, addedWords, boxState, itemHref, rangeKeys, sourceLabel, targets } from "./triage-text";
import { addUntriaged, countUntriaged, keyOf, pointerOf, readUntriaged, type Chosen, type UntriagedPage } from "./untriaged";
import { ObjectChip, Chip } from "../../shared/ui/state-chip";

/** The open issues on no board, `null` while they load, for each of `projects` (read in parallel); one that can't be read counts none. */
export function useUntriaged(projects: readonly string[]): Map<string, number> | null {
  const [counts, setCounts] = useState<Map<string, number> | null>(null);
  const key = projects.join(",");
  useEffect(() => {
    setCounts(null);
    let live = true;
    void countUntriaged(projects).then((c) => live && setCounts(c));
    return () => void (live = false);
  }, [key]);
  return counts;
}

/** Whether the screen is a phone's (≤640px): the list scrolls with the page there, and rows aren't dragged. */
function useNarrow(): boolean {
  const q = "(max-width: 640px)";
  const [narrow, setNarrow] = useState(() => matchMedia(q).matches);
  useEffect(() => {
    const m = matchMedia(q);
    const fn = () => setNarrow(m.matches);
    m.addEventListener("change", fn);
    return () => m.removeEventListener("change", fn);
  }, []);
  return narrow;
}

/** `value`, once it has stopped changing for `ms`. */
function useSettled<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

/** The section's state, lifted to the page so the board cards can take a drop and the tabs can count. `added` is told when rows went onto a board, so the boards read again. */
export function useTriage(project: string, added: () => void) {
  const [q, setQ] = useState("");
  const [label, setLabel] = useState("");
  const [order, setOrder] = useState<"newest" | "oldest">("newest");
  const [page, setPage] = useState<UntriagedPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [more, setMore] = useState(false);
  // The selection: these rows, or (`all`) every match of the query but the `except`ed, loaded or not.
  const [sel, setSel] = useState<ReadonlySet<string>>(new Set());
  const [all, setAll] = useState(false);
  // The row a Shift-click ranges from, and what a drag carries.
  const anchor = useRef<string | null>(null);
  const carry = useRef<Carried | null>(null);
  const seq = useRef(0);
  const words = useSettled(q, 250);
  const query = { q: words, label, order };
  const same = useRef("");
  same.current = JSON.stringify(query);

  useEffect(() => {
    const n = ++seq.current;
    // A new query is a new selection.
    setAll(false);
    setSel(new Set());
    readUntriaged(project, query).then(
      (p) => {
        if (n !== seq.current) return;
        setPage(p);
        setError(null);
        setMore(false);
      },
      (e: Error) => n === seq.current && setError(e.message),
    );
  }, [project, words, label, order]);

  const items = page?.items ?? [];
  const total = page?.total ?? 0;
  const count = all ? Math.max(0, total - sel.size) : sel.size;
  const isOn = (key: string) => (all ? !sel.has(key) : sel.has(key));
  const loadMore = () => {
    if (!page?.next || more) return;
    const n = seq.current;
    setMore(true);
    readUntriaged(project, { ...query, cursor: page.next }).then(
      (p) => {
        if (n !== seq.current) return;
        setPage((was) => {
          if (!was) return p;
          const have = new Set(was.items.map(keyOf));
          return { ...p, items: [...was.items, ...p.items.filter((i) => !have.has(keyOf(i)))] };
        });
        setMore(false);
      },
      (e: Error) => {
        setMore(false);
        toast(`Couldn't load more: ${e.message}`);
      },
    );
  };

  /** Selects (`on`) or clears the row `key`; in `all` the set holds the ones left out. */
  const mark = (next: Set<string>, key: string, on: boolean) => {
    if (on !== all) next.add(key);
    else next.delete(key);
  };
  const toggle = (key: string, range: boolean) => {
    const keys = items.map(keyOf);
    setSel((was) => {
      const next = new Set(was);
      if (range && anchor.current !== null) for (const k of rangeKeys(keys, anchor.current, key)) mark(next, k, true);
      else mark(next, key, !isOn(key));
      return next;
    });
    anchor.current = key;
  };
  /** Every match, loaded or not; again, none. */
  const selectAll = () => {
    const full = count > 0 && count === total;
    setAll(!full);
    setSel(new Set());
  };
  const clear = () => {
    setAll(false);
    setSel(new Set());
  };
  useEffect(() => {
    if (count === 0) return;
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      if ((e.target as Element | null)?.closest("input:not([type=checkbox]), textarea, select, [contenteditable]")) return;
      clear();
    };
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  }, [count > 0]);

  /** The whole selection. */
  const selection = (): Carried => ({ all, keys: [...sel], count });
  /** What a drag or a ＋ on the row `key` takes: the selection if the row is in it, else the row. */
  const taking = (key: string): Carried => (isOn(key) && count > 1 ? { all, keys: [...sel], count } : { all: false, keys: [key], count: 1 });

  /** Puts what `c` says on `board` in one call, then both halves move: the rows leave the list, the total and the facet are read again, and the boards are. */
  const addTo = async (board: { id: string; title: string }, c: Carried) => {
    const chosen: Chosen = c.all ? { query: { project, q: words || undefined, label: label || undefined }, except: c.keys.map(pointerOf) } : { pointers: c.keys.map(pointerOf) };
    const q0 = same.current;
    let n: number;
    try {
      n = await addUntriaged(board.id, chosen);
    } catch (e) {
      toast(`Couldn't add ${c.count === 1 ? "it" : `${c.count} issues`} to the board: ${(e as Error).message}`);
      return;
    }
    const lone = c.count === 1 && !c.all ? items.find((i) => keyOf(i) === c.keys[0]) : undefined;
    setPage((p) => {
      if (!p) return p;
      const left = c.all ? p.items.filter((i) => c.keys.includes(keyOf(i))) : p.items.filter((i) => !c.keys.includes(keyOf(i)));
      return { ...p, items: left, total: Math.max(0, p.total - n) };
    });
    clear();
    added();
    toast(addedWords(n, lone ? sourceLabel(lone) : "1 issue", board.title));
    // The total and the facet are the server's; the rows loaded stay where they are (all of them, once a drop took every match).
    const fresh = await readUntriaged(project, JSON.parse(q0)).catch(() => null);
    if (!fresh || same.current !== q0) return;
    setPage((p) => (p ? { ...p, total: fresh.total, labels: fresh.labels, ...(c.all ? { items: fresh.items, next: fresh.next } : {}) } : p));
  };

  return { q, setQ, label, setLabel, order, setOrder, page, items, error, more, count, isOn, anchor, carry, taking, selection, toggle, selectAll, clear, loadMore, addTo };
}

export type Triage = ReturnType<typeof useTriage>;

/** What a drag or a menu takes to a board: these rows (`keys`), or with `all` every match but those. */
type Carried = { all: boolean; keys: string[]; count: number };

/** The handlers that make an element a place to drop the dragged rows on `board`. */
export function dropOn(tri: Triage, board: { id: string; title: string }, done?: () => void) {
  return {
    onDragOver: (e: DragEvent) => {
      if (!tri.carry.current) return;
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
      (e.currentTarget as HTMLElement).classList.add("drop-over");
    },
    onDragLeave: (e: DragEvent) => {
      const el = e.currentTarget as HTMLElement;
      if (!el.contains(e.relatedTarget as Node | null)) el.classList.remove("drop-over");
    },
    onDrop: (e: DragEvent) => {
      const taken = tri.carry.current;
      if (!taken) return;
      e.preventDefault();
      (e.currentTarget as HTMLElement).classList.remove("drop-over");
      tri.carry.current = null;
      document.body.classList.remove("dragging");
      done?.();
      void tri.addTo(board, taken);
    },
  };
}

/** Whether any of the page's board cards is on screen. */
function boardsInView(): boolean {
  const g = document.querySelector("[data-boards] .board-card");
  if (!g) return false;
  const r = g.getBoundingClientRect();
  return r.bottom > 40 && r.top < innerHeight;
}

/** A project's "Not on a board" on its Boards page. `open` are the boards a row can go to; `scroll` brings the section into view once it has its list (`/triage`). */
export function NotOnBoard({ p, tri, open, scroll }: { p: PlatformProject; tri: Triage; open: ProjectBoard[]; scroll?: boolean }) {
  const { page, items, error, count } = tri;
  const narrow = useNarrow();
  const write = mayWrite(p);
  const top = useRef<HTMLElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const sentinel = useRef<HTMLLIElement>(null);
  const all = useRef<HTMLInputElement>(null);
  const [dock, setDock] = useState(false);
  const allLink = pathOf({ page: "issues", project: p.id }, names);
  const ready = page !== null || error !== null;
  useEffect(() => {
    if (scroll && ready) top.current?.scrollIntoView({ block: "start" });
  }, [scroll, ready]);
  // A new filter is a new list: from its top. Loading, selecting and adding keep the box where it is.
  useLayoutEffect(() => {
    if (box.current) box.current.scrollTop = 0;
  }, [tri.q.trim(), tri.label, tri.order]);
  // The next page, when the sentinel nears the foot of the box (of the page, on a phone).
  useEffect(() => {
    const s = sentinel.current;
    if (!s) return;
    const io = new IntersectionObserver((e) => e[0]?.isIntersecting && tri.loadMore(), { root: narrow ? null : box.current, rootMargin: "200px" });
    io.observe(s);
    return () => io.disconnect();
  }, [page?.next, items.length, narrow, tri.more]);
  const state = boxState(count, page?.total ?? 0);
  useEffect(() => {
    if (all.current) all.current.indeterminate = state === "some";
  }, [state]);

  // A drag from a row: the dock of boards drops from the top when the cards are off screen.
  useEffect(() => {
    const over = () => {
      if (!tri.carry.current) return;
      const off = !boardsInView();
      setDock((was) => (was === off ? was : off));
    };
    document.addEventListener("dragover", over);
    return () => document.removeEventListener("dragover", over);
  }, []);
  const canDrag = write && !narrow && !matchMedia("(pointer: coarse)").matches;
  const endDrag = () => {
    tri.carry.current = null;
    document.body.classList.remove("dragging");
    setDock(false);
    document.querySelectorAll(".drop-over").forEach((x) => x.classList.remove("drop-over"));
  };
  const dragStart = (e: DragEvent, k: string) => {
    tri.carry.current = tri.taking(k);
    e.dataTransfer?.setData("text/plain", `${tri.carry.current.count} issues`);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
    // After the browser has taken the drag image: changing the page at once can cancel the drag.
    requestAnimationFrame(() => {
      if (!tri.carry.current) return;
      document.body.classList.add("dragging");
      setDock(!boardsInView());
    });
  };

  // The boards, read the first time a menu is opened.
  const where = useRef<Promise<ReturnType<typeof targets>> | null>(null);
  const menu = async (from: HTMLElement, taken: ReturnType<Triage["taking"]>) => {
    const r = from.getBoundingClientRect();
    const at = { clientX: r.left, clientY: r.bottom, preventDefault: () => {} };
    try {
      where.current ??= boards.list(p.id).then((l) => targets(l.boards));
      const list = await where.current;
      const entries: MenuItem[] = list.length
        ? [{ header: taken.count === 1 ? "Add to…" : `Add ${taken.count.toLocaleString()} issues to…` }, ...list.map((b) => ({ label: b.title, run: () => void tri.addTo({ id: b.board, title: b.title }, taken) }))]
        : [{ label: "No open board to add to", run: () => {}, disabled: true }];
      openMenu(at, entries, { from });
    } catch (err) {
      where.current = null;
      toast(`Couldn't read the boards: ${(err as Error).message}`);
    }
  };

  const total = page?.total ?? 0;
  const filtered = tri.q.trim() !== "" || tri.label !== "";
  const tools = total > 8 || filtered || tri.order !== "newest";
  const words = count === 0 ? `Select all ${total.toLocaleString()}` : state === "all" ? `All ${total.toLocaleString()} selected` : `${count.toLocaleString()} of ${total.toLocaleString()} selected`;

  return (
    <section ref={top} id="triage" class="boards-untriaged" aria-labelledby={`untriaged-${p.id}`} data-boards-untriaged>
      <div class="sw-section-head">
        <h2 id={`untriaged-${p.id}`}>
          Not on a board{page && <span class="untri-n" data-triage-count>{total.toLocaleString()}</span>}
        </h2>
        <a class="sw-all" href={allLink} data-triage-all onClick={(e) => follow(e, allLink)}>
          All issues ›
        </a>
      </div>
      {error && <p class="control-error" role="alert">{error}</p>}
      {!error && page === null && <p class="dim">Loading…</p>}
      {page && total === 0 && !filtered && (
        <p class="dim" data-triage-none>
          {TRIAGE_NONE}
        </p>
      )}
      {page && (total > 0 || filtered) && (
        <>
          {tools && (
            <div class="untri-tools">
              <input type="search" class="issues-search" value={tri.q} placeholder="Filter by words or #number" aria-label="Filter issues not on a board" data-triage-filter autoComplete="off" onInput={(e) => tri.setQ(e.currentTarget.value)} />
              <select class="issues-filter" value={tri.order} aria-label="Order" data-triage-order onChange={(e) => tri.setOrder(e.currentTarget.value as "newest" | "oldest")}>
                <option value="newest">Newest first</option>
                <option value="oldest">Oldest first</option>
              </select>
              {page.labels.length > 1 && (
                <div class="untri-labels" role="group" aria-label="Labels">
                  {page.labels.map((l) => (
                    <button key={l.label} type="button" class="untri-label" aria-pressed={tri.label === l.label} data-triage-label={l.label} onClick={() => tri.setLabel(tri.label === l.label ? "" : l.label)}>
                      {l.label}
                      <span class="untri-ct">{l.count.toLocaleString()}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
          <div class="untri-frame">
            {write && items.length > 0 && (
              <label class="untri-head">
                <input ref={all} type="checkbox" checked={state === "all"} aria-label="Select all" data-triage-all-box onChange={tri.selectAll} />
                <span>{words}</span>
              </label>
            )}
            <div class="untri-box" ref={box} data-triage-box>
              <ul class="untri-list" data-triage-list>
                {items.map((i) => {
                  const k = keyOf(i);
                  const on = tri.isOn(k);
                  const name = sourceLabel(i);
                  const href = itemHref(i, (id) => pathOf({ page: "issue", project: p.id, issue: id }, names));
                  const github = i.source === "github" || i.source === "plane";
                  return (
                    <li
                      key={k}
                      class="untri-row"
                      data-selected={on || undefined}
                      data-triage-row={name}
                      draggable={canDrag}
                      onClick={(e) => {
                        if (!write || (e.target as Element).closest("a, button")) return;
                        tri.toggle(k, e.shiftKey);
                      }}
                      onDragStart={(e) => dragStart(e, k)}
                      onDragEnd={endDrag}
                    >
                      {write && <input type="checkbox" checked={on} aria-label={`Select ${name}`} data-triage-select onChange={() => {}} />}
                      <span class="untri-num">{name}</span>
                      {href ? (
                        <a class="untri-title" href={href} title={i.title} draggable={false} {...(github ? { target: "_blank", rel: "noopener noreferrer" } : { onClick: (e: MouseEvent) => follow(e, href) })}>
                          {i.title}
                        </a>
                      ) : (
                        <span class="untri-title dim">{i.title}</span>
                      )}
                      <span class="untri-fill" />
                      {i.labels?.map((l) => (
                        <Chip key={l} kind="tag" class="untri-label">
                          {l}
                        </Chip>
                      ))}
                      <span class="untri-age dim">{ago(i.updated_at, Date.now(), "short")}</span>
                      {write && (
                        <button type="button" class="untri-add" aria-label={`Add ${name} to a board`} aria-haspopup="menu" data-triage-add onClick={(e) => void menu(e.currentTarget, tri.taking(k))}>
                          ＋
                        </button>
                      )}
                    </li>
                  );
                })}
                {items.length === 0 && (
                  <li class="untri-empty dim" data-triage-nomatch>
                    Nothing matches.
                  </li>
                )}
                {page.next && (
                  <li class="untri-empty dim" ref={sentinel} data-triage-more>
                    Loading more…
                  </li>
                )}
              </ul>
            </div>
          </div>
        </>
      )}
      {write && <WaitingWrites project={p.id} />}
      {count > 0 && (
        <div class="untri-selbar" role="region" aria-label="Selected issues" data-triage-selbar>
          <span data-triage-selected>{count.toLocaleString()} selected</span>
          <button type="button" class="untri-go" aria-haspopup="menu" onClick={(e) => void menu(e.currentTarget, tri.selection())}>
            ＋ Add to board
          </button>
          <button type="button" class="untri-clear" onClick={tri.clear}>
            Clear
          </button>
        </div>
      )}
      <div class="untri-dock" data-on={dock || undefined} data-triage-dock aria-hidden="true">
        <span class="untri-dock-label">Drop on a board:</span>
        {open.map((b) => (
          <ObjectChip key={b.id} data-dock-board={b.id} {...dropOn(tri, b, () => setDock(false))}>
            {b.title}
          </ObjectChip>
        ))}
      </div>
    </section>
  );
}
