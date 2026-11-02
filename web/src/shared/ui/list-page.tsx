// A list page (Arugula #587): one row for every list, the same tools, and a
// live preview in the room beside it. Its rules:
//
// - Quiet by default. Rows have no box; a bar or a color only where a row
//   differs from its neighbours (open, failed, unread).
// - It never repeats the filter: in an Open list, nothing says "Open".
// - Accent means live.
// - One primary button, in the preview. The row itself is the target, and
//   what's destructive is behind ⋯ there.
// - The counts are the filters, `/` searches, ↑↓ and j/k move, Enter opens.
//
// The split goes by the room the page has (container queries, ./list-page.css),
// never the window's width: a page can be narrow on a wide screen. Too narrow
// for both, the list stands alone and a row opens its preview in its place.

import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import "./list-page.css";

/** Under this, the list stands alone (as in ./list-page.css). */
const NARROW = 760;
const LIST_MIN = 340;
const PREVIEW_MIN = 380;
/** A row under the pointer this long is the one previewed, so that crossing rows doesn't flicker. */
const HOVER_MS = 140;
/** Rows moved by the keyboard pass under a still pointer: that's not hovering. */
const KEYS_HOLD_MS = 400;

export type ListTone = "live" | "bad" | "warn";

/** One count over the list. With `key`, it's a filter. */
export interface ListCount {
  key?: string;
  n: ComponentChildren;
  label: string;
  tone?: ListTone;
}

export function ListCounts({ counts, on, pick }: { counts: ListCount[]; on: string; pick: (key: string) => void }) {
  return (
    <div class="list-counts" role="group" aria-label="Filter">
      {counts.map((c) =>
        c.key ? (
          <button key={c.key} type="button" class={c.tone ? `list-count ${c.tone}` : "list-count"} aria-pressed={on === c.key} data-list-count={c.key} onClick={() => pick(c.key!)}>
            <b>{c.n}</b>
            {c.label}
          </button>
        ) : (
          <span key={c.label} class="list-count still">
            <b>{c.n}</b>
            {c.label}
          </span>
        ),
      )}
    </div>
  );
}

/** What a row says, in the places every list's row says it. */
export interface ListRowParts {
  tone?: ListTone;
  title: ComponentChildren;
  /** Beside the title: only what's unusual about it. */
  tags?: ComponentChildren;
  age?: ComponentChildren;
  /** The dim line under it. */
  detail?: ComponentChildren;
  /** The row's picture of its data, at the right of that line. */
  mark?: ComponentChildren;
  /** Its `data-*` hooks. */
  attrs?: Record<string, string>;
}

export interface ListGroup<T> {
  title?: string;
  items: T[];
}

export interface ListPageProps<T> {
  /** What the list is, for the back link and for assistive tech: "Docs". */
  label: string;
  /** The page's header and its counts. */
  header: ComponentChildren;
  /** Its search box's words. */
  searchLabel: string;
  query: string;
  setQuery: (q: string) => void;
  groups: ListGroup<T>[];
  keyOf: (item: T) => string;
  row: (item: T) => ListRowParts;
  preview: (item: T) => ComponentChildren;
  /** Enter, or a double click. */
  onOpen: (item: T) => void;
  /** Where the divider's position is kept. */
  splitKey: string;
  /** Under the search: what went wrong. */
  notice?: ComponentChildren;
  /** In the list's place when it has nothing: loading, none yet, none match. */
  status?: ComponentChildren;
}

const inField = (t: EventTarget | null) => !!(t as Element | null)?.closest?.("input, textarea, select, [contenteditable]");
/** A dialog or a menu is over the page: its keys are its own. */
const covered = () => !!document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"]');

function stored(key: string): number | null {
  try {
    const n = Number(localStorage.getItem(key));
    return n > 0 ? n : null;
  } catch {
    return null;
  }
}

function store(key: string, px: number | null) {
  try {
    if (px === null) localStorage.removeItem(key);
    else localStorage.setItem(key, String(px));
  } catch {
    /* It's only remembered while storage works. */
  }
}

