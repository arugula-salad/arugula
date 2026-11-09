// A board's Steps view (Arugula #763, #786): a sequence chart of the board's
// phases (its groups). A row is one line: a title column (its lane's glyph,
// its number and title) and a bar in its step's column; done rows have theirs
// in ✓. Hovering a line (tapping it, on a phone) draws arrows for its links,
// each kind its own way, and fades the rest; the drawer says them in words,
// with the agent and the marks. Dragging a line inside its step moves it
// inside its group (the drawer's ↑ ↓ on a phone). Steps aren't stored: a
// reorder changes a row's place in its group, and the step is worked out
// again from the links. The title column's right edge is a handle: drag it,
// or use the arrow keys, and the step columns share the rest evenly.

import { useEffect, useId, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { BoardRow, BoardView } from "../proto";
import { usePhone } from "../ui/hooks";
import { CheckIcon } from "../ui/icons";
import { isCard, pointerLabel, rowGlyph } from "./text";
import { MOVED_FIRST, TITLE_DEFAULT, clampTitle, lastStep, linksOf, loadTitle, moreLine, phases, reorder, rowsOf, saveTitle, shown, groupOrder, titleRange, type Link, type Reorder } from "./steps";
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

/** How far the grip moves with an arrow key, in pixels. */
const KEY_STEP = 16;

/** Room an arrow's bend takes beside the bars it joins, in pixels. */
const BEND = 22;

interface Box {
  left: number;
  right: number;
  cy: number;
}

/** The path from the bar `a` to the bar `b`: out of the right edge into the left edge of a bar further right, the other way round for one further left, else bent round the right side (both in one column). */
function pathBetween(a: Box, b: Box): string {
  if (b.left >= a.right) {
    const m = (a.right + b.left) / 2;
    return `M${a.right} ${a.cy}C${m} ${a.cy} ${m} ${b.cy} ${b.left} ${b.cy}`;
  }
  if (b.right <= a.left) {
    const m = (a.left + b.right) / 2;
    return `M${a.left} ${a.cy}C${m} ${a.cy} ${m} ${b.cy} ${b.right} ${b.cy}`;
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
}) {
  const phone = usePhone();
  const uid = useId();
  const root = useRef<HTMLDivElement>(null);
  const chart = useRef<HTMLDivElement>(null);
  const ps = phases(view);
  const last = lastStep(ps);
  const everyRow = rowsOf(view);
  /** The phases whose step 1 is unfolded. */
  const [unfolded, setUnfolded] = useState<Set<string>>(new Set());
  const [hover, setHover] = useState<string | null>(null);
  const [pinned, setPinned] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ id: string; phase: string; step: number } | null>(null);
  const [over, setOver] = useState<{ id: string | null; phase: string; step: number; after: boolean } | null>(null);
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

  // The bars on screen, and the links of the row that is hovered or tapped among them. A done row is drawn in ✓ but has no arrows.
  const onScreen = new Set<string>();
  for (const p of ps) {
    const key = p.group?.id ?? "none";
    for (const s of p.steps) for (const r of shown(s.step, s.rows, unfolded.has(key)).rows) onScreen.add(r.id);
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
      return { left: r.left - base.left, right: r.right - base.left, cy: (r.top + r.bottom) / 2 - base.top };
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
  const drop = (r: BoardRow, group: string | null, step: BoardRow[], target: string | null) => {
    setDrag(null);
    setOver(null);
    const m = reorder(orders(group), step.map((x) => x.id), r.id, target);
    if (m) move(r, m);
  };
  const dragged = drag ? everyRow.find((r) => r.id === drag.id) : undefined;
  const canDrag = write && !phone && (view.order?.length ?? 0) > 0;
  const n = last + 1;
  const crit = new Set(view.chain ?? []);

  const line = (r: BoardRow, key: string, step: number, rows: BoardRow[]) => (
    <Line
      key={r.id}
      r={r}
      step={step}
      crit={crit.has(r.id)}
      faded={chain.size > 0 && !chain.has(r.id)}
      selected={active === r.id}
      opened={opened === r.id}
      dragging={drag?.id === r.id}
      drop={over?.id === r.id ? (over.after ? "after" : "before") : undefined}
      draggable={canDrag && step > 0}
      said={behind === r.id}
      hover={setHover}
      tap={() => (phone && pinned !== r.id ? setPinned(r.id) : open(r.id))}
      start={() => setDrag({ id: r.id, phase: key, step })}
      end={() => (setDrag(null), setOver(null))}
      over={(after) => setOver({ id: r.id, phase: key, step, after })}
      accepts={step > 0 && drag?.phase === key && drag.step === step}
      release={(after) => {
        if (!dragged) return;
        const at = rows.findIndex((x) => x.id === r.id);
        drop(dragged, key === "none" ? null : key, rows, after ? (rows[at + 1]?.id ?? null) : r.id);
      }}
    />
  );

  return (
    <div class="steps" ref={root} data-steps data-steps-last={last} style={{ "--lw": `${lw}px`, "--n": String(n) }}>
      <svg class="steps-links" width={drawn.width} height={drawn.height} aria-hidden="true" data-steps-links>
        <defs>
          {[...HEADS].map((k) => (
            <marker key={k} id={`${uid}-${k}`} class={`step-head step-head-${k}`} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto">
              <path d="M0 0L8 4L0 8z" />
            </marker>
          ))}
        </defs>
        {drawn.paths.map(({ link, d }) => (
          <g key={link.key} data-link-from={link.from} data-link-to={link.to}>
            <path class={`step-link step-link-${link.kind}`} d={d} data-link-kind={link.kind} marker-end={HEADS.has(link.kind) ? `url(#${uid}-${link.kind})` : undefined} />
            {link.kind === "duplicate_of" && <path class="step-link-gap" d={d} />}
          </g>
        ))}
      </svg>
      <div class="steps-chart" ref={chart} data-steps-chart>
        <div class="steps-band" aria-hidden="true" data-steps-band />
        <div class="steps-head">
          <div class="steps-lab" />
          <div class="steps-trk">
            {Array.from({ length: n }, (_, i) => (
              <div class="steps-col" key={i} style={{ "--s": String(i) }} data-steps-col={i} aria-label={i === 0 ? "Done" : undefined} data-steps-now={i === 1 ? "" : undefined}>
                {i === 0 ? <CheckIcon size="xs" /> : i}
              </div>
            ))}
          </div>
        </div>
        {ps.map((p) => {
          const key = p.group?.id ?? "none";
          const finished = phone ? [] : p.finished;
          return (
            <section class="steps-phase" key={key} data-steps-phase={key}>
              {(p.group || ps.length > 1) && (
                <div class="steps-line steps-phase-head">
                  <div class="steps-lab">
                    <h2>{p.group?.title ?? "No group"}</h2>
                  </div>
                </div>
              )}
              {p.steps.map((s) => {
                const folded = shown(s.step, s.rows, unfolded.has(key));
                return [
                  ...folded.rows.map((r) => line(r, key, s.step, s.rows)),
                  folded.more > 0 && (
                    <div class="steps-line" key={`more-${s.step}`}>
                      <div class="steps-lab">
                        <button type="button" class="secondary small" data-steps-more={key} onClick={() => setUnfolded(new Set(unfolded).add(key))}>
                          {moreLine(folded.more)}
                        </button>
                      </div>
                    </div>
                  ),
                ];
              })}
              {finished.map((r) => line(r, key, 0, finished))}
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

/** One row's line: the title column (its lane's glyph, its number and title) and its bar, in its step's column (step 0 is ✓: a done row). */
function Line({
  r,
  step,
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
  step: number;
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
  /** Whether a drag in progress is of this line's step, so it may drop here. */
  accepts: boolean;
  release: (after: boolean) => void;
}) {
  const label = pointerLabel(r.pointer, r.live);
  const title = isCard(r.live) ? r.live.title : label || "Note";
  const below = (e: DragEvent) => {
    const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return e.clientY > at.top + at.height / 2;
  };
  // A bar is its lane's colour in step 1 and outlined where the row waits on another; a done row's is faded green.
  const kind = r.lane === "done" ? "done" : step > 1 && r.lane === "next" ? "wait" : r.lane;
  const open = r.lane !== "done";
  return (
    <div
      class="steps-line steps-row"
      data-step-row={r.id}
      data-board-row={r.id}
      data-lane={r.lane}
      data-step={step}
      data-selected={selected ? "" : undefined}
      data-faded={faded ? "" : undefined}
      data-dragging={dragging ? "" : undefined}
      data-drop={drop}
      aria-current={opened ? "true" : undefined}
      role="button"
      tabIndex={0}
      draggable={draggable}
      onPointerEnter={(e) => open && e.pointerType === "mouse" && hover(r.id)}
      onPointerLeave={(e) => open && e.pointerType === "mouse" && hover(null)}
      onFocus={(e) => open && (e.currentTarget as HTMLElement).matches(":focus-visible") && hover(r.id)}
      onBlur={() => open && hover(null)}
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
        <span class="step-bar" data-step-bar={r.id} data-bar={kind} data-crit={crit ? "" : undefined} style={{ "--s": String(step) }} />
      </div>
    </div>
  );
}
