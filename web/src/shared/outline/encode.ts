// A camera frame as outlines (S43): the lines a face and a room make,
// traced from a small gray image, so a person's presence is a few hundred
// strokes rather than pixels. Nothing here touches the DOM: it takes
// RGBA bytes (an ImageData's) and gives polylines, so a page, a worker and
// node (web/scripts/outline.test.mjs) all run the same code.
//
// The steps are Canny's, cut down for a small grid (192×144): blur, Sobel,
// thin to one pixel (non-max suppression), keep strong edges and the weak
// ones touching them (hysteresis); then walk the edge pixels into chains,
// drop the short ones and simplify the rest (Douglas–Peucker). The
// thresholds come from the frame itself, so a dim room still draws.
//
// Edges flicker between frames: a weak edge that was an edge last frame
// stays one (`prev`), which steadies the lines without smearing motion.

/** A line: x0, y0, x1, y1, … in grid cells. */
export type Polyline = number[];

export interface Traced {
  w: number;
  h: number;
  lines: Polyline[];
  /** Each line's class (`labels`' value along it); all 0 without labels. */
  classes: number[];
  /** This frame's edge pixels, for the next frame's `prev`. */
  edges: Uint8Array;
  /** How much of the edge map changed since `prev` (0–1; 1 without one). */
  changed: number;
}

export interface TraceOptions {
  /** 0–1: how much of the frame's gradient counts as an edge. Higher is
   * busier. */
  detail?: number;
  /** Chains shorter than this many pixels are noise. */
  minLength?: number;
  /** Douglas–Peucker tolerance, in cells. */
  epsilon?: number;
  /** The most points a frame keeps; the shortest lines go first. */
  maxPoints?: number;
  /** The last frame's `edges`. */
  prev?: Uint8Array;
  /** A class per cell (./labels: what's there). A line takes the class of
   * the cells it runs along, and splits where that changes. */
  labels?: Uint8Array;
}

const DIRS = [1, 0, 1, 1, 0, 1, -1, 1, -1, 0, -1, -1, 0, -1, 1, -1];

/** `rgba` (w×h, 4 bytes a pixel) as outlines. */
export function trace(rgba: ArrayLike<number>, w: number, h: number, o: TraceOptions = {}): Traced {
  const detail = o.detail ?? 0.5;
  const n = w * h;
  const gray = new Float32Array(n);
  for (let i = 0; i < n; i++) gray[i] = 0.299 * rgba[i * 4] + 0.587 * rgba[i * 4 + 1] + 0.114 * rgba[i * 4 + 2];
  const soft = blur(gray, w, h);

  // Sobel: magnitude, and the gradient's direction in four bins
  // (0 —, 1 /, 2 |, 3 \).
  const mag = new Float32Array(n);
  const dir = new Uint8Array(n);
  for (let y = 1; y < h - 1; y++) {
    for (let x = 1; x < w - 1; x++) {
      const i = y * w + x;
      const a = soft[i - w - 1], b = soft[i - w], c = soft[i - w + 1];
      const d = soft[i - 1], f = soft[i + 1];
      const g = soft[i + w - 1], k = soft[i + w], l = soft[i + w + 1];
      const gx = c + 2 * f + l - a - 2 * d - g;
      const gy = g + 2 * k + l - a - 2 * b - c;
      mag[i] = Math.hypot(gx, gy);
      const t = Math.atan2(gy, gx);
      dir[i] = (Math.round((t < 0 ? t + Math.PI : t) / (Math.PI / 4)) % 4) as number;
    }
  }

  // Thin: a pixel stays only if it's the ridge across its gradient.
  const thin = new Float32Array(n);
  const across = [1, w + 1, w, w - 1];
  for (let y = 1; y < h - 1; y++) {
    for (let x = 1; x < w - 1; x++) {
      const i = y * w + x;
      const m = mag[i];
      if (!m) continue;
      const s = across[dir[i]];
      if (m >= mag[i + s] && m >= mag[i - s]) thin[i] = m;
    }
  }

  // detail 0 keeps the strongest 10% of ridges, 1 the strongest 60%.
  const high = percentile(thin, 0.9 - 0.5 * detail);
  const low = high * 0.45;
  const edges = new Uint8Array(n);
  const stack: number[] = [];
  for (let i = 0; i < n; i++) {
    if (thin[i] >= high || (o.prev?.[i] && thin[i] >= low * 0.7)) {
      edges[i] = 1;
      stack.push(i);
    }
  }
  while (stack.length) {
    const i = stack.pop()!;
    const x = i % w, y = (i / w) | 0;
    for (let k = 0; k < 16; k += 2) {
      const nx = x + DIRS[k], ny = y + DIRS[k + 1];
      if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
      const j = ny * w + nx;
      if (!edges[j] && thin[j] >= low) {
        edges[j] = 1;
        stack.push(j);
      }
    }
  }

  let changed = 1;
  if (o.prev && o.prev.length === n) {
    let diff = 0, on = 0;
    for (let i = 0; i < n; i++) {
      if (edges[i] !== o.prev[i]) diff++;
      if (edges[i] || o.prev[i]) on++;
    }
    changed = on ? diff / on : 0;
  }

  // More detail keeps shorter strokes too (hair, creases).
  const chains = chainsOf(edges, w, h, o.minLength ?? Math.round(7 - 4 * detail));
  const eps = o.epsilon ?? 0.8;
  let runs: { line: Polyline; cls: number }[] = [];
  for (const c of chains) {
    for (const r of o.labels ? byClass(c, o.labels, w, h) : [{ line: c, cls: 0 }]) {
      const line = simplify(r.line, eps);
      if (line.length >= 4) runs.push({ line, cls: r.cls });
    }
  }
  const max = o.maxPoints ?? 2500;
  let points = runs.reduce((s, r) => s + r.line.length / 2, 0);
  if (points > max) {
    runs.sort((a, b) => length(b.line) - length(a.line));
    while (points > max && runs.length) points -= runs.pop()!.line.length / 2;
  }
  return { w, h, lines: runs.map((r) => r.line), classes: runs.map((r) => r.cls), edges, changed };
}

