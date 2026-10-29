// What a rigged avatar's character is (S44): a function that draws a bust
// from a rig (./rig) on a 2D canvas, in line art like S43's outlines. The
// stage (./stage) sets the canvas up so a character draws in bust space
// and never thinks about pixels: the middle of the shoulders at (0, 0),
// the shoulders 1 apart, +y down, the viewer's right +x.
//
// A character draws its head around (rig.x, HEAD_Y + rig.y), within
// HEAD_R or so of it, turned by rig.roll, with yaw and pitch shown by
// sliding the face across the head (2.5D). The stage crops to the head
// alone for a chat avatar (36 px), so a face has to read at that size.
// An arm is drawn only where the rig has its wrist (the stage fills in a
// missing elbow): no wrist, no arm, a bust.

import type { Rig } from "./rig.ts";

/** Where an upright head's middle is, and about how big it is: a big
 * head on a short neck, so the face carries at chat size. */
export const HEAD_Y = -0.72;
export const HEAD_R = 0.44;

export interface Stage {
  /** The whole bust (a tile), or the head alone (a chat avatar). */
  size: "bust" | "head";
  /** The person's colour (their pick), the theme's ink (text), a fill to
   * hide what's behind (the tile's background), and dim. */
  colour: string;
  ink: string;
  fill: string;
  dim: string;
  /** g.lineWidth as the stage set it: the character's base stroke. */
  stroke: number;
  /** Seconds, for anything that moves on its own. */
  t: number;
  /** 0–1, the breath's phase: the shoulders rise a little at 1. */
  breathe: number;
  /** Away: dozing (the rig already has the eyes shut). */
  away: boolean;
}

export interface Character {
  /** On the wire (one byte); never reuse one. */
  id: number;
  name: string;
  draw(g: CanvasRenderingContext2D, rig: Rig, s: Stage): void;
}
