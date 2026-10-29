// A person's pose as numbers (S44): what a rigged avatar is drawn from.
// The page with the camera fills one from the face and pose landmarkers
// (./track) and sends it, a few dozen bytes, through S43's relay; every
// page that shows the person draws their character (./characters) from it.
// No picture of anyone leaves their page.
//
// Space: "bust space", as the person faces whoever looks at them (not a
// mirror). The middle of the shoulders is (0, 0), the shoulders are 1
// apart, +x is the viewer's right (the person's left), +y is down. A
// side (L, R) is the person's own.
//
// On the wire (encoding byte 0, as S43's frames: raw):
//   rig     = kind:u8 (0x40) flags:u8 character:u8 colour:u8 head face arms?
//   head    = yaw:i8 pitch:i8 roll:i8 (degrees) x:i8 y:i8 (1/50 shoulder)
//   face    = 12 × u8 (0–255 for 0–1; lookX, lookY as i8 for −1–1)
//   arms    = 4 points (elbowL, elbowR, wristL, wristR) × x:i8 y:i8 (1/25
//             shoulder), sent when flags & ARMS; a point at (−128, −128)
//             wasn't seen (an elbow mostly isn't, at a desk: `withElbows`
//             puts it where the arm would bend)
//   hand    = angle:i8 (π/127), for the left hand when flags & HAND_L,
//             then the right when flags & HAND_R
//   shoulders = shoulderL, shoulderR × x:i8 y:i8 (1/25 shoulder), when
//             flags & SHOULDERS
// flags: FACE (a face was found), ARMS, HAND_L, HAND_R, SHOULDERS.

export const RIG = 0x40;
const FACE = 1;
const ARMS = 2;
const HAND_L = 4;
const HAND_R = 8;
const SHOULDERS = 16;

export type Point = [number, number] | null;

/** A hand, as a character draws it at the wrist: no fingers, just which
 * way it points. */
export interface Hand {
  /** Wrist to knuckles: radians in bust space (0 toward +x, the viewer's
   * right; −π/2 straight up). */
  angle: number;
}

export interface Rig {
  /** A face was found this frame (else the head is a guess). */
  face: boolean;
  /** Radians. yaw > 0 turns toward the viewer's right, pitch > 0 looks
   * up, roll > 0 tips the top of the head toward the viewer's right. */
  yaw: number;
  pitch: number;
  roll: number;
  /** The head's lean from upright over the shoulders, in shoulder widths. */
  x: number;
  y: number;
  /** 0 open, 1 shut. */
  blinkL: number;
  blinkR: number;
  /** Where the eyes look: −1 to 1, +x the viewer's right, +y up. */
  lookX: number;
  lookY: number;
  /** 0–1. */
  browUp: number;
  browDown: number;
  browInnerUp: number;
  jawOpen: number;
  smile: number;
  /** An "oo" (funnel) and a kiss (pucker). */
  funnel: number;
  pucker: number;
  cheekPuff: number;
  /** In bust space; null where the arm wasn't seen (it rests). */
  elbowL: Point;
  elbowR: Point;
  wristL: Point;
  wristR: Point;
  /** Hands, where the hand landmarker saw them (else null: a resting
   * hand, as the character likes). */
  handL: Hand | null;
  handR: Hand | null;
  /** Where each shoulder is: about (±0.5, 0) at rest, closer together as
   * the body turns, one above the other as it tilts, both up in a shrug.
   * Null where the pose wasn't seen: draw them at rest (SHOULDER_L/R). */
  shoulderL: Point;
  shoulderR: Point;
}

/** Shoulders at rest, for a rig without them. */
export const SHOULDER_L: [number, number] = [0.5, 0];
export const SHOULDER_R: [number, number] = [-0.5, 0];

/** Which character, in which colour (./characters). */
export interface Look {
  character: number;
  colour: number;
}

/** A neutral, upright, eyes-open rig. */
export function neutral(): Rig {
  return {
    face: false, yaw: 0, pitch: 0, roll: 0, x: 0, y: 0,
    blinkL: 0, blinkR: 0, lookX: 0, lookY: 0,
    browUp: 0, browDown: 0, browInnerUp: 0, jawOpen: 0, smile: 0, funnel: 0, pucker: 0, cheekPuff: 0,
    elbowL: null, elbowR: null, wristL: null, wristR: null, handL: null, handR: null,
    shoulderL: null, shoulderR: null,
  };
}

