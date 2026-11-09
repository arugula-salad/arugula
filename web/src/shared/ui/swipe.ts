// A horizontal swipe on a host element that moves between its siblings (#815:
// a phone's panes; #516 will use it for a View's leaves). Touch events,
// because only a touchmove can be prevented to stop the browser scrolling
// (a `touch-action` on the host would stop a pane's own sideways scrolling). Nothing is claimed until the finger has gone clearly sideways, so a
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
 * is a frame with drags of its own (a Doc's prototype, #507). */
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

  const find = (list: TouchList, id: number) => Array.from(list).find((t) => t.identifier === id);

  const down = (e: TouchEvent) => {
    if (busy || track || e.touches.length !== 1) return;
    if (!ours(e.target, host)) return;
    const f = e.touches[0];
    track = { id: f.identifier, x: f.clientX, y: f.clientY, claimed: false, samples: [[e.timeStamp, f.clientX]] };
  };

  const move = (e: TouchEvent) => {
    const t = track;
    const f = t && find(e.touches, t.id);
    if (!t || !f) return;
    const dx = f.clientX - t.x;
    const dy = f.clientY - t.y;
    if (!t.claimed) {
      if (Math.abs(dy) > SLOP && Math.abs(dy) >= Math.abs(dx)) {
        track = null; // vertical: the page's
        return;
      }
      if (Math.abs(dx) <= SLOP || Math.abs(dx) <= RATIO * Math.abs(dy)) return;
      t.claimed = true;
    }
    // Ours now: the browser doesn't scroll under it.
    if (e.cancelable) e.preventDefault();
    t.samples.push([e.timeStamp, f.clientX]);
    if (t.samples.length > 6) t.samples.shift();
    const dir: SwipeDir = dx < 0 ? 1 : -1;
    shift(o.has(dir) ? dx : dx * RUBBER);
  };

  const end = (e: TouchEvent) => {
    const t = track;
    const f = t && find(e.changedTouches, t.id);
    if (!t || !f) return;
    track = null;
    if (!t.claimed) return;
    const up = e.type === "touchend";
    if (up && e.cancelable) e.preventDefault();
    const dx = f.clientX - t.x;
    const dir: SwipeDir = dx < 0 ? 1 : -1;
    const [t0, x0] = t.samples[0];
    const v = up && e.timeStamp > t0 ? Math.abs(f.clientX - x0) / (e.timeStamp - t0) : 0;
    const width = host.clientWidth;
    const go = up && o.has(dir) && (Math.abs(dx) > width / 3 || (v > FLICK && Math.sign(f.clientX - x0) === Math.sign(dx)));
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

  host.addEventListener("touchstart", down, { passive: true });
  host.addEventListener("touchmove", move, { passive: false });
  host.addEventListener("touchend", end);
  host.addEventListener("touchcancel", end);
  return () => {
    host.removeEventListener("touchstart", down);
    host.removeEventListener("touchmove", move);
    host.removeEventListener("touchend", end);
    host.removeEventListener("touchcancel", end);
    clearTimeout(timer);
    reset();
  };
}
