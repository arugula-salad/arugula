// Tabs and panes (design lift §4.1, part 11; §3.12; Arugula #1497): a
// place's panes beside each other. Its default panes (a board's lane
// groups) share the first column and are quiet: no title row of their own
// until something opens beside them, and then they compact. Each opened
// pane gets a column, up to three; past that, or past what the width holds
// at `min` each, they stack in the last one. A default pane keeps its own
// heading, with its controls over its corner on hover. Each opened pane has a title
// row with its kind, its title, who opened it and its state, plus controls
// that show on hover or focus: move to its own tab, ⤢ and ×. Dividers are
// hairlines you drag; `max` fills the place with one pane. Machines' panes
// (Lift 6a) draw through the same part.

import type { ComponentChildren } from "preact";
import { useLayoutEffect, useRef, useState } from "preact/hooks";
import { CloseIcon, ExpandIcon, IconButton, TabsIcon } from "../../ui/icons";
import type { State } from "../state-mark";
import { dragLine } from "./drag-line";
import { PaneHead } from "./pane-head";
import { len } from "./geometry";
import "../kit.css";

/** The most opened panes that get a column of their own. */
export const COLUMNS = 3;

export interface PaneItem {
  id: string;
  title: string;
  icon?: ComponentChildren;
  /** After the title: who opened it, what it stands in place of. */
  meta?: ComponentChildren;
  /** Its state, with its word. */
  state?: { state: State; word?: string };
  /** A default pane of the place: no title row of its own. */
  default?: boolean;
  /** An agent opened it while you were busy: its dot, until you look. */
  dot?: boolean;
  /** It just arrived: it slides in (not when it has a dot). */
  arriving?: boolean;
  /** Its source is offline: its last state, dim. */
  dim?: boolean;
  /** Its share of the place's width, in thousandths. */
  size?: number;
  /** Its controls; each one missing isn't shown. */
  move?: () => void;
  max?: () => void;
  close?: () => void;
  /** More controls before those (↺, Open). */
  extra?: ComponentChildren;
  /** Over its body, under its title row: what it asks you (a prompt's question, a plain `claude`'s ask). */
  above?: ComponentChildren;
  body: ComponentChildren;
  data?: Record<string, string | undefined>;
}

/** The columns for `defaults` and `opened`: the defaults' first (while any shows), then a column for each opened pane up to `COLUMNS`, the rest stacked in the last; never more than `fit` columns. */
export function columnsFor<T>(defaults: T[], opened: T[], fit: number): T[][] {
  const most = Math.max(1, Math.min(fit, COLUMNS + (defaults.length ? 1 : 0)));
  const cols: T[][] = defaults.length ? [defaults] : [];
  for (const p of opened) {
    if (cols.length < most) cols.push([p]);
    else cols[cols.length - 1]!.push(p);
  }
  return cols;
}

export function Panes({ label, defaults, opened, focused, max, min, onFocus, onResize }: { label: string; defaults: PaneItem[]; opened: PaneItem[]; focused: string | null; max: string | null; min: number; onFocus: (id: string) => void; onResize: (id: string, size: number) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current!;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);
  /** A divider being dragged: the column before it, and its width now. */
  const [dragging, setDragging] = useState<{ col: number; px: number } | null>(null);
  const all = [...defaults, ...opened];
  const maxed = max ? all.find((p) => p.id === max) : undefined;
  const cols = maxed ? [[maxed]] : columnsFor(defaults, opened, width ? Math.floor(width / min) : COLUMNS + 1);
  const beside = opened.length > 0 && !maxed;
  const many = all.length > 1;
  const grow = (col: number, e: PointerEvent) => {
    const el = (e.currentTarget as HTMLElement).previousElementSibling as HTMLElement | null;
    const start = el?.getBoundingClientRect().width ?? 0;
    const first = cols[col]![0]!;
    let at = start;
    dragLine(e, "row", {
      move: (d) => setDragging({ col, px: (at = Math.max(min, Math.min(width - min, start + d))) }),
      done: () => {
        setDragging(null);
        if (width && at !== start) onResize(first.id, Math.round((at / width) * 1000));
      },
    });
  };
  return (
    <div ref={ref} class={beside ? "k-panes is-beside" : "k-panes"} role="group" aria-label={label} data-kit="panes" data-panes-columns={cols.length}>
      {cols.map((col, i) => {
        const size = dragging?.col === i ? undefined : col[0]!.size;
        const style = dragging?.col === i ? { flex: `0 0 ${len(dragging.px)}` } : size && cols.length > 1 ? { flex: `0 0 ${size / 10}%` } : undefined;
        return [
          i > 0 && !maxed ? <div key={`d${i}`} class="k-panes-divider" role="separator" aria-orientation="vertical" data-panes-divider={i - 1} onPointerDown={(e) => grow(i - 1, e)} /> : null,
          <div key={col[0]!.id} class="k-panes-col" style={{ ...style, minWidth: len(Math.min(min, width || min)) }} data-panes-col={i}>
            {col.map((p) => (
              <PaneFrame key={p.id} p={p} focused={focused === p.id && many} compact={!!p.default && beside} onFocus={() => onFocus(p.id)} />
            ))}
          </div>,
        ];
      })}
    </div>
  );
}

function PaneFrame({ p, focused, compact, onFocus }: { p: PaneItem; focused: boolean; compact: boolean; onFocus: () => void }) {
  const cls = ["k-pane", p.default && "is-default", focused && "is-focused", compact && "is-compact", p.dim && "is-dim", p.arriving && !p.dot && "is-arriving"].filter(Boolean).join(" ");
  return (
    <section class={cls} aria-label={p.title} data-pane-id={p.id} data-pane-focused={focused ? "" : undefined} data-pane-compact={compact ? "" : undefined} onPointerDown={onFocus} {...p.data}>
      <PaneHead
        icon={p.default ? undefined : p.icon}
        title={p.default ? undefined : p.title}
        meta={p.default ? undefined : p.meta}
        dot={!p.default && p.dot}
        state={p.default ? undefined : p.state}
        actions={
          <span class="k-pane-controls">
            {p.extra}
            {p.move && (
              <IconButton label={`Move ${p.title} to its own tab`} class="bar-btn sq" data-pane-move onClick={p.move}>
                <TabsIcon size="sm" />
              </IconButton>
            )}
            {p.max && (
              <IconButton label={`Maximise ${p.title}`} class="bar-btn sq" data-pane-max onClick={p.max}>
                <ExpandIcon size="sm" />
              </IconButton>
            )}
            {p.close && (
              <IconButton label={`Close ${p.title}`} class="bar-btn sq" data-pane-close onClick={p.close}>
                <CloseIcon size="sm" />
              </IconButton>
            )}
          </span>
        }
      />
      {p.above && <div class="k-pane-above">{p.above}</div>}
      <div class="k-pane-body">{p.body}</div>
    </section>
  );
}