const FACE_KEYS = ["blinkL", "blinkR", "lookX", "lookY", "browUp", "browDown", "browInnerUp", "jawOpen", "smile", "funnel", "pucker", "cheekPuff"] as const;
const SIGNED = new Set(["lookX", "lookY"]);
const ARM_KEYS = ["elbowL", "elbowR", "wristL", "wristR"] as const;
const POINT_KEYS = [...ARM_KEYS, "shoulderL", "shoulderR"] as const;
const DEG = 180 / Math.PI;

const i8 = (v: number) => Math.max(-127, Math.min(127, Math.round(v))) & 0xff;
const s8 = (b: number) => (b > 127 ? b - 256 : b);
const u8 = (v: number) => Math.max(0, Math.min(255, Math.round(v * 255)));

export function packRig(r: Rig, look: Look): Uint8Array {
  const arms = ARM_KEYS.some((k) => r[k]);
  const shoulders = !!(r.shoulderL && r.shoulderR);
  const flags = (r.face ? FACE : 0) | (arms ? ARMS : 0) | (r.handL ? HAND_L : 0) | (r.handR ? HAND_R : 0) | (shoulders ? SHOULDERS : 0);
  const out = [RIG, flags, look.character & 0xff, look.colour & 0xff];
  out.push(i8(r.yaw * DEG), i8(r.pitch * DEG), i8(r.roll * DEG), i8(r.x * 50), i8(r.y * 50));
  for (const k of FACE_KEYS) out.push(SIGNED.has(k) ? i8(r[k] * 127) : u8(r[k]));
  if (arms) {
    for (const k of ARM_KEYS) {
      const p = r[k];
      out.push(...(p ? [i8(p[0] * 25), i8(p[1] * 25)] : [0x80, 0x80]));
    }
  }
  for (const h of [r.handL, r.handR]) if (h) out.push(i8((wrap(h.angle) * 127) / Math.PI));
  if (shoulders) for (const p of [r.shoulderL!, r.shoulderR!]) out.push(i8(p[0] * 25), i8(p[1] * 25));
  return Uint8Array.from(out);
}

export function unpackRig(b: Uint8Array): { rig: Rig; look: Look } {
  if (b[0] !== RIG || b.length < 4 + 5 + FACE_KEYS.length) throw new Error("rig frame: not one");
  const flags = b[1];
  const r = neutral();
  r.face = !!(flags & FACE);
  let p = 4;
  r.yaw = s8(b[p++]) / DEG;
  r.pitch = s8(b[p++]) / DEG;
  r.roll = s8(b[p++]) / DEG;
  r.x = s8(b[p++]) / 50;
  r.y = s8(b[p++]) / 50;
  for (const k of FACE_KEYS) r[k] = SIGNED.has(k) ? s8(b[p++]) / 127 : b[p++] / 255;
  if (flags & ARMS) {
    if (b.length < p + 8) throw new Error("rig frame: cut short");
    for (const k of ARM_KEYS) {
      const x = b[p++], y = b[p++];
      r[k] = x === 0x80 && y === 0x80 ? null : [s8(x) / 25, s8(y) / 25];
    }
  }
  const hand = (): Hand => {
    if (b.length < p + 1) throw new Error("rig frame: cut short");
    return { angle: (s8(b[p++]) * Math.PI) / 127 };
  };
  if (flags & HAND_L) r.handL = hand();
  if (flags & HAND_R) r.handR = hand();
  if (flags & SHOULDERS) {
    if (b.length < p + 4) throw new Error("rig frame: cut short");
    r.shoulderL = [s8(b[p++]) / 25, s8(b[p++]) / 25];
    r.shoulderR = [s8(b[p++]) / 25, s8(b[p++]) / 25];
  }
  return { rig: r, look: { character: b[2], colour: b[3] } };
}

