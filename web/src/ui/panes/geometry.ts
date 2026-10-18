// Cell rectangles to CSS: the daemon lays a tab out in cells, the page
// draws it in pixels.

import type { Rect } from "../../proto";
import type { Cell } from "../cells";

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
