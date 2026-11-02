// How building a View moves (#503). These numbers came from the motion
// prototype, whose Copy values button gives this same object: tuning the feel
// is a change to this file.

export const MOTION = {
  /** How hard a block pulls to where it's going, and how much it overshoots. */
  stiffness: 320,
  damping: 25,
  /** How much each block of a carried group trails the one before. */
  flockLag: 0.55,
  /** A press this long on a block picks it up; moving sooner pans. */
  grabMs: 130,
  /** Touch waits longer: a finger on a block is usually a scroll. */
  touchGrabMs: 280,
  /** How much a tile swells when grabbed. */
  lift: 1.25,
  /** A release this fast toward the stage is a fling (px/s). */
  flingSpeed: 900,
  /** How long a flung block keeps its momentum before the pull wins. */
  flightMs: 300,
  /** The gap between a group's blocks landing. */
  stagger: 55,
  /** How much of a leaf counts as its edge. */
  edge: 0.3,
  /** How far past the stage a leaf goes before it heads home (px). */
  tear: 56,
  trail: 0.7,
  glow: 1,
};

export const reducedMotion = (): boolean => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/** One step of a damped spring. */
export function spring(o: Record<string, number>, key: string, vel: string, target: number, k: number, c: number, dt: number) {
  o[vel] += (k * (target - o[key]) - c * o[vel]) * dt;
  o[key] += o[vel] * dt;
}
