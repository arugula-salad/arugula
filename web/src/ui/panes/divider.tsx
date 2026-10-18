// Resize handles follow the projected pixel gaps; pointer travel still
// changes daemon-owned split extents in terminal cells.

import type { Client } from "../../client";
import type { Rect, SplitRect } from "../../proto";
import type { Cell } from "../cells";
import { len, px } from "./geometry";
import { PIXELS } from "./chrome-geometry";

/** The hit band, in pixels, where it can't be a whole cell. */
const HIT = 6;

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

  const onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const start = row ? e.clientX : e.clientY;
    const unit = (resizeUnit ?? (row ? cell.width : cell.height)) * scale;
    const base = split.extents.slice();
    let last = 0;
    let timer: number | undefined;
    let pending: number[] | null = null;
    const flush = () => {
      timer = undefined;
      if (pending) client.intent({ op: "resize_split", split: split.id, weights: pending });
      pending = null;
    };
    const move = (ev: PointerEvent) => {
      const d = Math.round(((row ? ev.clientX : ev.clientY) - start) / unit);
      if (d === last) return;
      last = d;
      const ext = base.slice();
      const a = Math.max(1, Math.min(base[index] + d, base[index] + base[index + 1] - 1));
      ext[index] = a;
      ext[index + 1] = base[index] + base[index + 1] - a;
      pending = ext;
      // Live, but not faster than the panes can sensibly redraw.
      timer ??= window.setTimeout(flush, 50);
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      clearTimeout(timer);
      flush();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };

  return (
    <div
      class={`divider live ${row ? "col-resize" : "row-resize"}`}
      data-split={split.id}
      style={style}
      onPointerDown={onPointerDown}
    />
  );
}
