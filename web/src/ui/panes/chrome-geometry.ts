// Project daemon-owned cell rectangles into pixels, reserving actual space
// for headers. Terminal cells and the split weights stay daemon-owned.
import type { Layout, Rect, SplitRect } from "../../proto";
import type { Cell } from "../cells";

export const HEADER_HEIGHT = 34;
export const PANE_GAP = 6;
export const PIXELS: Cell = { width: 1, height: 1 };

type Branch = { rect: Rect; overhead: number; split?: SplitRect; children?: Branch[]; pane?: number };
const same = (a: Rect, b: Rect) => a.x === b.x && a.y === b.y && a.cols === b.cols && a.rows === b.rows;

/** Horizontal siblings share the tallest chrome budget; vertical siblings
 * add theirs. Any spare height in a shorter branch stays below its terminal,
 * never covers or removes a cell. This also handles asymmetric split trees. */
export function paneChrome(layout: Layout, cols: number, rows: number, cell: Cell) {
  const header = Math.max(HEADER_HEIGHT, cell.height);
  function branch(rect: Rect): Branch {
    const split = layout.splits.find((s) => same(s.rect, rect));
    if (!split) return { rect, overhead: header, pane: layout.panes.find(([, r]) => same(r, rect))?.[0] };
    let at = 0;
    const children = split.extents.map((extent) => {
      const r = split.dir === "row"
        ? { ...rect, x: rect.x + at, cols: extent }
        : { ...rect, y: rect.y + at, rows: extent };
      at += extent + 1;
      return branch(r);
    });
    const overhead = split.dir === "row"
      ? Math.max(...children.map((c) => c.overhead))
      : children.reduce((sum, c) => sum + c.overhead, 0) + (children.length - 1) * (PANE_GAP - cell.height);
    return { rect, split, children, overhead };
  }
  const root = branch({ x: 0, y: 0, cols, rows });
  const panes = new Map<number, Rect>();
  const frames = new Map<number, Rect>();
  const dividers = new Map<string, Rect>();
  const resizeUnits = new Map<string, number>();
  function draw(node: Branch, x: number, y: number, height: number) {
    const width = node.rect.cols * cell.width;
    if (!node.split || !node.children) {
      if (node.pane !== undefined) {
        frames.set(node.pane, { x, y, cols: width, rows: height });
        panes.set(node.pane, { x, y: y + header, cols: width, rows: height - header });
      }
      return;
    }
    const horizontal = node.split.dir === "row";
    const spare = height - node.rect.rows * cell.height - node.overhead;
    const totalRows = node.children.reduce((sum, c) => sum + c.rect.rows, 0);
    let cursor = horizontal ? x : y;
    node.children.forEach((child, i) => {
      const h = horizontal ? height : child.rect.rows * cell.height + child.overhead + spare * child.rect.rows / totalRows;
      draw(child, horizontal ? cursor : x, horizontal ? y : cursor, h);
      cursor += horizontal ? child.rect.cols * cell.width : h;
      if (i < node.children!.length - 1) {
        // Hit targets occupy the real gap; terminal mouse input is untouched.
        resizeUnits.set(`${node.split!.id}-${i}`, horizontal ? cell.width : cell.height + spare / totalRows);
        dividers.set(`${node.split!.id}-${i}`, horizontal
          ? { x: cursor, y, cols: cell.width, rows: height }
          : { x, y: cursor, cols: width, rows: PANE_GAP });
        cursor += horizontal ? cell.width : PANE_GAP;
      }
    });
  }
  draw(root, 0, 0, rows * cell.height + root.overhead);
  return { panes, frames, dividers, resizeUnits, overhead: root.overhead, header, width: cols * cell.width, height: rows * cell.height + root.overhead };
}