/** A chain cut where the class under it changes. An edge sits between
 * two things (you and the wall behind you), so a cell's class is the
 * commonest labelled one (not background) around it; a run of fewer than
 * RUN cells takes its neighbour's, so a line doesn't flicker between two. */
function byClass(chain: Polyline, labels: Uint8Array, w: number, h: number): { line: Polyline; cls: number }[] {
  const RUN = 4;
  const m = chain.length / 2;
  const cls = new Array<number>(m);
  const count = new Map<number, number>();
  for (let i = 0; i < m; i++) {
    const x = chain[i * 2], y = chain[i * 2 + 1];
    count.clear();
    for (let dy = -1; dy <= 1; dy++) {
      for (let dx = -1; dx <= 1; dx++) {
        const nx = x + dx, ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
        const c = labels[ny * w + nx];
        if (c) count.set(c, (count.get(c) ?? 0) + 1);
      }
    }
    let best = 0, n = 0;
    for (const [c, k] of count) if (k > n) [best, n] = [c, k];
    cls[i] = best;
  }
  // Short runs take the class before them (or after, at the start).
  for (let i = 0; i < m; ) {
    let j = i;
    while (j < m && cls[j] === cls[i]) j++;
    if (j - i < RUN && (i > 0 || j < m)) {
      const to = i > 0 ? cls[i - 1] : cls[j];
      for (let k = i; k < j; k++) cls[k] = to;
    }
    i = j;
  }
  const out: { line: Polyline; cls: number }[] = [];
  for (let i = 0; i < m; ) {
    let j = i;
    while (j < m && cls[j] === cls[i]) j++;
    // Each run starts where the last ended, so the drawing stays joined.
    const from = i > 0 ? i - 1 : i;
    out.push({ line: chain.slice(from * 2, j * 2), cls: cls[i] });
    i = j;
  }
  return out;
}

/** A 5-tap Gaussian, across then down. */
function blur(src: Float32Array, w: number, h: number): Float32Array {
  const k = [1, 4, 6, 4, 1];
  const tmp = new Float32Array(src.length);
  const out = new Float32Array(src.length);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let s = 0;
      for (let t = -2; t <= 2; t++) s += k[t + 2] * src[y * w + Math.min(w - 1, Math.max(0, x + t))];
      tmp[y * w + x] = s / 16;
    }
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let s = 0;
      for (let t = -2; t <= 2; t++) s += k[t + 2] * tmp[Math.min(h - 1, Math.max(0, y + t)) * w + x];
      out[y * w + x] = s / 16;
    }
  }
  return out;
}

