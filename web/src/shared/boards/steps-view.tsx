// A board's Steps view (Arugula #763, #786, #824): a sequence chart of the
// board's phases (its groups). A row is one line: a title column (its lane's
// glyph, its number and title) and a bar in its place's column. Done rows
// stay in their place, with the title struck through, so finishing work
// doesn't move the chart; the arrows out of a done row are green, for what
// they waited on is met. Which rows are ready now is the bars' look, not a
// column: a ready row's bar is filled, a waiting row's outlined. Hovering a line (tapping it, on a phone) draws arrows for its links,
// each kind its own way, and fades the rest; the drawer says them in words,
// with the agent and the marks. Dragging a line inside its column moves it
// inside its group (the drawer's ↑ ↓ on a phone). Steps aren't stored: a
// reorder changes a row's position in its group, and the place is worked out
// again from the links. The title column's right edge is a handle: drag it,
// or use the arrow keys, and the columns share the rest evenly.

import { useEffect, useId, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { BoardRow, BoardView } from "../proto";
import { usePhone } from "../ui/hooks";
import { isCard, pointerLabel, rowGlyph } from "./text";
import { MOVED_FIRST, TITLE_DEFAULT, clampTitle, columns, lastPlace, linksOf, loadTitle, moreLine, phases, reorder, stepOf, rowsOf, saveTitle, shown, groupOrder, titleRange, withoutDone, type Link, type Reorder } from "./steps";
import "./steps.css";

/** One arrow, measured: where it runs, in the chart's own coordinates. */
interface Drawn {
  link: Link;
  d: string;
}

/** What the measuring found, so a render that finds the same sets nothing. */
interface Measured {
  paths: Drawn[];
  width: number;
  height: number;
}

/** The kinds that point (a head), and those that don't. */
const HEADS = new Set(["before", "blocks", "discovered_from"]);

/** The kinds that make a row wait on another: an arrow of these out of a done row is met, and drawn green. */
const WAITS = new Set(["before", "blocks"]);

/** How far the grip moves with an arrow key, in pixels. */
const KEY_STEP = 16;

/** Room an arrow's bend takes beside the bars it joins, in pixels. */
const BEND = 22;
/** How far inside its outer edge an elbow leaves its bar: with the 6px between neighbouring columns, the run into the next bar is long enough to carry its head. */
const IN = 10;
/** The elbow's rounded corner. */
const TURN = 6;

interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
  cy: number;
}

/** The path from the bar `a` to the bar `b`. A bar in another column gets an elbow: down out of `a`'s bottom (up out of its top for a row above), a little inside the edge facing `b`, then a turn and a run into `b`'s near side, so the head lies on the line and points into the bar. Two bars in one column are joined by a bend round their right side. */
function pathBetween(a: Box, b: Box): string {
  const dir = b.left >= a.right ? 1 : b.right <= a.left ? -1 : 0;
  if (dir) {
    const sx = dir > 0 ? Math.max(a.left + (a.right - a.left) / 2, a.right - IN) : Math.min(a.left + (a.right - a.left) / 2, a.left + IN);
    const down = b.cy > a.cy;
    const sy = down ? a.bottom : a.top;
    const ex = dir > 0 ? b.left : b.right;
    const r = Math.min(TURN, Math.abs(b.cy - sy) / 2, Math.abs(ex - sx) / 2);
    const vy = down ? b.cy - r : b.cy + r;
    return `M${sx} ${sy}V${vy}Q${sx} ${b.cy} ${sx + dir * r} ${b.cy}H${ex}`;
  }
  const x = Math.max(a.right, b.right);
  return `M${a.right} ${a.cy}C${x + BEND} ${a.cy} ${x + BEND} ${b.cy} ${b.right} ${b.cy}`;
}

