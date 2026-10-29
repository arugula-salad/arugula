// An outline frame on a canvas (S43): its lines stroked in one colour,
// the grid fitted inside the canvas and centred, round joins so a few
// hundred short segments read as a drawing.

import type { Frame } from "./frame";

export interface DrawOptions {
  color: string;
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
  g.strokeStyle = o.color;
  g.lineWidth = ((o.width ?? 1.6) * dpr) / s;
  g.lineJoin = "round";
  g.lineCap = "round";
  g.beginPath();
  for (const l of f.lines) {
    g.moveTo(l[0] + 0.5, l[1] + 0.5);
    for (let i = 2; i < l.length; i += 2) g.lineTo(l[i] + 0.5, l[i + 1] + 0.5);
  }
  g.stroke();
}
