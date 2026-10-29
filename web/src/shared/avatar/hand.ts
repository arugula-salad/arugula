// A cartoon hand for any character (S44): a palm at the wrist, pointing
// along the hand's angle, five fingers each shortened by its curl (a front
// view: a curled finger folds toward you), the thumb on the side toward
// the body's middle, as a raised palm shows it. With no hand seen, a
// resting mitten pointing down. Drawn in bust space with the caller's
// stroke and colours; a character picks a style that suits it.

import type { Hand } from "./rig";

export interface HandStyle {
  /** The palm's length, in shoulder widths (about 0.16 for a bust). */
  size: number;
  /** Separate fingers, or one mitten (fingers 2–5 together, a thumb). */
  kind: "fingers" | "mitten";
  /** Filled with this first, so it hides what's behind it. */
  fill: string;
  stroke: string;
}

/** How much bigger than `size` a hand draws. */
const BIG = 1.5;

/** Each finger: where it leaves the knuckles (across the palm, −1…1), its
 * length and its splay (radians), index to pinky; then the thumb. */
const FINGERS = [
  { at: 0.62, len: 0.78, splay: 0.12 },
  { at: 0.2, len: 0.88, splay: 0.03 },
  { at: -0.22, len: 0.8, splay: -0.06 },
  { at: -0.6, len: 0.62, splay: -0.16 },
];

/** `side`: +1 for the person's left hand (the viewer's right), −1 for the
 * right; `hand` null for one at rest. */
export function drawHand(g: CanvasRenderingContext2D, wrist: [number, number], hand: Hand | null, side: 1 | -1, o: HandStyle) {
  const angle = hand ? hand.angle : Math.PI / 2;
  const curl = hand ? hand.curl : ([0.55, 0.7, 0.7, 0.7, 0.7] as Hand["curl"]);
  // Cartoon hands are big: a true-to-life one is lost at tile size.
  const L = o.size * BIG, W = L * 0.86;
  g.save();
  g.translate(wrist[0], wrist[1]);
  g.rotate(angle);
  // Hand space: +x from the wrist to the knuckles; the thumb is toward
  // the body's middle, which is −side across a raised hand.
  const thumb = side === 1 ? -1 : 1;
  g.scale(1, thumb);
  g.strokeStyle = o.stroke;
  g.fillStyle = o.fill;
  const lw = g.lineWidth;

  const finger = (bx: number, by: number, dir: number, len: number, c: number, width: number) => {
    // Front view: a curl folds the finger toward you, so it shortens and
    // its tip comes back toward the palm.
    const seen = len * (1 - 0.8 * c);
    const tx = bx + Math.cos(dir) * seen, ty = by + Math.sin(dir) * seen;
    g.lineWidth = width;
    g.beginPath();
    g.moveTo(bx, by);
    g.lineTo(tx, ty);
    g.stroke();
    // A fill line down the middle, so overlaps read as separate fingers.
    g.lineWidth = Math.max(0, width - lw * 2);
    g.strokeStyle = o.fill;
    g.stroke();
    g.strokeStyle = o.stroke;
  };

  // The palm.
  g.lineWidth = lw;
  g.beginPath();
  g.ellipse(L * 0.5, 0, L * 0.55, W * 0.5, 0, 0, Math.PI * 2);
  g.fill();
  g.stroke();
  const fw = W * 0.26;
  if (o.kind === "fingers") {
    for (let i = 0; i < 4; i++) {
      const f = FINGERS[i];
      // The index is next to the thumb (+y in hand space).
      finger(L * 0.92, f.at * W * 0.42, f.splay, L * f.len, curl[i + 1], fw);
    }
  } else {
    // A mitten: the four fingers as one, as curled as their average.
    const c = (curl[1] + curl[2] + curl[3] + curl[4]) / 4;
    finger(L * 0.88, 0, 0, L * 0.7, c, W * 0.86);
  }
  // The thumb: out from the side of the palm, folding across it as it curls.
  const td = 1.0 - 0.8 * curl[0];
  finger(L * 0.35, W * 0.42, td, L * 0.62, curl[0] * 0.6, fw * 1.05);
  g.restore();
}
