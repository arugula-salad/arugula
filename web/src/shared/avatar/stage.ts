// Where a character is drawn (S44): a canvas set up in bust space (./character),
// the bust or the head alone, every avatar on the page drawn from one
// animation frame loop while any is on screen. Each view eases toward the
// newest rig it was given (./rig approach), so a dozen frames a second draw
// at the screen's rate; with no rig (no camera, or a stale one) it idles,
// and dozes while its person is away.

import { HEAD_R, HEAD_Y, type Character } from "./character";
import { approach, idle, neutral, withElbows, type Look, type Rig } from "./rig";

/** The colours a person picks from, as theme variables. */
export const COLOURS = ["--accent", "--teal", "--ok", "--warn", "--info", "--danger", "--fg"] as const;

export interface Source {
  /** The newest rig, or null to idle. */
  rig: Rig | null;
  look: Look;
  away: boolean;
}

export interface ViewOptions {
  size: "bust" | "head";
  /** Your own, as a mirror shows you. */
  mirror?: boolean;
  /** Where the rig comes from, read every frame. */
  source: () => Source | null;
  /** The character for a look's number. */
  character: (id: number) => Character;
}

interface View extends ViewOptions {
  canvas: HTMLCanvasElement;
  cur: Rig;
  seed: number;
}

const views = new Set<View>();
let raf = 0;
let then = 0;

/** Draw `canvas` as an avatar until the returned function is called. */
export function animate(canvas: HTMLCanvasElement, o: ViewOptions): () => void {
  let seed = 0;
  for (const c of String(Math.random())) seed = (seed * 31 + c.charCodeAt(0)) % 997;
  const v: View = { ...o, canvas, cur: neutral(), seed: seed / 997 };
  views.add(v);
  if (!raf) raf = requestAnimationFrame(frame);
  return () => {
    views.delete(v);
  };
}

function frame(now: number) {
  raf = 0;
  if (!views.size) return;
  const dt = then ? Math.min(0.1, (now - then) / 1000) : 1 / 60;
  then = now;
  for (const v of views) {
    try {
      paint(v, now / 1000, dt);
    } catch {
      // A character that throws draws nothing this frame, not a stuck loop.
    }
  }
  raf = requestAnimationFrame(frame);
}

function paint(v: View, t: number, dt: number) {
  const c = v.canvas;
  const cw = c.clientWidth, ch = c.clientHeight;
  // Off screen (a hidden tab, a scrolled-away message): nothing to do.
  if (!cw || !ch || !c.isConnected) return;
  const src = v.source();
  if (!src) return;
  const dpr = window.devicePixelRatio || 1;
  if (c.width !== Math.round(cw * dpr) || c.height !== Math.round(ch * dpr)) {
    c.width = Math.round(cw * dpr);
    c.height = Math.round(ch * dpr);
  }
  const resting = idle(t, src.away, v.seed);
  v.cur = approach(v.cur, src.rig ?? resting.rig, dt, src.rig ? 14 : 4);
  const g = c.getContext("2d");
  if (!g) return;
  const css = getComputedStyle(c);
  const of = (name: string) => css.getPropertyValue(name).trim() || css.color;
  const fill = css.getPropertyValue("--void").trim() || css.getPropertyValue("--bg").trim() || "transparent";
  g.setTransform(1, 0, 0, 1, 0, 0);
  g.clearRect(0, 0, c.width, c.height);
  // Bust space to pixels: the bust fits the canvas; the head fills it.
  let s: number, ox: number, oy: number;
  if (v.size === "bust") {
    // Moved about the tile as the person moves about the frame, and
    // bigger leaning in; never so far that the head leaves the tile.
    s = Math.min(c.width / 2.2, c.height / 2.3) * v.cur.scale;
    ox = c.width / 2 + v.cur.bodyX * c.width * 0.3;
    oy = c.height - 0.9 * s + v.cur.bodyY * c.height * 0.2;
    const hx = ox + v.cur.x * s;
    ox += Math.max(0, c.width * 0.15 - hx) - Math.max(0, hx - c.width * 0.85);
    oy = Math.max(oy, -(HEAD_Y - HEAD_R * 1.15) * s);
  } else {
    const r = HEAD_R * 1.25;
    s = Math.max(c.width, c.height) / (2 * r);
    ox = c.width / 2 - v.cur.x * s;
    oy = c.height / 2 - (HEAD_Y + v.cur.y) * s;
  }
  g.setTransform(v.mirror ? -s : s, 0, 0, s, v.mirror ? c.width - ox : ox, oy);
  const stroke = ((v.size === "bust" ? 2 : 1.4) * dpr) / s;
  g.lineWidth = stroke;
  g.lineJoin = "round";
  g.lineCap = "round";
  const look = src.look;
  v.character(look.character).draw(g, withElbows(v.cur), {
    size: v.size,
    colour: of(COLOURS[look.colour % COLOURS.length]),
    ink: of("--fg"),
    fill,
    dim: of("--dim"),
    stroke,
    t,
    breathe: resting.breathe,
    away: src.away && !src.rig,
  });
}
