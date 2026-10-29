// From what the landmarkers see to a rig (S44): the face landmarker's
// ARKit blendshapes and head transform, the pose landmarker's shoulders,
// elbows and wrists, and the hand landmarker's hands (where each is and
// which way it points), as ./rig's numbers. An elbow the camera doesn't
// see is left out: the drawing puts it where the arm bends (rig's
// withElbows). Pure, so node tests
// it (web/scripts/avatar.test.mjs); ./track runs the models.

import { neutral, type Hand, type Point, type Rig } from "./rig.ts";

/** A blendshape's name to its score (0–1), as MediaPipe names them
 * (ARKit's names; Left and Right are the person's own). */
export type Shapes = Record<string, number>;

/** A landmark in the camera's image: 0–1 across and down, unmirrored. */
export interface Mark {
  x: number;
  y: number;
  visibility?: number;
}

/** Pose landmarks: shoulders, elbows, wrists (BlazePose's numbering). */
const POSE = { shoulderL: 11, shoulderR: 12, elbowL: 13, elbowR: 14, wristL: 15, wristR: 16 } as const;
const EAR_L = 7, EAR_R = 8;
/** The face mesh's nose tip. */
const NOSE = 1;

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
/** MediaPipe's blinks rarely reach 1, or rest at 0: stretch 0.15–0.6 to 0–1. */
const lid = (v: number) => clamp((v - 0.15) / 0.45, 0, 1);

export interface Seen {
  /** The face transform, 4×4, column-major (MediaPipe's), or null. */
  matrix: number[] | null;
  shapes: Shapes | null;
  /** The face mesh (only the nose tip is used), or null. */
  face: Mark[] | null;
  /** The body (33 BlazePose points), or null. */
  pose: Mark[] | null;
  /** Hands (21 points each), any number, or null. */
  hands?: Mark[][] | null;
  /** The image's width over its height (landmarks are 0–1 on each). */
  aspect?: number;
  /** The body in metres around the hips (the pose's world landmarks):
   * which way the shoulders face. */
  poseWorld?: { x: number; y: number; z: number }[] | null;
}

/** What a shrug is measured against: each shoulder's usual drop below
 * its ear, learnt as it goes (quick to rise, slow to fall, so a shrug
 * doesn't become the new rest but a slouch does, in half a minute). */
export interface Calib {
  gapL?: number;
  gapR?: number;
}

const learn = (base: number | undefined, v: number) => (base === undefined ? v : base + (v - base) * (v > base ? 0.3 : 0.003));

/** Which way `h` points, wrist to middle knuckle, in bust space (`ax`
 * scales x for the image's aspect). */
export function handOf(h: Mark[], ax: number): Hand {
  return { angle: Math.atan2(h[9].y - h[0].y, (h[9].x - h[0].x) * ax) };
}