/** What a person does with no camera on, at `t` seconds: breathes, blinks
 * every few seconds, glances about; away, dozes (eyes shut, head tipped,
 * slow breaths). `seed` keeps two people from moving in step. */
export function idle(t: number, away: boolean, seed = 0): { rig: Rig; breathe: number } {
  const r = neutral();
  const s = t + seed * 7.31;
  if (away) {
    r.blinkL = r.blinkR = 1;
    r.roll = 0.16;
    r.pitch = -0.12;
    r.y = 0.04;
    return { rig: r, breathe: Math.sin(s * 1.3) * 0.5 + 0.5 };
  }
  // A blink about every 4 s, 150 ms long.
  const phase = (s % 4.1) / 4.1;
  const blink = phase > 0.96 ? Math.sin(((phase - 0.96) / 0.04) * Math.PI) : 0;
  r.blinkL = r.blinkR = blink;
  r.lookX = Math.sin(s * 0.37) * 0.35;
  r.lookY = Math.sin(s * 0.23) * 0.15;
  r.yaw = Math.sin(s * 0.29) * 0.12;
  r.pitch = Math.sin(s * 0.17) * 0.05;
  r.smile = 0.15;
  return { rig: r, breathe: Math.sin(s * 2.1) * 0.5 + 0.5 };
}

/** An angle in −π…π. */
export const wrap = (a: number) => Math.atan2(Math.sin(a), Math.cos(a));

/** `cur` moved toward `want` over `dt` seconds, critically damped, so a
 * 12 fps stream draws smoothly at 60. Arms and hands snap in and out of
 * view. */
export function approach(cur: Rig, want: Rig, dt: number, rate = 14): Rig {
  const k = 1 - Math.exp(-dt * rate);
  const out = { ...want };
  for (const key of ["yaw", "pitch", "roll", "x", "y", ...FACE_KEYS] as const) out[key] = cur[key] + (want[key] - cur[key]) * k;
  for (const key of POINT_KEYS) {
    const a = cur[key], b = want[key];
    out[key] = a && b ? [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k] : b;
  }
  for (const key of ["handL", "handR"] as const) {
    const a = cur[key], b = want[key];
    out[key] = a && b ? { angle: a.angle + wrap(b.angle - a.angle) * k } : b;
  }
  return out;
}

/** An arm's upper and lower lengths, in shoulder widths. */
const UPPER = 0.55, LOWER = 0.5;

/** Where an elbow goes for a shoulder and a wrist: two bones of fixed
 * length, bent outward and down as an arm bends (`side` +1 for the
 * person's left, on +x). A wrist out of reach straightens the arm toward
 * it. */
export function elbowFor(shoulder: [number, number], wrist: [number, number], side: 1 | -1): [number, number] {
  const dx = wrist[0] - shoulder[0], dy = wrist[1] - shoulder[1];
  const d = Math.min(UPPER + LOWER - 1e-3, Math.max(Math.abs(UPPER - LOWER) + 1e-3, Math.hypot(dx, dy) || 1e-3));
  const base = Math.atan2(dy, dx);
  const bend = Math.acos((UPPER * UPPER + d * d - LOWER * LOWER) / (2 * UPPER * d));
  const at = (a: number): [number, number] => [shoulder[0] + Math.cos(a) * UPPER, shoulder[1] + Math.sin(a) * UPPER];
  const a = at(base + bend), b = at(base - bend);
  // Outward first, then down.
  const score = (e: [number, number]) => side * (e[0] - shoulder[0]) + 0.6 * (e[1] - shoulder[1]);
  return score(a) >= score(b) ? a : b;
}

/** `r` with an elbow wherever there's a wrist but no elbow: the camera
 * at a desk sees hands far more often than elbows. */
export function withElbows(r: Rig): Rig {
  if ((r.elbowL || !r.wristL) && (r.elbowR || !r.wristR)) return r;
  const out = { ...r };
  if (!r.elbowL && r.wristL) out.elbowL = elbowFor(r.shoulderL ?? SHOULDER_L, r.wristL, 1);
  if (!r.elbowR && r.wristR) out.elbowR = elbowFor(r.shoulderR ?? SHOULDER_R, r.wristR, -1);
  return out;
}
