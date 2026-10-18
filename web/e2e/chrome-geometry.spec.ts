import { expect, test } from "@playwright/test";
import type { Layout } from "../src/proto";
import { paneChrome, HEADER_HEIGHT, PANE_GAP } from "../src/ui/panes/chrome-geometry";
const cell = { width: 8, height: 17 };
const asymmetric: Layout = {
  panes: [[1, { x: 0, y: 0, cols: 40, rows: 30 }], [2, { x: 41, y: 0, cols: 59, rows: 12 }], [3, { x: 41, y: 13, cols: 59, rows: 17 }]],
  splits: [
    { id: 10, dir: "row", rect: { x: 0, y: 0, cols: 100, rows: 30 }, extents: [40, 59] },
    { id: 11, dir: "column", rect: { x: 41, y: 0, cols: 59, rows: 30 }, extents: [12, 17] },
  ],
};
test("nested pane chrome reserves headers without clipping terminal cells", () => {
  const drawn = paneChrome(asymmetric, 100, 30, cell);
  expect(drawn.overhead).toBe(HEADER_HEIGHT * 2 + PANE_GAP - cell.height);
  for (const [id, original] of asymmetric.panes) {
    const content = drawn.panes.get(id)!;
    const frame = drawn.frames.get(id)!;
    expect(content.cols).toBe(original.cols * cell.width);
    expect(content.rows).toBeGreaterThanOrEqual(original.rows * cell.height);
    expect(content.y - frame.y).toBe(HEADER_HEIGHT);
    expect(frame.y + frame.rows).toBeLessThanOrEqual(drawn.height);
  }
  const upper = drawn.frames.get(2)!;
  const lower = drawn.frames.get(3)!;
  expect(lower.y - upper.y - upper.rows).toBe(PANE_GAP);
  expect(drawn.dividers.get("11-0")).toEqual({ x: upper.x, y: upper.y + upper.rows, cols: upper.cols, rows: PANE_GAP });
  expect(drawn.dividers.get("10-0")!.rows).toBe(drawn.height);
});
test("chrome budget is stable when resizing split weights", () => {
  const first = paneChrome(asymmetric, 100, 30, cell);
  const resized = structuredClone(asymmetric);
  resized.splits[1].extents = [20, 9];
  resized.panes[1][1].rows = 20;
  resized.panes[2][1].y = 21;
  resized.panes[2][1].rows = 9;
  const next = paneChrome(resized, 100, 30, cell);
  expect(next.overhead).toBe(first.overhead);
  expect(next.dividers.get("11-0")!.y - first.dividers.get("11-0")!.y).toBe(8 * cell.height);
  // A smaller viewer scales the entire owner grid, including its chrome.
  const scale = Math.min(1, 500 / first.width, 300 / first.height);
  expect(first.height * scale).toBeLessThanOrEqual(300);
  expect(first.width * scale).toBeLessThanOrEqual(500);
});
test("single and focused pane layouts reserve one header", () => {
  const drawn = paneChrome({ panes: [[7, { x: 0, y: 0, cols: 80, rows: 24 }]], splits: [] }, 80, 24, cell);
  expect(drawn.overhead).toBe(HEADER_HEIGHT);
  expect(drawn.panes.get(7)).toEqual({ x: 0, y: HEADER_HEIGHT, cols: 640, rows: 408 });
  expect(drawn.dividers.size).toBe(0);
});

test("resize distance includes spare chrome height in a shallower stacked branch", () => {
  const layout: Layout = {
    panes: [
      [1, { x: 0, y: 0, cols: 40, rows: 12 }], [2, { x: 0, y: 13, cols: 40, rows: 17 }],
      [3, { x: 41, y: 0, cols: 59, rows: 8 }], [4, { x: 41, y: 9, cols: 59, rows: 9 }], [5, { x: 41, y: 19, cols: 59, rows: 11 }],
    ],
    splits: [
      { id: 10, dir: "row", rect: { x: 0, y: 0, cols: 100, rows: 30 }, extents: [40, 59] },
      { id: 11, dir: "column", rect: { x: 0, y: 0, cols: 40, rows: 30 }, extents: [12, 17] },
      { id: 12, dir: "column", rect: { x: 41, y: 0, cols: 59, rows: 30 }, extents: [8, 9, 11] },
    ],
  };
  const before = paneChrome(layout, 100, 30, cell);
  layout.splits[1].extents = [13, 16];
  layout.panes[0][1].rows = 13;
  layout.panes[1][1].y = 14;
  layout.panes[1][1].rows = 16;
  const after = paneChrome(layout, 100, 30, cell);
  const travel = after.dividers.get("11-0")!.y - before.dividers.get("11-0")!.y;
  expect(travel).toBeGreaterThan(cell.height);
  expect(travel).toBeCloseTo(before.resizeUnits.get("11-0")!);
  expect(before.resizeUnits.get("12-0")).toBe(cell.height);
});
