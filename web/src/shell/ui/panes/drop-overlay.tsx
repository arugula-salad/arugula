// What a drag shows: the half of a pane a drop would take, and the label
// following the pointer.

import type { Client } from "../../client";
import type { Edge, Rect, TabView } from "../../../shared/proto";
import type { Cell } from "../cells";
import { drag } from "../drag";
import { useSubscribe } from "../../../shared/ui/hooks";
import { dropAllowed } from "./drop";
import { len, px } from "./geometry";

export function DropOverlay({ client, tab, cell }: { client: Client; tab: TabView; cell: Cell }) {
  useSubscribe(drag.subscribe);
  const d = drag.current;
  const t = d?.target;
  if (!d || t?.kind !== "pane" || !dropAllowed(client, d.what, t)) return null;
  const rect = tab.layout.panes.find(([id]) => id === t.pane)?.[1];
  if (!rect) return null;
  const half = (r: Rect, edge: Edge): Rect => {
    const w = Math.max(1, Math.floor(r.cols / 2));
    const h = Math.max(1, Math.floor(r.rows / 2));
    switch (edge) {
      case "left":
        return { ...r, cols: w };
      case "right":
        return { ...r, x: r.x + r.cols - w, cols: w };
      case "top":
        return { ...r, rows: h };
      case "bottom":
        return { ...r, y: r.y + r.rows - h, rows: h };
      default:
        return r;
    }
  };
  return <div class="drop-zone" style={px(half(rect, t.edge), cell)} />;
}

export function DragGhost() {
  useSubscribe(drag.subscribe);
  const d = drag.current;
  if (!d) return null;
  return (
    <div class="drag-ghost" style={{ left: len(d.x + 14), top: len(d.y + 14) }}>
      {d.label}
    </div>
  );
}
