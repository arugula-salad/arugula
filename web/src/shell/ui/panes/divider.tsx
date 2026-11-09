// Resize handles follow the projected pixel gaps; pointer travel still
// changes daemon-owned split extents in terminal cells.

import { useEffect, useLayoutEffect, useRef } from "preact/hooks";
import type { Client } from "../../client";
import type { Rect, SplitRect } from "../../../shared/proto";
import type { Cell } from "../cells";
import { len, px } from "./geometry";
import { PIXELS } from "./chrome-geometry";

/** The hit band, in pixels, where it can't be a whole cell. */
const HIT = 6;

/** How long the pointer is still before the terminals are resized, in ms. */
const SETTLE = 150;

/** How long a released divider waits for the daemon's layout, in ms. */
const SNAP_BACK = 1500;

export function Divider({
  client,
  split,
  index,
  cell,
  scale,
  projected,
  resizeUnit,
}: {
  client: Client;
  split: SplitRect;
  index: number;
  cell: Cell;
  scale: number;
  projected?: Rect;
  resizeUnit?: number;
}) {
  const row = split.dir === "row";
  const at = split.extents.slice(0, index + 1).reduce((a, b) => a + b, 0) + index;
  const r = split.rect;
  const cellStyle = row
    ? px({ x: r.x + at, y: r.y, cols: 1, rows: r.rows }, cell)
    : px({ x: r.x, y: r.y + at, cols: r.cols, rows: 1 }, cell);
  const style = projected ? px(projected, PIXELS) : row
    ? { ...cellStyle, left: len((r.x + at) * cell.width + Math.min(0, (cell.width - HIT) / 2)), width: len(Math.max(cell.width, HIT)) }
    : { ...cellStyle, top: len((r.y + at) * cell.height - HIT / 2), height: len(HIT) };

  // The divider follows the pointer at once (a transform on the handle);
  // the panes, and the terminals in them, follow the daemon's layout. The
  // preview is an offset from where the layout has the divider, so a layout
  // arriving mid-drag moves the handle's base without a jump.
  const el = useRef<HTMLDivElement>(null);
  const preview = useRef<{ to: number | null; released: string | null; timer?: number; here: number; key: string }>({ to: null, released: null, here: 0, key: "" });
  const here = split.extents[index];
  const key = split.extents.join();
  const per = resizeUnit ?? (row ? cell.width : cell.height);
  preview.current.here = here;
  preview.current.key = key;
  useLayoutEffect(() => {
    const p = preview.current;
    if (!el.current) return;
    // Released and the layout has since changed (or already agrees): the
    // daemon's layout wins.
    if (p.to !== null && p.released !== null && (key !== p.released || here === p.to)) {
      p.to = null;
      p.released = null;
      clearTimeout(p.timer);
    }
    const delta = p.to === null ? 0 : (p.to - here) * per;
    el.current.style.transform = delta ? (row ? `translateX(${delta}px)` : `translateY(${delta}px)`) : "";
  });
  useEffect(() => () => clearTimeout(preview.current.timer), []);

  const onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const start = row ? e.clientX : e.clientY;
    const unit = per * scale;
    const base = split.extents.slice();
    const p = preview.current;
    clearTimeout(p.timer);
    p.released = null;
    let last = 0;
    let timer: number | undefined;
    let pending: number[] | null = null;
    let sent = "";
    const flush = () => {
      timer = undefined;
      if (pending && pending.join() !== sent) {
        sent = pending.join();
        client.intent({ op: "resize_split", split: split.id, weights: pending });
      }
      pending = null;
    };
    const settle = () => {
      clearTimeout(timer);
      timer = window.setTimeout(flush, SETTLE);
    };
    const move = (ev: PointerEvent) => {
      // Any movement, even within a cell, is the pointer not yet still.
      if (pending) settle();
      const d = Math.round(((row ? ev.clientX : ev.clientY) - start) / unit);
      if (d === last) return;
      last = d;
      const ext = base.slice();
      const a = Math.max(1, Math.min(base[index] + d, base[index] + base[index + 1] - 1));
      ext[index] = a;
      ext[index + 1] = base[index] + base[index + 1] - a;
      pending = ext;
      p.to = a;
      const delta = (a - preview.current.here) * per;
      handle.style.transform = delta ? (row ? `translateX(${delta}px)` : `translateY(${delta}px)`) : "";
      // Resize the terminals once the pointer is still, not at every step.
      settle();
    };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
      clearTimeout(timer);
      flush();
      // Hold the preview until the daemon's layout arrives; if it never
      // changes (a refusal), snap back to it.
      if (p.to === null || p.to === preview.current.here) p.to = null;
      else {
        p.released = preview.current.key;
        p.timer = window.setTimeout(() => {
          p.to = null;
          p.released = null;
          if (el.current) el.current.style.transform = "";
        }, SNAP_BACK);
      }
      if (el.current && p.to === null) el.current.style.transform = "";
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  };

  return (
    <div
      ref={el}
      class={`divider live ${row ? "col-resize" : "row-resize"}`}
      data-split={split.id}
      style={style}
      onPointerDown={onPointerDown}
    />
  );
}
