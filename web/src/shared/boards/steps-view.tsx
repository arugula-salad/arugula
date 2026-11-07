// A board's Steps view (Arugula #763): the board's phases (its groups), each
// with its open rows in columns by step, for the people following the work.
// A row is one bar: its title, the agent on it live, its marks. Hovering a
// row (tapping it, on a phone) draws arrows for its links, each kind its own
// way; the drawer says them in words. Dragging a row inside its step moves
// it inside its group (the drawer's ↑ ↓ on a phone). Steps aren't stored: a
// reorder changes a row's place in its group, and the step is worked out
// again from the links.

import { useEffect, useId, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { BoardAttachment, BoardRow, BoardView } from "../proto";
import { StateChip } from "../ui/state-chip";
import { usePhone } from "../ui/hooks";
import { cardTone, isCard, marksOf, pointerLabel, rowGlyph } from "./text";
import { MOVED_FIRST, doneLine, lastStep, linksOf, moreLine, phases, reorder, rowsOf, shown, groupOrder, type Link, type Reorder } from "./steps";
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

/** Room an arrow's bend takes beside the bars it joins, in pixels. */
const BEND = 22;

interface Box {
  left: number;
  right: number;
  cy: number;
}

/** The path from the bar `a` to the bar `b`: out of the right edge into the left edge of a bar further right, the other way round for one further left, else bent round the right side (both in one column, or stacked on a phone). */
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
  chip,
  waiting,
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
  /** An agent block as the board's chip, as a row draws it. */
  chip: (a: BoardAttachment) => preact.ComponentChildren;
  /** What a row waiting on a person shows in place of its agent (#764): nothing yet. */
  waiting?: (r: BoardRow) => preact.ComponentChildren;
}) {
  const phone = usePhone();
  const uid = useId();
  const root = useRef<HTMLDivElement>(null);
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

  // The bars on screen, and the links of the row that is hovered or tapped among them.
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

  /** Measure the bars the arrows join, in the chart's coordinates. It runs after every layout, on a resize and when the chart's size changes (a "+M more" unfolding, the font arriving), so an arrow is never where the bars were. */
  const measure = () => {
    const el = root.current;
    if (!el) return;
    const base = el.getBoundingClientRect();
    const box = (id: string): Box | null => {
      const b = el.querySelector(`[data-step-row="${id}"]`);
      if (!b) return null;
      const r = b.getBoundingClientRect();
      // The chart scrolls sideways: its own coordinates are the page's less the chart's edge, plus what is scrolled.
      const [dx, dy] = [el.scrollLeft - base.left, el.scrollTop - base.top];
      return { left: r.left + dx, right: r.right + dx, cy: (r.top + r.bottom) / 2 + dy };
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
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    window.addEventListener("resize", measure);
    void document.fonts?.ready.then(measure);
    return () => {
      watch.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);

  const orders = (group: string | null) => groupOrder(view, group);
  const drop = (r: BoardRow, group: string | null, step: BoardRow[], target: string | null) => {
    setDrag(null);
    setOver(null);
    const m = reorder(orders(group), step.map((x) => x.id), r.id, target);
    if (m) move(r, m);
  };
  const dragged = drag ? everyRow.find((r) => r.id === drag.id) : undefined;
  const canDrag = write && !phone && (view.order?.length ?? 0) > 0;

  return (
    <div class="steps" ref={root} data-steps data-steps-last={last}>
      <svg class="steps-links" width={drawn.width} height={drawn.height} aria-hidden="true" data-steps-links>
        <defs>
          {[...HEADS].map((k) => (
            <marker key={k} id={`${uid}-${k}`} class={`step-head step-head-${k}`} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" orient="auto">
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
      {ps.map((p) => {
        const key = p.group?.id ?? "none";
        // A phase with nothing open draws only its head.
        const cols = phone || !p.steps.length ? p.steps.map((s) => s.step) : Array.from({ length: last }, (_, i) => i + 1);
        return (
          <section class="steps-phase" key={key} data-steps-phase={key}>
            <header class="steps-phase-head">
              {(p.group || ps.length > 1) && <h2>{p.group?.title ?? "No group"}</h2>}
              {p.done > 0 && (
                <span class="dim" data-steps-done={p.done}>
                  {doneLine(p.done)}
                </span>
              )}
            </header>
            {cols.length > 0 && (
              <div class="steps-cols" style={{ "--steps": String(cols.length) }}>
                {cols.map((n) => {
                  const rows = p.steps.find((s) => s.step === n)?.rows ?? [];
                  const folded = shown(n, rows, unfolded.has(key));
                  const here = drag?.phase === key && drag.step === n;
                  return (
                    <div
                      class="steps-col"
                      key={n}
                      data-steps-col={n}
                      onDragOver={(e) => {
                        if (!here) return;
                        e.preventDefault();
                        setOver({ id: null, phase: key, step: n, after: true });
                      }}
                      onDrop={(e) => {
                        if (!here || !dragged) return;
                        e.preventDefault();
                        drop(dragged, p.group?.id ?? null, rows, null);
                      }}
                    >
                      <h3 class="steps-col-head dim">Step {n}</h3>
                      <ul class="steps-bars">
                        {folded.rows.map((r) => (
                          <Bar
                            key={r.id}
                            r={r}
                            phone={phone}
                            selected={active === r.id}
                            opened={opened === r.id}
                            dragging={drag?.id === r.id}
                            drop={over?.id === r.id ? (over.after ? "after" : "before") : undefined}
                            draggable={canDrag}
                            said={behind === r.id}
                            chip={chip}
                            waiting={waiting}
                            hover={setHover}
                            tap={() => (phone && pinned !== r.id ? setPinned(r.id) : open(r.id))}
                            start={() => setDrag({ id: r.id, phase: key, step: n })}
                            end={() => (setDrag(null), setOver(null))}
                            over={(after) => setOver({ id: r.id, phase: key, step: n, after })}
                            accepts={here}
                            release={(after) => {
                              if (!dragged) return;
                              const at = rows.findIndex((x) => x.id === r.id);
                              drop(dragged, p.group?.id ?? null, rows, after ? (rows[at + 1]?.id ?? null) : r.id);
                            }}
                          />
                        ))}
                        {folded.more > 0 && (
                          <li class="steps-more">
                            <button type="button" class="secondary small" data-steps-more={key} onClick={() => setUnfolded(new Set(unfolded).add(key))}>
                              {moreLine(folded.more)}
                            </button>
                          </li>
                        )}
                      </ul>
                    </div>
                  );
                })}
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
}

/** One row's bar, on one line: its lane's glyph, its number and title, the agent on it (its state live) and its marks. */
function Bar({
  r,
  phone,
  selected,
  opened,
  dragging,
  drop,
  draggable,
  said,
  chip,
  waiting,
  hover,
  tap,
  start,
  end,
  over,
  accepts,
  release,
}: {
  r: BoardRow;
  phone: boolean;
  selected: boolean;
  opened: boolean;
  dragging: boolean;
  drop?: "before" | "after";
  draggable: boolean;
  said: boolean;
  chip: (a: BoardAttachment) => preact.ComponentChildren;
  waiting?: (r: BoardRow) => preact.ComponentChildren;
  hover: (id: string | null) => void;
  tap: () => void;
  start: () => void;
  end: () => void;
  over: (after: boolean) => void;
  /** Whether a drag in progress is of this bar's step, so it may drop here. */
  accepts: boolean;
  release: (after: boolean) => void;
}) {
  const label = pointerLabel(r.pointer, r.live);
  const title = isCard(r.live) ? r.live.title : label || "Note";
  const marks = marksOf(r);
  // The agents at work on it: a block that hasn't closed and wasn't replaced.
  const agents = r.attachments.filter((a) => a.role === "work" && a.block && a.block.state !== "closed" && !marks.earlier.has(a.id));
  const wait = waiting?.(r);
  const below = (e: DragEvent) => {
    const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return e.clientY > at.top + at.height / 2;
  };
  return (
    <li
      class="step-bar"
      data-step-row={r.id}
      data-board-row={r.id}
      data-lane={r.lane}
      data-step={r.step ?? 1}
      data-tone={cardTone(r)}
      data-selected={selected ? "" : undefined}
      data-dragging={dragging ? "" : undefined}
      data-drop={drop}
      aria-current={opened ? "true" : undefined}
      tabIndex={0}
      draggable={draggable}
      onPointerEnter={(e) => e.pointerType === "mouse" && hover(r.id)}
      onPointerLeave={(e) => e.pointerType === "mouse" && hover(null)}
      onFocus={(e) => (e.currentTarget as HTMLElement).matches(":focus-visible") && hover(r.id)}
      onBlur={() => hover(null)}
      onClick={(e) => {
        if ((e.target as Element).closest("a, button")) return;
        tap();
      }}
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
      {r.marks?.some((m) => m.kind === "cycle") && (
        <StateChip tone="warn" data-board-mark="cycle" title="This row and others wait on each other, so they share a step">
          cycle
        </StateChip>
      )}
      {marks.stalled && (
        <StateChip tone="warn" data-board-mark="stalled" title="Every agent on this row has been idle past the project's limit, and no pull request is open">
          {marks.stalled.words}
        </StateChip>
      )}
      {marks.blocked.slice(0, phone ? 1 : 2).map((m, i) => (
        <StateChip key={i} tone="accent" data-board-mark="blocked_by" title={m.title ?? "You can't read the item that blocks this one"}>
          {m.words}
        </StateChip>
      ))}
      {wait ?? (agents.length > 0 && <span class="step-agent" data-step-agent>{chip(agents[0])}</span>)}
    </li>
  );
}