export function StepsView({
  view,
  write,
  open,
  opened,
  move,
  behind,
  showDone,
}: {
  view: BoardView;
  /** Whether the reader may reorder (the platform refuses a viewer's move). */
  write: boolean;
  open: (row: string) => void;
  /** The row whose drawer is open. */
  opened: string | null;
  /** A reorder of a row; the view makes none itself. */
  move: (row: BoardRow, m: Reorder) => void;
  /** The row whose reorder was refused because someone else had moved its group first. */
  behind: string | null;
  /** Whether done rows are drawn (#831). Hidden, their columns close up: a bar's column is then not its place. */
  showDone: boolean;
}) {
  const phone = usePhone();
  const uid = useId();
  const root = useRef<HTMLDivElement>(null);
  const chart = useRef<HTMLDivElement>(null);
  const all = phases(view);
  const ps = showDone ? all : withoutDone(all);
  /** Each place drawn to its column: with done rows shown, the place itself (#824). */
  const col = showDone ? new Map(ps.flatMap((p) => p.places.map((s) => [s.place, s.place] as const))) : columns(ps.flatMap((p) => p.places.map((s) => s.place)));
  const last = showDone ? lastPlace(ps) : Math.max(1, col.size);
  const everyRow = rowsOf(view);
  /** The phases whose column 1 is unfolded. */
  const [unfolded, setUnfolded] = useState<Set<string>>(new Set());
  const [hover, setHover] = useState<string | null>(null);
  const [pinned, setPinned] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ id: string; phase: string; place: number } | null>(null);
  const [over, setOver] = useState<{ id: string | null; phase: string; place: number; after: boolean } | null>(null);
  const [drawn, setDrawn] = useState<Measured>({ paths: [], width: 0, height: 0 });
  const was = useRef("");
  /** The chart's width, and the title column's as the viewer set it (null: the default). */
  const [span, setSpan] = useState(0);
  const [set, setSet] = useState<number | null>(loadTitle);
  const lw = clampTitle(set ?? (phone ? TITLE_DEFAULT.phone : TITLE_DEFAULT.desktop), span, phone);
  const [min, max] = titleRange(span, phone);
  const grab = useRef<{ x: number; width: number } | null>(null);
  const lwNow = useRef(lw);
  lwNow.current = lw;

  // The bars on screen, and the links of the row that is hovered or tapped among them. A done row has arrows too.
  const onScreen = new Set<string>();
  for (const p of ps) {
    const key = p.group?.id ?? "none";
    for (const s of p.places) for (const r of shown(col.get(s.place)!, s.rows, unfolded.has(key)).rows) onScreen.add(r.id);
  }
  const active = hover ?? pinned;
  const row = active ? everyRow.find((r) => r.id === active) : undefined;
  const links = row && onScreen.has(row.id) ? linksOf(row, everyRow, onScreen) : [];
  const linksNow = useRef(links);
  linksNow.current = links;
  /** The rows the arrows join: the others fade while a row is held. */
  const chain = new Set(row && onScreen.has(row.id) ? [row.id, ...links.flatMap((l) => [l.from, l.to])] : []);

  /** Measure the bars the arrows join, in the chart's coordinates. It runs after every layout (so after the title column's width changes, which resizes no box the observer watches), on a resize and when the chart's size changes (a "+M more" unfolding, the font arriving), so an arrow is never where the bars were. */
  const measure = () => {
    const el = root.current;
    if (!el) return;
    const base = el.getBoundingClientRect();
    const box = (id: string): Box | null => {
      const b = el.querySelector(`[data-step-bar="${id}"]`);
      if (!b) return null;
      const r = b.getBoundingClientRect();
      return { left: r.left - base.left, right: r.right - base.left, top: r.top - base.top, bottom: r.bottom - base.top, cy: (r.top + r.bottom) / 2 - base.top };
    };
    const paths = linksNow.current.flatMap((link) => {
      const [a, b] = [box(link.from), box(link.to)];
      return a && b ? [{ link, d: pathBetween(a, b) }] : [];
    });
    const next: Measured = { paths, width: el.scrollWidth, height: el.scrollHeight };
    const sig = JSON.stringify(next);
    if (sig === was.current) return;
    was.current = sig;
    setDrawn(next);
  };
  useLayoutEffect(measure);
  useLayoutEffect(() => {
    if (chart.current) setSpan(chart.current.clientWidth);
  });
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    const watch = new ResizeObserver(() => {
      measure();
      if (chart.current) setSpan(chart.current.clientWidth);
    });
    watch.observe(el);
    window.addEventListener("resize", measure);
    void document.fonts?.ready.then(measure);
    return () => {
      watch.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);

  /** The title column at `width`, kept for next time when `keep`. */
  const widen = (width: number, keep: boolean) => {
    const w = clampTitle(width, chart.current?.clientWidth ?? span, phone);
    setSet(w);
    lwNow.current = w;
    if (keep) saveTitle(w);
  };

  const orders = (group: string | null) => groupOrder(view, group);
  const drop = (r: BoardRow, group: string | null, column: BoardRow[], target: string | null) => {
    setDrag(null);
    setOver(null);
    const m = reorder(orders(group), column.map((x) => x.id), r.id, target);
    if (m) move(r, m);
  };
  const dragged = drag ? everyRow.find((r) => r.id === drag.id) : undefined;
  const canDrag = write && !phone && (view.order?.length ?? 0) > 0;
  const n = last;
  const done = new Set(everyRow.filter((r) => r.lane === "done").map((r) => r.id));
  const crit = new Set(view.chain ?? []);

  /** `rows`: the open rows of the column, which a drag moves among. */
  const line = (r: BoardRow, key: string, place: number, rows: BoardRow[]) => (
    <Line
      key={r.id}
      r={r}
      place={place}
      crit={crit.has(r.id)}
      faded={chain.size > 0 && !chain.has(r.id)}
      selected={active === r.id}
      opened={opened === r.id}
      dragging={drag?.id === r.id}
      drop={over?.id === r.id ? (over.after ? "after" : "before") : undefined}
      draggable={canDrag && r.lane !== "done"}
      said={behind === r.id}
      hover={setHover}
      tap={() => (phone && pinned !== r.id ? setPinned(r.id) : open(r.id))}
      start={() => setDrag({ id: r.id, phase: key, place })}
      end={() => (setDrag(null), setOver(null))}
      over={(after) => setOver({ id: r.id, phase: key, place, after })}
      accepts={r.lane !== "done" && drag?.phase === key && drag.place === place}
      release={(after) => {
        if (!dragged) return;
        const at = rows.findIndex((x) => x.id === r.id);
        drop(dragged, key === "none" ? null : key, rows, after ? (rows[at + 1]?.id ?? null) : r.id);
      }}
    />
  );

  /** An arrow out of a done row, of a kind that makes a row wait: what it waited on is met. */
  const met = (l: Link) => WAITS.has(l.kind) && done.has(l.from);
  const markerOf = (l: Link) => (met(l) ? `url(#${uid}-met)` : HEADS.has(l.kind) ? `url(#${uid}-${l.kind})` : undefined);

  return (
    <div class="steps" ref={root} data-steps data-steps-last={last} style={{ "--lw": `${lw}px`, "--n": String(n) }}>
      <svg class="steps-links" width={drawn.width} height={drawn.height} aria-hidden="true" data-steps-links>
        <defs>
          {[...HEADS].map((k) => (
            <marker key={k} id={`${uid}-${k}`} class={`step-head step-head-${k}`} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto">
              <path d="M0 0L8 4L0 8z" />
            </marker>
          ))}
          <marker id={`${uid}-met`} class="step-head step-head-met" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto">
            <path d="M0 0L8 4L0 8z" />
          </marker>
        </defs>
        {drawn.paths.map(({ link, d }) => (
          <g key={link.key} data-link-from={link.from} data-link-to={link.to}>
            <path class={`step-link step-link-${link.kind}${met(link) ? " step-link-met" : ""}`} d={d} data-link-kind={link.kind} data-link-met={met(link) ? "" : undefined} marker-end={markerOf(link)} />
            {link.kind === "duplicate_of" && <path class="step-link-gap" d={d} />}
          </g>
        ))}
      </svg>
      <div class="steps-chart" ref={chart} data-steps-chart>
        <div class="steps-head">
          <div class="steps-lab" />
          <div class="steps-trk">
            {Array.from({ length: n }, (_, i) => (
              <div class="steps-col" key={i} style={{ "--s": String(i) }} data-steps-col={i}>
                {i + 1}
              </div>
            ))}
          </div>
        </div>
        {ps.map((p) => {
          const key = p.group?.id ?? "none";
          return (
            <section class="steps-phase" key={key} data-steps-phase={key}>
              {(p.group || ps.length > 1) && (
                <div class="steps-line steps-phase-head">
                  <div class="steps-lab">
                    <h2>{p.group?.title ?? "No group"}</h2>
                    {!showDone && p.done > 0 && (
                      <span class="dim" data-steps-done>
                        {p.done} done
                      </span>
                    )}
                  </div>
                </div>
              )}
              {p.places.map((s) => {
                const at = col.get(s.place)!;
                const folded = shown(at, s.rows, unfolded.has(key));
                const open = s.rows.filter((r) => r.lane !== "done");
                return [
                  ...folded.rows.map((r) => line(r, key, at, open)),
                  folded.more > 0 && (
                    <div class="steps-line" key={`more-${at}`}>
                      <div class="steps-lab">
                        <button type="button" class="secondary small" data-steps-more={key} onClick={() => setUnfolded(new Set(unfolded).add(key))}>
                          {moreLine(folded.more)}
                        </button>
                      </div>
                    </div>
                  ),
                ];
              })}
            </section>
          );
        })}
        <div
          class="steps-grip"
          role="separator"
          aria-orientation="vertical"
          aria-label="Title column width"
          aria-valuenow={lw}
          aria-valuemin={min}
          aria-valuemax={Number.isFinite(max) ? max : undefined}
          tabIndex={0}
          data-steps-grip
          onPointerDown={(e) => {
            e.preventDefault();
            e.currentTarget.focus();
            e.currentTarget.setPointerCapture(e.pointerId);
            grab.current = { x: e.clientX, width: lw };
          }}
          onPointerMove={(e) => grab.current && widen(grab.current.width + e.clientX - grab.current.x, false)}
          onPointerUp={() => {
            if (!grab.current) return;
            grab.current = null;
            saveTitle(lwNow.current);
          }}
          onPointerCancel={() => (grab.current = null)}
          onKeyDown={(e) => {
            if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
            e.preventDefault();
            widen(lw + (e.key === "ArrowRight" ? KEY_STEP : -KEY_STEP), true);
          }}
          onDblClick={() => {
            setSet(null);
            saveTitle(null);
          }}
        />
      </div>
    </div>
  );
}

