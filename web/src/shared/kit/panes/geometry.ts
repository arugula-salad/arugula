// Tabs and panes (design lift §4.1, part 11; Arugula #1497): rectangles to
// CSS. The daemon lays a tab out in cells, the page draws it in pixels; a
// place's panes (a board's) are already pixels, at a cell of 1×1.

import type { Rect } from "../../proto";

/** One terminal cell's size in pixels. */
export interface Cell {
  width: number;
  height: number;
}

/** CSS length (Preact 11 no longer appends "px" to numbers). */
export const len = (n: number) => `${n}px`;

export function px(r: Rect, cell: Cell) {
  return {
    left: len(r.x * cell.width),
    top: len(r.y * cell.height),
    width: len(r.cols * cell.width),
    height: len(r.rows * cell.height),
  };
}