/** The value `q` of the way up the non-zero values (a 256-bucket
 * histogram: exact enough for a threshold). */
function percentile(v: Float32Array, q: number): number {
  let max = 0;
  for (let i = 0; i < v.length; i++) if (v[i] > max) max = v[i];
  if (!max) return Infinity;
  const hist = new Uint32Array(256);
  let count = 0;
  for (let i = 0; i < v.length; i++) {
    if (!v[i]) continue;
    hist[Math.min(255, ((v[i] / max) * 255) | 0)]++;
    count++;
  }
  const want = count * q;
  let seen = 0;
  for (let b = 0; b < 256; b++) {
    seen += hist[b];
    if (seen >= want) return Math.max((b / 255) * max, max * 0.04);
  }
  return max;
}

/** The edge pixels as chains: from the ends first (a pixel with one
 * neighbour), then whatever's left (loops). A walk prefers a straight-on
 * neighbour so lines don't zig-zag. */
function chainsOf(edges: Uint8Array, w: number, h: number, minLength: number): Polyline[] {
  const left = edges.slice();
  const out: Polyline[] = [];
  const neighbours = (x: number, y: number) => {
    let c = 0;
    for (let k = 0; k < 16; k += 2) {
      const nx = x + DIRS[k], ny = y + DIRS[k + 1];
      if (nx >= 0 && ny >= 0 && nx < w && ny < h && left[ny * w + nx]) c++;
    }
    return c;
  };
  const walk = (start: number) => {
    const line: Polyline = [];
    let x = start % w, y = (start / w) | 0;
    let pd = -1;
    for (;;) {
      left[y * w + x] = 0;
      line.push(x, y);
      let next = -1;
      // Straight on first, then the turns nearest it.
      const order = pd < 0 ? [0, 2, 4, 6, 8, 10, 12, 14] : [0, 2, -2, 4, -4, 6, -6, 8].map((d) => (pd + d + 16) % 16);
      for (const k of order) {
        const nx = x + DIRS[k], ny = y + DIRS[k + 1];
        if (nx >= 0 && ny >= 0 && nx < w && ny < h && left[ny * w + nx]) {
          next = k;
          break;
        }
      }
      if (next < 0) break;
      x += DIRS[next];
      y += DIRS[next + 1];
      pd = next;
    }
    if (line.length / 2 >= minLength) out.push(line);
  };
  for (let i = 0; i < left.length; i++) if (left[i] && neighbours(i % w, (i / w) | 0) <= 1) walk(i);
  for (let i = 0; i < left.length; i++) if (left[i]) walk(i);
  return out;
}

/** Douglas–Peucker, without recursion. */
export function simplify(line: Polyline, eps: number): Polyline {
  const m = line.length / 2;
  if (m <= 2) return line.slice();
  const keep = new Uint8Array(m);
  keep[0] = keep[m - 1] = 1;
  const spans: [number, number][] = [[0, m - 1]];
  while (spans.length) {
    const [a, b] = spans.pop()!;
    const ax = line[a * 2], ay = line[a * 2 + 1], bx = line[b * 2], by = line[b * 2 + 1];
    const dx = bx - ax, dy = by - ay;
    const len = Math.hypot(dx, dy) || 1;
    let far = -1, dmax = eps;
    for (let i = a + 1; i < b; i++) {
      const d = Math.abs(dy * line[i * 2] - dx * line[i * 2 + 1] + bx * ay - by * ax) / len;
      if (d > dmax) {
        dmax = d;
        far = i;
      }
    }
    if (far >= 0) {
      keep[far] = 1;
      spans.push([a, far], [far, b]);
    }
  }
  const out: Polyline = [];
  for (let i = 0; i < m; i++) if (keep[i]) out.push(line[i * 2], line[i * 2 + 1]);
  return out;
}

function length(l: Polyline): number {
  let s = 0;
  for (let i = 2; i < l.length; i += 2) s += Math.abs(l[i] - l[i - 2]) + Math.abs(l[i + 1] - l[i - 1]);
  return s;
}