/** The rig for what was seen; `null`s leave the rig neutral there. */
export function solve(s: Seen, calib: Calib = {}): Rig {
  const r = neutral();
  const m = s.matrix;
  if (m && m.length >= 16) {
    r.face = true;
    // The head's forward axis is the third column and its up axis the
    // second, in the camera's space (x the image's right, y up).
    r.yaw = Math.atan2(m[8], m[10]);
    r.pitch = Math.asin(clamp(m[9], -1, 1));
    r.roll = Math.atan2(m[4], m[5]);
  }
  const b = s.shapes;
  if (b) {
    const v = (k: string) => b[k] ?? 0;
    r.blinkL = lid(v("eyeBlinkLeft"));
    r.blinkR = lid(v("eyeBlinkRight"));
    // Looking toward the viewer's right is the person's left: the left
    // eye looks out, the right eye in.
    r.lookX = clamp(((v("eyeLookOutLeft") + v("eyeLookInRight")) - (v("eyeLookInLeft") + v("eyeLookOutRight"))) * 0.75, -1, 1);
    r.lookY = clamp(((v("eyeLookUpLeft") + v("eyeLookUpRight")) - (v("eyeLookDownLeft") + v("eyeLookDownRight"))) * 0.75, -1, 1);
    r.browUp = (v("browOuterUpLeft") + v("browOuterUpRight")) / 2;
    r.browDown = (v("browDownLeft") + v("browDownRight")) / 2;
    r.browInnerUp = v("browInnerUp");
    r.jawOpen = clamp(v("jawOpen") * 1.4, 0, 1);
    r.smile = clamp(((v("mouthSmileLeft") + v("mouthSmileRight")) / 2) * 1.3, 0, 1);
    r.funnel = v("mouthFunnel");
    r.pucker = v("mouthPucker");
    r.cheekPuff = v("cheekPuff");
  }
  // Landmarks are 0–1 across and 0–1 down: x stretched by the aspect is
  // the same scale as y.
  const ax = s.aspect ?? 4 / 3;
  const p = s.pose;
  const sl = p?.[POSE.shoulderL], sr = p?.[POSE.shoulderR];
  let toBust: ((q: Mark) => [number, number]) | null = null;
  let mid = 0.5;
  if (p && sl && sr && seen(sl) && seen(sr)) {
    const mx = (sl.x + sr.x) / 2, my = (sl.y + sr.y) / 2;
    mid = mx;
    // Turned, the shoulders look closer together: measure in their real
    // width (the world landmarks say how far round they face), so the rest
    // of the body doesn't grow as you turn.
    const w = s.poseWorld;
    const turn = w?.[POSE.shoulderL] && w[POSE.shoulderR] ? Math.atan2(w[POSE.shoulderL].z - w[POSE.shoulderR].z, w[POSE.shoulderL].x - w[POSE.shoulderR].x) : 0;
    const unit = (Math.hypot((sl.x - sr.x) * ax, sl.y - sr.y) || 1) / Math.max(0.5, Math.cos(turn));
    // The image unmirrored: the person's left is the image's right, which
    // is bust space's +x already.
    toBust = (q) => [((q.x - mx) * ax) / unit, (q.y - my) / unit];
    const at = (k: keyof typeof POSE): Point => {
      const q = p[POSE[k]];
      return q && seen(q) ? toBust!(q) : null;
    };
    r.elbowL = at("elbowL");
    r.elbowR = at("elbowR");
    // A wrist counts without its elbow: at a desk the elbows are mostly
    // out of frame and the hands in it.
    r.wristL = at("wristL");
    r.wristR = at("wristR");
    // The shoulders themselves: tilt and turn come with where they are; a
    // shrug is a shoulder nearer its ear than usual.
    const lift = (sh: Mark, ear: Mark | undefined, key: "gapL" | "gapR") => {
      if (!ear || !seen(ear)) return 0;
      const gap = (sh.y - ear.y) / unit;
      const base = (calib[key] = learn(calib[key], gap));
      return clamp((base - gap) / 0.3, 0, 1) * 0.22;
    };
    const liftL = lift(sl, p[EAR_L], "gapL"), liftR = lift(sr, p[EAR_R], "gapR");
    const bl = toBust(sl), br = toBust(sr);
    r.shoulderL = [bl[0], bl[1] - liftL];
    r.shoulderR = [br[0], br[1] - liftR];
    const nose = s.face?.[NOSE];
    if (nose) {
      // An upright head's nose sits about 0.9 shoulder widths above them;
      // a shrug brings the shoulders up, not the head down.
      r.x = clamp(((nose.x - mx) * ax) / unit, -0.4, 0.4);
      r.y = clamp(((nose.y - my) / unit + 0.9) * 0.6 - ((liftL + liftR) / 2) * 0.6, -0.3, 0.3);
    }
  } else if (s.face?.[NOSE]) {
    r.x = clamp((s.face[NOSE].x - 0.5) * 0.6, -0.3, 0.3);
  }
  for (const h of s.hands ?? []) {
    if (!h || h.length < 21) continue;
    // Whose hand: the nearer of the pose's wrists, else which side of the
    // shoulders it's on (the person's left is the image's right).
    const wl = p?.[POSE.wristL], wr = p?.[POSE.wristR];
    const dist = (q: Mark | undefined) => (q ? Math.hypot((q.x - h[0].x) * ax, q.y - h[0].y) : Infinity);
    const left = wl || wr ? dist(wl) < dist(wr) : h[0].x > mid;
    const side = left ? "L" : "R";
    if (r[`hand${side}`]) continue;
    r[`hand${side}`] = handOf(h, ax);
    // A hand seen where the pose lost the wrist: the wrist is the hand's.
    if (toBust && !r[`wrist${side}`]) r[`wrist${side}`] = toBust(h[0]);
  }
  return r;
}

const seen = (q: Mark) => (q.visibility ?? 1) >= 0.5;

/** The One-Euro filter (Casiez et al.): smooth when still, quick when
 * moving. One per number. */
export class OneEuro {
  private x: number | null = null;
  private dx = 0;
  private t = 0;
  private minCutoff: number;
  private beta: number;
  private dCutoff: number;
  constructor(minCutoff = 1.2, beta = 0.02, dCutoff = 1) {
    this.minCutoff = minCutoff;
    this.beta = beta;
    this.dCutoff = dCutoff;
  }
  filter(v: number, t: number): number {
    if (this.x === null) {
      this.x = v;
      this.t = t;
      return v;
    }
    const dt = Math.max(1e-3, t - this.t);
    this.t = t;
    const a = (c: number) => 1 / (1 + 1 / (2 * Math.PI * c * dt));
    const dx = (v - this.x) / dt;
    this.dx += a(this.dCutoff) * (dx - this.dx);
    this.x += a(this.minCutoff + this.beta * Math.abs(this.dx)) * (v - this.x);
    return this.x;
  }
}

/** A rig's numbers through a filter each, at `t` seconds. Arms are
 * filtered while seen and forgotten when not. */
export class RigFilter {
  private f = new Map<string, OneEuro>();
  private angles = new Map<string, number>();
  apply(r: Rig, t: number): Rig {
    const out = { ...r };
    const run = (k: string, v: number) => {
      let f = this.f.get(k);
      if (!f) this.f.set(k, (f = k.startsWith("blink") ? new OneEuro(3, 0.5) : new OneEuro()));
      return f.filter(v, t);
    };
    for (const k of Object.keys(r) as (keyof Rig)[]) {
      const v = r[k];
      if (typeof v === "number") (out as Record<string, unknown>)[k] = run(k, v);
      else if (Array.isArray(v)) (out as Record<string, unknown>)[k] = [run(`${k}.x`, v[0]), run(`${k}.y`, v[1])];
      else if (v && typeof v === "object") {
        // A hand: its angle, unwrapped so it doesn't spin at ±π.
        const prev = this.angles.get(k);
        const a = prev === undefined ? v.angle : prev + Math.atan2(Math.sin(v.angle - prev), Math.cos(v.angle - prev));
        const angle = run(`${k}.a`, a);
        this.angles.set(k, angle);
        (out as Record<string, unknown>)[k] = { angle };
      } else if (v === null) {
        for (const key of [...this.f.keys()]) if (key.startsWith(`${k}.`)) this.f.delete(key);
        this.angles.delete(k);
      }
    }
    return out;
  }
}
