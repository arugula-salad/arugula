// A cartoon hand for any character (S44): a mitten at the wrist, pointing
// the way the hand points, with a thumb on the side toward the body's
// middle, as a raised palm shows it. No fingers: at tile size a hand
// being there, and where, is what reads. Characters draw an arm and its
// hand only where the camera saw the wrist; a hand whose direction wasn't
// seen points down. Drawn in bust space with the caller's stroke and
// colours.

import type { Hand } from "./rig";

export interface HandStyle {
  /** The hand's length, in shoulder widths (about 0.16 for a bust). */
  size: number;
  /** Filled with this first, so it hides what's behind it. */
  fill: string;
  stroke: string;
}

/** How much bigger than `size` a hand draws: a true-to-life one is lost
 * at tile size. */
const BIG = 1.5;

/** `side`: +1 for the person's left hand (the viewer's right), −1 for the
 * right; `hand` null for one at rest. */
export function drawHand(g: CanvasRenderingContext2D, wrist: [number, number], hand: Hand | null, side: 1 | -1, o: HandStyle) {
  const L = o.size * BIG, W = L * 0.8;
  g.save();
  g.translate(wrist[0], wrist[1]);
  g.rotate(hand ? hand.angle : Math.PI / 2);
  // Hand space: +x from the wrist to the fingertips; the thumb toward the
  // body's middle, which is −side across a raised hand.
  g.scale(1, side === 1 ? -1 : 1);
  g.strokeStyle = o.stroke;
  g.fillStyle = o.fill;
  // The thumb first, so the mitten covers its root.
  g.beginPath();
  g.ellipse(L * 0.42, W * 0.42, L * 0.24, W * 0.17, 0.9, 0, Math.PI * 2);
  g.fill();
  g.stroke();
  g.beginPath();
  g.ellipse(L * 0.55, 0, L * 0.5, W * 0.5, 0, 0, Math.PI * 2);
  g.fill();
  g.stroke();
  g.restore();
}