/** One row's line: the title column (its lane's glyph, its number and title; struck through when done) and its bar, in its place's column. */
function Line({
  r,
  place,
  crit,
  faded,
  selected,
  opened,
  dragging,
  drop,
  draggable,
  said,
  hover,
  tap,
  start,
  end,
  over,
  accepts,
  release,
}: {
  r: BoardRow;
  /** The column the bar is in. */
  place: number;
  /** On the longest open chain. */
  crit: boolean;
  /** Not on the chain of the row held, which is drawn. */
  faded: boolean;
  selected: boolean;
  opened: boolean;
  dragging: boolean;
  drop?: "before" | "after";
  draggable: boolean;
  said: boolean;
  hover: (id: string | null) => void;
  tap: () => void;
  start: () => void;
  end: () => void;
  over: (after: boolean) => void;
  /** Whether a drag in progress is of this line's column, so it may drop here. */
  accepts: boolean;
  release: (after: boolean) => void;
}) {
  const label = pointerLabel(r.pointer, r.live);
  const title = isCard(r.live) ? r.live.title : label || "Note";
  const below = (e: DragEvent) => {
    const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return e.clientY > at.top + at.height / 2;
  };
  // A bar is its lane's colour when the row is ready (step 1) and outlined where it waits on an open row; a done row's is faded green. The column says where the row sits in the plan, not whether it is ready.
  const step = stepOf(r);
  const kind = r.lane === "done" ? "done" : step > 1 && r.lane === "next" ? "wait" : r.lane;
  return (
    <div
      class="steps-line steps-row"
      data-step-row={r.id}
      data-board-row={r.id}
      data-lane={r.lane}
      data-step={step}
      data-place={place}
      data-selected={selected ? "" : undefined}
      data-faded={faded ? "" : undefined}
      data-dragging={dragging ? "" : undefined}
      data-drop={drop}
      aria-current={opened ? "true" : undefined}
      role="button"
      tabIndex={0}
      draggable={draggable}
      onPointerEnter={(e) => e.pointerType === "mouse" && hover(r.id)}
      onPointerLeave={(e) => e.pointerType === "mouse" && hover(null)}
      onFocus={(e) => (e.currentTarget as HTMLElement).matches(":focus-visible") && hover(r.id)}
      onBlur={() => hover(null)}
      onClick={() => tap()}
      onKeyDown={(e) => e.key === "Enter" && e.target === e.currentTarget && (e.preventDefault(), tap())}
      onDragStart={(e) => {
        e.dataTransfer?.setData("text/plain", r.id);
        if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
        start();
      }}
      onDragEnd={end}
      onDragOver={(e) => {
        if (!accepts) return;
        e.preventDefault();
        e.stopPropagation();
        over(below(e));
      }}
      onDrop={(e) => {
        if (!accepts) return;
        e.preventDefault();
        e.stopPropagation();
        release(below(e));
      }}
    >
      <div class="steps-lab">
        <span class="step-glyph" aria-hidden="true">
          {rowGlyph(r)}
        </span>
        {label && <span class="step-label dim">{label}</span>}
        <span class="step-title" data-step-title>
          {title}
        </span>
        {said && (
          <span class="step-said" role="status" data-step-moved-first>
            {MOVED_FIRST}
          </span>
        )}
      </div>
      <div class="steps-trk">
        <span class="step-bar" data-step-bar={r.id} data-bar={kind} data-crit={crit ? "" : undefined} style={{ "--s": String(place - 1) }} />
      </div>
    </div>
  );
}
