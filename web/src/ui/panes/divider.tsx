// The one-cell gutter between two panes of a split: dragging it resizes
// them.

import type { Client } from "../../client";
import type { SplitRect } from "../../proto";
import type { Cell } from "../cells";
import { px } from "./geometry";

export function Divider({
  client,
  split,
  index,
  cell,
  scale,
}: {
  client: Client;
  split: SplitRect;
  index: number;
  cell: Cell;
  scale: number;
}) {
  const row = split.dir === "row";
  const at = split.extents.slice(0, index + 1).reduce((a, b) => a + b, 0) + index;
  const r = split.rect;
  const style = row
    ? px({ x: r.x + at, y: r.y, cols: 1, rows: r.rows }, cell)
    : px({ x: r.x, y: r.y + at, cols: r.cols, rows: 1 }, cell);

  const onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const start = row ? e.clientX : e.clientY;
    const unit = (row ? cell.width : cell.height) * scale;
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

  return <div class={row ? "divider col-resize" : "divider row-resize"} style={style} onPointerDown={onPointerDown} />;
}
