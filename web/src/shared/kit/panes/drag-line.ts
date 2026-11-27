// Tabs and panes (design lift §4.1, part 11; Arugula #1497): dragging a
// divider between two panes. The pointer is captured on the handle; `move`
// hears how far it has gone along the divider's axis, in pixels, and `done`
// hears it let go. What a pixel means (a terminal cell, a share of a
// place's width) is the caller's.

/** Start dragging the divider `e` pressed, across (`row`: left and right)
 * or down (`column`). Nothing for a button other than the first. */
export function dragLine(e: PointerEvent, dir: "row" | "column", on: { move: (px: number) => void; done: () => void }) {
  if (e.button !== 0) return;
  e.preventDefault();
  const handle = e.currentTarget as HTMLElement;
  handle.setPointerCapture(e.pointerId);
  const start = dir === "row" ? e.clientX : e.clientY;
  const move = (ev: PointerEvent) => on.move((dir === "row" ? ev.clientX : ev.clientY) - start);
  const up = () => {
    handle.removeEventListener("pointermove", move);
    handle.removeEventListener("pointerup", up);
    handle.removeEventListener("pointercancel", up);
    on.done();
  };
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", up);
  handle.addEventListener("pointercancel", up);
}
