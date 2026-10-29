// An outline frame on a canvas (S43): its lines stroked in their class's
// colour, the grid fitted inside the canvas and centred, round joins so a
// few hundred short segments read as a drawing.

import type { Frame } from "./frame.ts";

export interface DrawOptions {
  /** A line's colour by its class (./labels); `color(0)` for unknown. */
  color: (cls: number) => string;
  /** Flip left to right: your own tile, as a mirror shows you. */
  mirror?: boolean;
  /** Stroke width in CSS pixels. */
  width?: number;
  /** Fit the whole grid (contain, the default) or fill the canvas and
   * crop (cover: an avatar). */
  fit?: "contain" | "cover";
}

export function draw(canvas: HTMLCanvasElement, f: Frame, o: DrawOptions) {
  const dpr = window.devicePixelRatio || 1;
  const cw = canvas.clientWidth, ch = canvas.clientHeight;
  if (!cw || !ch) return;
  if (canvas.width !== Math.round(cw * dpr) || canvas.height !== Math.round(ch * dpr)) {
    canvas.width = Math.round(cw * dpr);
    canvas.height = Math.round(ch * dpr);
  }
  const g = canvas.getContext("2d");
  if (!g) return;
  g.setTransform(1, 0, 0, 1, 0, 0);
  g.clearRect(0, 0, canvas.width, canvas.height);
  const s = (o.fit === "cover" ? Math.max : Math.min)(canvas.width / f.w, canvas.height / f.h);
  const ox = (canvas.width - f.w * s) / 2, oy = (canvas.height - f.h * s) / 2;
  if (o.mirror) g.setTransform(-s, 0, 0, s, canvas.width - ox, oy);
  else g.setTransform(s, 0, 0, s, ox, oy);
  g.lineWidth = ((o.width ?? 1.6) * dpr) / s;
  g.lineJoin = "round";
  g.lineCap = "round";
  // One path a colour and distance: the far and the unknown first, so
  // what's near and labelled draws on top. Far lines are fainter and
  // thinner, four steps from near to far.
  const width = g.lineWidth;
  const step = (i: number) => (f.depths?.[i] ?? 255) >> 6;
  const by = new Map<string, { color: string; step: number; idx: number[] }>();
  const order = f.lines
    .map((_, i) => i)
    .sort((a, b) => step(a) - step(b) || (f.classes?.[a] ?? 0) - (f.classes?.[b] ?? 0));
  for (const i of order) {
    const color = o.color(f.classes?.[i] ?? 0);
    const key = `${step(i)} ${color}`;
    let group = by.get(key);
    if (!group) by.set(key, (group = { color, step: step(i), idx: [] }));
    group.idx.push(i);
  }
  for (const { color, step: near, idx } of by.values()) {
    g.strokeStyle = color;
    g.globalAlpha = [0.3, 0.5, 0.75, 1][near];
    g.lineWidth = width * [0.55, 0.7, 0.85, 1][near];
    g.beginPath();
    for (const i of idx) {
      const l = f.lines[i];
      g.moveTo(l[0] + 0.5, l[1] + 0.5);
      for (let k = 2; k < l.length; k += 2) g.lineTo(l[k] + 0.5, l[k + 1] + 0.5);
    }
    g.stroke();
  }
  g.globalAlpha = 1;
}
