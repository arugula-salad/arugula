// A horizontal swipe on a host element that moves between its siblings (#815:
// a phone's panes; #516 will use it for a View's leaves). Touch pointers
// only. Nothing is claimed until the finger has gone clearly sideways, so a
// tap, a vertical scroll and a long-press stay the page's own.

export type SwipeDir = 1 | -1;

export type SwipeOptions = {
  /** The element that follows the finger. */
  moved: () => HTMLElement | null;
  /** Is there a next (1) or previous (-1) one? Without, the drag rubber-bands. */
  has: (dir: SwipeDir) => boolean;
  commit: (dir: SwipeDir) => void;
};

const SLOP = 12;
const RATIO = 1.5;
const FLICK = 0.5; // px per ms
const SLIDE_MS = 180;
const RUBBER = 0.3;

/** Starting here, the swipe isn't ours: the thing scrolls sideways itself, or
 * is a frame with pointer drags of its own (a Doc's prototype, #507). */
function ours(start: EventTarget | null, host: HTMLElement): boolean {
  for (let el = start instanceof Element ? start : null; el && el !== host; el = el.parentElement) {
    if (el instanceof HTMLIFrameElement) return false;
    const x = getComputedStyle(el).overflowX;
    if ((x === "auto" || x === "scroll") && el.scrollWidth > el.clientWidth) return false;
  }
  return true;
}

/** Listens on `host`; returns what removes it. */
export function attachSwipe(host: HTMLElement, o: SwipeOptions): () => void {
  type Track = { id: number; x: number; y: number; claimed: boolean; samples: [number, number][] };
  let track: Track | null = null;
  let busy = false; // sliding after a release
  let timer = 0;

  const shift = (px: number, ms = 0) => {
    const el = o.moved();
    if (!el) return;
    el.style.transition = ms ? `translate ${ms}ms ease-out` : "none";
    el.style.translate = px ? `${px}px 0` : "";
  };
  const reset = () => {
    const el = o.moved();
    if (el) {
      el.style.transition = "";
      el.style.translate = "";
    }
  };

  const down = (e: PointerEvent) => {
    if (e.pointerType !== "touch" || busy || track || !e.isPrimary) return;
    if (!ours(e.target, host)) return;
    track = { id: e.pointerId, x: e.clientX, y: e.clientY, claimed: false, samples: [[e.timeStamp, e.clientX]] };
  };

  const move = (e: PointerEvent) => {
    const t = track;
    if (!t || e.pointerId !== t.id) return;
    const dx = e.clientX - t.x;
    const dy = e.clientY - t.y;
    if (!t.claimed) {
      if (Math.abs(dy) > SLOP && Math.abs(dy) >= Math.abs(dx)) {
        track = null; // vertical: the page's
        return;
      }
      if (Math.abs(dx) <= SLOP || Math.abs(dx) <= RATIO * Math.abs(dy)) return;
      t.claimed = true;
      try {
        host.setPointerCapture(e.pointerId);
      } catch {
        // The pointer is gone already; the release ends it.
      }
    }
    t.samples.push([e.timeStamp, e.clientX]);
    if (t.samples.length > 6) t.samples.shift();
    const dir: SwipeDir = dx < 0 ? 1 : -1;
    shift(o.has(dir) ? dx : dx * RUBBER);
  };

  const end = (e: PointerEvent) => {
    const t = track;
    if (!t || e.pointerId !== t.id) return;
    track = null;
    if (!t.claimed) return;
    const dx = e.clientX - t.x;
    const dir: SwipeDir = dx < 0 ? 1 : -1;
    const [t0, x0] = t.samples[0];
    const v = e.type === "pointerup" && e.timeStamp > t0 ? Math.abs(e.clientX - x0) / (e.timeStamp - t0) : 0;
    const width = host.clientWidth;
    const go = e.type === "pointerup" && o.has(dir) && (Math.abs(dx) > width / 3 || (v > FLICK && Math.sign(e.clientX - x0) === Math.sign(dx)));
    // The click this touch would end in isn't a tap.
    const swallow = (c: Event) => c.stopPropagation();
    host.addEventListener("click", swallow, { capture: true, once: true });
    setTimeout(() => host.removeEventListener("click", swallow, true), 400);
    const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (calm) {
      reset();
      if (go) o.commit(dir);
      return;
    }
    busy = true;
    shift(go ? -dir * width : 0, SLIDE_MS);
    timer = window.setTimeout(() => {
      busy = false;
      if (go) o.commit(dir);
      // After the next frame draws the new pane, not before.
      requestAnimationFrame(reset);
    }, SLIDE_MS);
  };

  host.addEventListener("pointerdown", down);
  host.addEventListener("pointermove", move);
  host.addEventListener("pointerup", end);
  host.addEventListener("pointercancel", end);
  return () => {
    host.removeEventListener("pointerdown", down);
    host.removeEventListener("pointermove", move);
    host.removeEventListener("pointerup", end);
    host.removeEventListener("pointercancel", end);
    clearTimeout(timer);
    reset();
  };
}