export function ListPage<T>({ label, header, searchLabel, query, setQuery, groups, keyOf, row, preview, onOpen, splitKey, notice, status }: ListPageProps<T>) {
  const root = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLInputElement>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [detail, setDetail] = useState(false);
  const [narrow, setNarrow] = useState(false);
  const storeKey = `arugula-list-split:${splitKey}`;
  const [width, setWidth] = useState<number | null>(() => stored(storeKey));
  const scrollTo = useRef(false);
  const keyed = useRef(0);
  const hover = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(hover.current), []);

  const flat = groups.flatMap((g) => g.items);
  const selected = flat.find((i) => keyOf(i) === picked) ?? flat[0];
  const selectedKey = selected === undefined ? null : keyOf(selected);
  const rowId = (key: string) => `list-row-${key}`;

  useEffect(() => {
    const el = root.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const seen = new ResizeObserver(() => setNarrow(el.clientWidth < NARROW));
    seen.observe(el);
    return () => seen.disconnect();
  }, []);
  // Wider again, or the Doc gone: back to the list.
  useEffect(() => {
    if (detail && (!narrow || selected === undefined)) setDetail(false);
  }, [detail, narrow, selected === undefined]);
  // The keyboard's arrows work as soon as the page is up.
  useEffect(() => {
    if (!inField(document.activeElement)) list.current?.focus({ preventScroll: true });
  }, [list.current === null]);
  useLayoutEffect(() => {
    if (scrollTo.current && selectedKey !== null) document.getElementById(rowId(selectedKey))?.scrollIntoView({ block: "nearest" });
    scrollTo.current = false;
  }, [selectedKey]);

  const live = useRef({ flat, selected, selectedKey, narrow, detail, onOpen, keyOf, query, setQuery });
  live.current = { flat, selected, selectedKey, narrow, detail, onOpen, keyOf, query, setQuery };
  useEffect(() => {
    const keys = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented || covered()) return;
      const s = live.current;
      const searching = e.target === box.current;
      if (e.key === "/" && !inField(e.target)) {
        e.preventDefault();
        box.current?.focus();
        return;
      }
      if (e.key === "Escape") {
        if (searching) {
          e.preventDefault();
          s.setQuery("");
          list.current?.focus({ preventScroll: true });
        } else if (s.detail) {
          e.preventDefault();
          setDetail(false);
        }
        return;
      }
      if (inField(e.target) && !searching) return;
      const arrow = e.key === "ArrowDown" || e.key === "ArrowUp";
      const letter = !searching && (e.key === "j" || e.key === "k");
      if ((arrow || letter) && s.flat.length) {
        e.preventDefault();
        const at = s.flat.findIndex((i) => s.keyOf(i) === s.selectedKey);
        const to = Math.max(0, Math.min(s.flat.length - 1, at + (e.key === "ArrowDown" || e.key === "j" ? 1 : -1)));
        keyed.current = performance.now();
        scrollTo.current = true;
        setPicked(s.keyOf(s.flat[to]));
        return;
      }
      // A button's Enter is its own.
      if (e.key === "Enter" && s.selected !== undefined && !(e.target as Element | null)?.closest?.("button, a, [role='separator']")) {
        e.preventDefault();
        if (s.narrow) setDetail(true);
        else s.onOpen(s.selected);
      }
    };
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, []);

  const clamp = (px: number) => {
    const w = root.current?.clientWidth ?? 0;
    return Math.round(Math.max(LIST_MIN, Math.min(w - PREVIEW_MIN, px)));
  };
  const resize = (px: number) => {
    const v = clamp(px);
    setWidth(v);
    store(storeKey, v);
  };
  const drag = (e: PointerEvent) => {
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    const left = root.current?.getBoundingClientRect().left ?? 0;
    handle.setPointerCapture(e.pointerId);
    handle.classList.add("dragging");
    const move = (m: PointerEvent) => resize(m.clientX - left);
    const up = () => {
      handle.classList.remove("dragging");
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  };
  const paneWidth = () => list.current?.parentElement?.offsetWidth ?? 0;
  const total = root.current?.clientWidth ?? 0;
  const pct = total && width ? Math.round((clamp(width) / total) * 100) : 44;

  const hovered = (key: string, e: PointerEvent) => {
    if (e.pointerType !== "mouse" || narrow || key === selectedKey) return;
    clearTimeout(hover.current);
    hover.current = setTimeout(() => {
      if (performance.now() - keyed.current > KEYS_HOLD_MS) setPicked(key);
    }, HOVER_MS);
  };

  const draw = (item: T) => {
    const key = keyOf(item);
    const r = row(item);
    return (
      <div
        key={key}
        id={rowId(key)}
        role="option"
        aria-selected={key === selectedKey}
        class={r.tone ? `list-row ${r.tone}` : "list-row"}
        {...r.attrs}
        onClick={() => {
          setPicked(key);
          if (narrow) setDetail(true);
        }}
        onDblClick={() => !narrow && onOpen(item)}
        onPointerMove={(e) => hovered(key, e)}
        onPointerLeave={() => clearTimeout(hover.current)}
      >
        <div class="list-row-top">
          <span class="list-row-title">{r.title}</span>
          {r.tags}
        </div>
        <span class="list-row-age">{r.age}</span>
        <div class="list-row-detail">{r.detail}</div>
        <span class="list-row-mark">{r.mark}</span>
      </div>
    );
  };

  const style = width ? ({ "--list-w": `${width}px` } as Record<string, string>) : undefined;
  return (
    <div ref={root} class={detail ? "list-page detail" : "list-page"} style={style} data-list-page>
      <div class="list-split">
        <section class="list-pane" aria-label={label}>
          <div class="list-head">{header}</div>
          <div class="list-tools">
            <label class="list-search">
              <input
                ref={box}
                type="search"
                placeholder={searchLabel}
                aria-label={searchLabel}
                autoComplete="off"
                value={query}
                data-list-search
                onInput={(e) => {
                  setQuery(e.currentTarget.value);
                  setPicked(null);
                }}
              />
              <kbd aria-hidden="true">/</kbd>
            </label>
          </div>
          {notice}
          {status ?? null}
          {flat.length > 0 && (
            <div ref={list} class="list-rows" role="listbox" tabIndex={0} aria-label={label} aria-activedescendant={selectedKey === null ? undefined : rowId(selectedKey)} data-list-rows>
              {groups.map((g, i) =>
                g.items.length === 0 ? null : g.title ? (
                  <div key={g.title} role="group" aria-labelledby={`list-group-${i}`}>
                    <div class="list-group" id={`list-group-${i}`}>
                      {g.title}
                      <span>{g.items.length}</span>
                    </div>
                    {g.items.map(draw)}
                  </div>
                ) : (
                  g.items.map(draw)
                ),
              )}
            </div>
          )}
        </section>
        {selected !== undefined && (
          <>
            <aside class="list-preview" aria-label="Preview" data-list-preview>
              {detail && (
                <button type="button" class="link list-back" data-list-back onClick={() => setDetail(false)}>
                  <span aria-hidden="true">‹ </span>
                  {label}
                </button>
              )}
              {preview(selected)}
            </aside>
            <div
              class="list-handle"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize the list and the preview"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={pct}
              tabIndex={0}
              title="Drag to resize. Double-click to reset."
              onPointerDown={drag}
              onDblClick={() => {
                setWidth(null);
                store(storeKey, null);
              }}
              onKeyDown={(e) => {
                if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
                e.preventDefault();
                e.stopPropagation();
                resize((width ?? paneWidth()) + (e.key === "ArrowLeft" ? -24 : 24));
              }}
            />
          </>
        )}
      </div>
    </div>
  );
}

/** A preview's top and body: the name, its facts, its buttons, its words, and what's beside them. */
export function ListPreview({ title, facts, actions, children, aside, ...attrs }: { title: ComponentChildren; facts?: ComponentChildren; actions?: ComponentChildren; children?: ComponentChildren; aside?: ComponentChildren; [hook: `data-${string}`]: string | undefined }) {
  return (
    <>
      <header class="list-preview-head" {...attrs}>
        <h2>{title}</h2>
        {facts && <div class="list-preview-facts">{facts}</div>}
        {actions && <div class="list-preview-actions">{actions}</div>}
      </header>
      <div class="list-preview-body">
        <div class="list-preview-words">{children}</div>
        {aside && <div class="list-preview-aside">{aside}</div>}
      </div>
    </>
  );
}
