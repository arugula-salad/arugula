// Charts in a Doc (#964): a bar and a line type, drawn as SVG by Preact, no
// library. `checkSpec` says whether a spec can be drawn and `buildChart`
// lays it out as a tree of plain data (so a spec can look inside it);
// `Chart` turns that tree into SVG. A chart that can't be drawn says why
// and shows its source: never an empty box.
//
// The rules are the daemon's (`crates/daemon/src/docs/embeds.rs`, which has
// the grammar) and say the same things: `doc_fill` refuses a chart that
// breaks them, so they only matter here for a chart that arrives another
// way, but a spec runs both over the same files (`tests/fixtures/docs/`).
// Colours are `--c1` to `--c4` (style.css, from the palette) with light
// fallbacks; text and axes are `currentColor`, so both themes read.

import { h, type ComponentChildren } from "preact";

export const MAX_ROWS = 200;
export const MAX_SERIES = 4;
const FALLBACK = ["#2a6fb0", "#d9822b", "#2f8f63", "#8a55a0"];
const W = 360;
const H = 240;

const oneLine = (s: string) => s.split(/\s+/).filter(Boolean).join(" ");
const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const isNumber = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);

/** Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm). */
function days(y: number, m: number, d: number): number {
  const yy = m <= 2 ? y - 1 : y;
  const era = Math.floor(yy / 400);
  const yoe = yy - era * 400;
  const doy = Math.floor((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5) + d - 1;
  return era * 146097 + yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy - 719468;
}

const ISO = /^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2})(\.\d+)?)?(Z|[+-]\d{2}:?\d{2})?)?$/;

/** Seconds since the epoch for an ISO date, or one with a time (a time with
 * no zone is UTC); null for anything else or a date that isn't one. */
