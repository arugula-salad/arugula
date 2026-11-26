// The Overview's divider (#1444): the right column (Needs you, the talk
// panel) is a third of the page by default; drag the hairline, or use the
// arrow keys on it, to resize. The choice is a fraction of the width, kept
// on this device.

import { useEffect, useState } from "preact/hooks";

const KEY = "arugula.overview.column";
/** The column's default share of the Overview's width. */
export const THIRD = 1 / 3;
const MAX = 0.5;
/** The column's least width, in rem (the CSS clamps to it as well). */
const MIN_REM = 18;
const STEP = 0.02;

function load(): number {
  try {
    const v = Number(JSON.parse(localStorage.getItem(KEY) ?? "null"));
    return Number.isFinite(v) && v > 0 ? Math.min(MAX, v) : THIRD;
  } catch {
    return THIRD;
  }
}

let frac = load();
const listeners = new Set<() => void>();

function set(f: number, keep: boolean) {
  frac = Math.min(MAX, Math.max(0, f));
  listeners.forEach((fn) => fn());
  if (!keep) return;
  try {
    localStorage.setItem(KEY, JSON.stringify(frac));
  } catch {
    // Private mode: it lasts for this page.
  }
}

/** The column's share now, for the grid's `--ov-col`. */
export function useColumn(): number {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  return frac;
}

/** The divider, a grid item of the Overview, on the column's left edge. */
export function OverviewSplit() {
  const f = useColumn();
  const [w, setW] = useState(0);
  const [el, setEl] = useState<HTMLElement | null>(null);
  const grid = () => el?.parentElement ?? null;
  useEffect(() => {
    const g = el?.parentElement;
    if (!g) return;
    const ro = new ResizeObserver(() => setW(g.getBoundingClientRect().width));
    ro.observe(g);
    return () => ro.disconnect();
  }, [el]);
  const rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  const min = w > 0 ? Math.min(MAX, (MIN_REM * rem) / w) : 0;
  const now = Math.min(MAX, Math.max(min, f));
  const pct = (x: number) => Math.round(x * 100);
  return (
    <div
      ref={setEl}
      class="ov-split"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize Needs you and the talk panel"
      aria-valuenow={pct(now)}
      aria-valuemin={pct(min)}
      aria-valuemax={pct(MAX)}
      tabIndex={0}
      data-ov-split
      title="Drag to resize; double-click to reset"
      onDblClick={() => set(THIRD, true)}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault();
          // The column grows as the divider goes left.
          set(Math.min(MAX, Math.max(min, now + (e.key === "ArrowLeft" ? STEP : -STEP))), true);
        }
      }}
      onPointerDown={(e) => {
        const g = grid();
        if (!g || e.button !== 0) return;
        const target = e.currentTarget as HTMLElement;
        target.setPointerCapture(e.pointerId);
        // From where it was grabbed, so a click doesn't move it.
        const x0 = e.clientX;
        const w0 = now * g.getBoundingClientRect().width;
        const to = (x: number) => {
          const r = g.getBoundingClientRect();
          return Math.min(MAX, Math.max(r.width > 0 ? (MIN_REM * rem) / r.width : 0, (w0 + x0 - x) / r.width));
        };
        const move = (m: PointerEvent) => set(to(m.clientX), false);
        const up = (u: PointerEvent) => {
          set(to(u.clientX), true);
          target.removeEventListener("pointermove", move);
          target.removeEventListener("pointerup", up);
        };
        target.addEventListener("pointermove", move);
        target.addEventListener("pointerup", up);
      }}
    />
  );
}
