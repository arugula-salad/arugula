// An outline frame on a canvas (S43): its lines stroked in their class's
// colour, the grid fitted inside the canvas and centred, round joins so a
// few hundred short segments read as a drawing.

import type { Frame } from "./frame";

export interface DrawOptions {
  /** A line's colour by its class (./labels); `color(0)` for unknown. */
  color: (cls: number) => string;
  /** Flip left to right: your own tile, as a mirror shows you. */
  mirror?: boolean;
  /** Stroke width in CSS pixels. */
  width?: number;
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
  const s = Math.min(canvas.width / f.w, canvas.height / f.h);
  const ox = (canvas.width - f.w * s) / 2, oy = (canvas.height - f.h * s) / 2;
  if (o.mirror) g.setTransform(-s, 0, 0, s, canvas.width - ox, oy);
  else g.setTransform(s, 0, 0, s, ox, oy);
  g.lineWidth = ((o.width ?? 1.6) * dpr) / s;
  g.lineJoin = "round";
  g.lineCap = "round";
  // One path a colour, the unknown first so what's labelled draws on top.
  const by = new Map<string, number[]>();
  const order = f.lines.map((_, i) => i).sort((a, b) => (f.classes?.[a] ?? 0) - (f.classes?.[b] ?? 0));
  for (const i of order) {
    const c = o.color(f.classes?.[i] ?? 0);
    let list = by.get(c);
    if (!list) by.set(c, (list = []));
    list.push(i);
  }
  for (const [color, idx] of by) {
    g.strokeStyle = color;
    g.beginPath();
    for (const i of idx) {
      const l = f.lines[i];
      g.moveTo(l[0] + 0.5, l[1] + 0.5);
      for (let k = 2; k < l.length; k += 2) g.lineTo(l[k] + 0.5, l[k + 1] + 0.5);
    }
    g.stroke();
  }
}