function instant(s: string): number | null {
  const m = ISO.exec(s);
  if (!m) return null;
  const [y, mo, d] = [+m[1], +m[2], +m[3]];
  const leap = (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0;
  const dim = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (mo < 1 || mo > 12 || d < 1 || d > dim[mo - 1]) return null;
  let secs = days(y, mo, d) * 86400;
  if (m[4] === undefined) return secs;
  const [hh, mm, ss] = [+m[4], +m[5], +(m[6] ?? 0) + (m[7] ? +m[7] : 0)];
  if (hh > 23 || mm > 59 || ss >= 60) return null;
  secs += hh * 3600 + mm * 60 + ss;
  const z = m[8];
  if (z && z !== "Z") {
    const digits = z.slice(1).replace(":", "");
    const [oh, om] = [+digits.slice(0, 2), +digits.slice(2)];
    if (oh > 23 || om > 59) return null;
    secs -= (z[0] === "-" ? -1 : 1) * (oh * 3600 + om * 60);
  }
  return secs;
}

/** A line joins its points in row order, so the order has to mean something:
 * numbers going up, dates going forward, or labels the spec listed in
 * `order`. Bars have no such rule. */
function ordered(spec: Record<string, unknown>, x: string, rows: Record<string, unknown>[]): string | null {
  const xs = rows.map((r) => r[x]);
  const rising = (keys: number[], what: string) => {
    for (let i = 1; i < keys.length; i++) {
      if (!(keys[i] > keys[i - 1])) {
        return `a line chart's x must go up: row ${i + 1} (${oneLine(JSON.stringify(xs[i]))}) does not come after row ${i} (${what})`;
      }
    }
    return null;
  };
  if (xs.every((v) => typeof v === "number")) return rising(xs as number[], "numbers");
  const dates = xs.map((v) => (typeof v === "string" ? instant(v) : null));
  if (dates.every((d) => d !== null)) return rising(dates as number[], "dates");
  if (spec.order === undefined) {
    return 'a line chart\'s x must be numbers, dates (2026-01-31), or labels with "order": [...] listing them in order';
  }
  const order = spec.order;
  if (!Array.isArray(order) || order.some((o) => typeof o !== "string")) return "order must be a list of the x labels, in order";
  const at: number[] = [];
  for (const [i, v] of xs.entries()) {
    const p = typeof v === "string" ? order.indexOf(v) : -1;
    if (p < 0) return `row ${i + 1} (${oneLine(JSON.stringify(v))}) is not in order`;
    at.push(p);
  }
  return rising(at, "order");
}

/** Null if the spec can be drawn, else one line saying why not. */
export function checkSpec(spec: unknown): string | null {
  if (!isObject(spec)) return "must be a JSON object";
  if (spec.type !== "bar" && spec.type !== "line") {
    return `type must be "bar" or "line", got ${oneLine(JSON.stringify(spec.type) ?? "nothing")}`;
  }
  if (spec.title !== undefined && typeof spec.title !== "string") return "title must be a string";
  const x = spec.x;
  if (typeof x !== "string" || !x) return "x must name the key that labels each row";
  const ys = Array.isArray(spec.y) ? spec.y : [spec.y];
  if (ys.length < 1 || ys.length > MAX_SERIES || ys.some((y) => typeof y !== "string" || !y)) {
    return `y must be a key, or a list of 1 to ${MAX_SERIES} keys`;
  }
  const rows = spec.rows;
  if (!Array.isArray(rows) || rows.length === 0) return "rows must be a non-empty list";
  if (rows.length > MAX_ROWS) return `${rows.length} rows; the cap is ${MAX_ROWS}`;
  for (const [i, row] of rows.entries()) {
    if (!isObject(row)) return `row ${i + 1} must be an object`;
    if (typeof row[x] !== "string" && typeof row[x] !== "number") return `row ${i + 1} has no ${JSON.stringify(x)} label`;
    for (const y of ys as string[]) if (!isNumber(row[y])) return `row ${i + 1}: ${JSON.stringify(y)} must be a number`;
  }
  return spec.type === "line" ? ordered(spec, x, rows as Record<string, unknown>[]) : null;
}

/** A chart fence's source, parsed, or one line saying why it can't be:
 * `chart: …`, as the daemon's refusal says it. */
export function parseChart(source: string): { spec: Spec } | { why: string } {
  let spec: unknown;
  try {
    spec = JSON.parse(source);
  } catch (e) {
    return { why: `chart: not JSON (${oneLine((e as Error).message)})` };
  }
  const why = checkSpec(spec);
  return why ? { why: `chart: ${why}` } : { spec: spec as Spec };
}

/** Null if the chart can be drawn, else why not. */
export function checkChart(source: string): string | null {
  const p = parseChart(source);
  return "why" in p ? p.why : null;
}

/** `[chart: Sales by month (bar, 3 rows)]`; `[chart]` for one that can't be
 * read: what a Doc's text has in place of the fence. */
export function summary(source: string): string {
  const p = parseChart(source);
  if ("why" in p) return "[chart]";
  const n = p.spec.rows.length;
  const rows = `${n} row${n === 1 ? "" : "s"}`;
  return p.spec.title ? `[chart: ${oneLine(p.spec.title)} (${p.spec.type}, ${rows})]` : `[chart: ${p.spec.type}, ${rows}]`;
}

export interface Spec {
  type: "bar" | "line";
  title?: string;
  x: string;
  y: string | string[];
  rows: Record<string, number | string>[];
  order?: string[];
}

/** A node of the drawing, as data. */
export interface El {
  tag: string;
  attrs: Record<string, string | number>;
  text?: string;
  kids: El[];
}

const fmt = (n: number) => (Math.abs(n) >= 10000 ? `${Number((n / 1000).toPrecision(3))}k` : String(Number(n.toPrecision(3))));

/** Ticks from `lo` to `hi` on a 1, 2 or 5 step. */
export function niceTicks(lo: number, hi: number, count = 4): number[] {
  if (hi === lo) hi = lo + 1;
  const raw = (hi - lo) / count;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map((m) => m * mag).find((s) => s >= raw)!;
  const ticks: number[] = [];
  for (let t = Math.floor(lo / step) * step; t < hi + step - 1e-9; t += step) {
    ticks.push(Number(t.toPrecision(12)));
    if (t >= hi - 1e-9) break;
  }
  return ticks;
}

/** One sentence of values, for the <title> and aria-label. */
export function describe(spec: Spec): string {
  const ys = ([] as string[]).concat(spec.y);
  const parts = ys.map((y) => `${y}: ${spec.rows.map((r) => `${r[spec.x]} ${r[y]}`).join(", ")}`);
  return `${spec.title ? `${spec.title}, ` : ""}${spec.type} chart. ${parts.join(". ")}`;
}

/** The chart, laid out. `spec` has passed `checkSpec`. */
export function buildChart(spec: Spec): El {
  const ys = ([] as string[]).concat(spec.y);
  const rows = spec.rows;
  const make = (tag: string, attrs: El["attrs"] = {}, text?: string | number): El => ({
    tag,
    attrs,
    kids: [],
    ...(text !== undefined ? { text: String(text) } : {}),
  });
  const color = (i: number) => `var(--c${i + 1}, ${FALLBACK[i]})`;
  const label = describe(spec);
  const svg = make("svg", {
    viewBox: `0 0 ${W} ${H}`,
    role: "img",
    "aria-label": label,
    class: "chart",
    style: "width:100%;height:auto;max-width:36rem;display:block;font:11px system-ui,sans-serif;color:inherit",
  });
  const add = (el: El) => svg.kids.push(el);
  add(make("title", {}, label));

  // Room for the title, the legend, and axis labels; the plot is what is left.
  let top = 8;
  if (spec.title) {
    add(make("text", { x: W / 2, y: 18, "text-anchor": "middle", "font-size": 14, "font-weight": 600, fill: "currentColor" }, spec.title));
    top = 28;
  }
  if (ys.length > 1) {
    let x = 44;
    ys.forEach((y, i) => {
      add(make("rect", { x, y: top + 1, width: 10, height: 10, rx: 2, style: `fill:${color(i)}` }));
      add(make("text", { x: x + 14, y: top + 10, fill: "currentColor" }, y));
      x += 24 + y.length * 6;
    });
    top += 18;
  }
  const plot = { l: 44, r: W - 10, t: top + 6, b: H - 44 };
  const width = plot.r - plot.l;
  const height = plot.b - plot.t;

  // Bars start at zero, and so does a line's axis when its values are all on one side of it.
  const values = rows.flatMap((r) => ys.map((y) => r[y] as number));
  const ticks = niceTicks(Math.min(0, ...values), Math.max(0, ...values));
  const lo = ticks[0];
  const hi = ticks[ticks.length - 1];
  const yAt = (v: number) => plot.b - ((v - lo) / (hi - lo)) * height;

  for (const t of ticks) {
    add(make("line", { x1: plot.l, x2: plot.r, y1: yAt(t), y2: yAt(t), stroke: "currentColor", "stroke-opacity": t === 0 ? 0.6 : 0.15 }));
    add(make("text", { x: plot.l - 5, y: yAt(t) + 4, "text-anchor": "end", fill: "currentColor" }, fmt(t)));
  }

  const n = rows.length;
  const xs = rows.map((r) => r[spec.x]);
  const numeric = spec.type === "line" && xs.every((x) => typeof x === "number");
  const slot = width / n;
  // Bars sit in the middle of their slot; a line's points run edge to edge.
  const xAt = (i: number) => {
    if (spec.type === "bar") return plot.l + slot * (i + 0.5);
    if (n === 1) return (plot.l + plot.r) / 2;
    if (numeric && xs[n - 1] !== xs[0]) return plot.l + (((xs[i] as number) - (xs[0] as number)) / ((xs[n - 1] as number) - (xs[0] as number))) * width;
    return plot.l + (width * i) / (n - 1);
  };

  if (spec.type === "bar") {
    const barW = Math.max(1, (slot * 0.8) / ys.length);
    ys.forEach((y, s) => {
      rows.forEach((r, i) => {
        const x = xAt(i) - (slot * 0.8) / 2 + s * barW;
        const a = yAt(Math.max(0, r[y] as number));
        const b = yAt(Math.min(0, r[y] as number));
        add(make("rect", { x, y: a, width: Math.max(1, barW - 1), height: Math.max(0, b - a), style: `fill:${color(s)}`, "data-bar": y }));
      });
    });
  } else {
    ys.forEach((y, s) => {
      const d = rows.map((r, i) => `${i ? "L" : "M"}${xAt(i).toFixed(1)} ${yAt(r[y] as number).toFixed(1)}`).join(" ");
      add(make("path", { d, fill: "none", "stroke-width": 2, "stroke-linejoin": "round", style: `stroke:${color(s)}`, "data-line": y }));
      // Dots on short series only; 200 of them are just noise.
      if (n <= 40) rows.forEach((r, i) => add(make("circle", { cx: xAt(i), cy: yAt(r[y] as number), r: 2.5, style: `fill:${color(s)}` })));
    });
  }

  // Labels thin out so that they never overlap: about 6 px a character.
  const labels = xs.map((x) => String(x));
  const widest = Math.min(12, Math.max(...labels.map((l) => l.length)));
  const every = Math.max(1, Math.ceil((widest * 6 + 6) / (spec.type === "bar" ? slot : width / Math.max(1, n - 1))));
  labels.forEach((l, i) => {
    if (i % every === 0) add(make("text", { x: xAt(i), y: plot.b + 14, "text-anchor": "middle", fill: "currentColor" }, l.length > 12 ? `${l.slice(0, 11)}…` : l));
  });
  add(make("text", { x: (plot.l + plot.r) / 2, y: H - 6, "text-anchor": "middle", "fill-opacity": 0.75, fill: "currentColor" }, spec.x));
  if (ys.length === 1) {
    const mid = (plot.t + plot.b) / 2;
    add(make("text", { x: 10, y: mid, "text-anchor": "middle", "fill-opacity": 0.75, fill: "currentColor", transform: `rotate(-90 10 ${mid})` }, ys[0]));
  }
  return svg;
}

const vnode = (e: El): ComponentChildren => h(e.tag, e.attrs, e.text ?? e.kids.map(vnode));

/** A chart fence: the chart, or why it isn't drawn and its source. */
export function Chart({ source }: { source: string }) {
  const p = parseChart(source);
  if ("why" in p) {
    return (
      <div class="chart-note" data-chart-note>
        <p class="dim">Chart not drawn: {p.why.replace(/^chart: /, "")}</p>
        <pre>{source}</pre>
      </div>
    );
  }
  return <div class="chart-box">{vnode(buildChart(p.spec))}</div>;
}
