// Look (design lift §4.3, §4.4): the page's role tokens (./tokens.css)
// painted from the brand and three sliders, light↔dark, cool↔warm and
// flat↔deep, each 0 to 1. A preset (Arugula, the default, and four
// others) is the accent's hue and loudness and the neutrals' tint in each
// theme; the sliders move the neutrals (surfaces, ink and lines) from
// there. Needs-you amber, failing red and in-review blue are the same
// whatever the preset. Whatever the sliders, every word reads at WCAG AA
// (4.5:1) on the surfaces it sits on. Ported from the brand v2 prototype's
// `theme()` (design-pass/prototypes/brand2).

/** A theme's colors: style.css's legacy custom properties, without the
 * `--`. The brand paints the roles; these still give the terminals, the
 * editors and Docs their colors, and the few legacy hues no role covers. */
export interface Ui {
  bg: string;
  fg: string;
  dim: string;
  surface: string;
  letterbox: string;
  raised: string;
  "raised-hover": string;
  accent: string;
  danger: string;
  ok: string;
  warn: string;
  info: string;
  peach: string;
  teal: string;
  pink: string;
  /** A merged change (the item block's purple pill). */
  merged: string;
  muted: string;
  subtle: string;
  /** Text on an accent, danger or ok fill. */
  "on-color": string;
}

export interface Sliders {
  /** 0 the lightest, 1 the darkest the theme goes. */
  light: number;
  /** 0 cool (blue-grey), 0.5 neutral grey, 1 warm (stone). */
  warmth: number;
  /** 0 flat, 1 deep: how far apart the surfaces step, and the shadows. */
  depth: number;
}

// ---- sRGB, WCAG contrast

const channels = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const toHex = (c: number[]) => `#${c.map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0")).join("")}`;

function luminance(hex: string): number {
  const [r, g, b] = channels(hex).map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG's contrast ratio between two `#rrggbb` colors. */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

/** `a` moved `t` of the way to `b`, in sRGB (CSS's `color-mix(in srgb)`). */
export function mix(a: string, b: string, t: number): string {
  const [x, y] = [channels(a), channels(b)];
  return toHex(x.map((v, i) => v + (y[i] - v) * t));
}

// ---- OKLab and OKLCH

type Rgb = number[];

const toLinear = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const toGamma = (v: number) => (v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055);

function oklab(c: Rgb): [number, number, number] {
  const [r, g, b] = c.map((v) => toLinear(v / 255));
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

/** OKLab to linear sRGB, unclamped. */
function linear(L: number, A: number, B: number): Rgb {
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

const gamma = (x: Rgb): Rgb => x.map((v) => Math.round(toGamma(Math.min(1, Math.max(0, v))) * 255));

/** A color's OKLCH: lightness, chroma and hue in degrees. */
export function oklch(hex: string): [l: number, c: number, h: number] {
  const [L, A, B] = oklab(channels(hex));
  return [L, Math.hypot(A, B), ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360];
}

/** OKLCH to sRGB; out of gamut, the chroma shrinks and the hue stays. */
function lch(L: number, C: number, h: number): Rgb {
  const r = (h * Math.PI) / 180;
  let x = linear(L, C * Math.cos(r), C * Math.sin(r));
  for (let i = 0; i < 80 && !x.every((v) => v > -1e-4 && v < 1.0001); i++) {
    C *= 0.96;
    x = linear(L, C * Math.cos(r), C * Math.sin(r));
  }
  return gamma(x);
}

/** HSL (degrees, percent) to sRGB 0–255: the neutrals' lightness ladder. */
function hsl(h: number, s: number, l: number): Rgb {
  s /= 100;
  l = Math.max(0, Math.min(100, l)) / 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
  return [f(0), f(8), f(4)].map((v) => Math.round(v * 255));
}

// ---- the brand (§4.4, decided 2026-11-25)

/** A preset's colors in one theme: the accent's OKLCH hue and how loud it
 * is (× the theme's own chroma), and the neutrals' tint hue and strength
 * (0: none, pure grey). */
export interface Hue {
  hue: number;
  chroma: number;
  tint: number;
  tintStrength: number;
}

/** A named starting point: its hues in each theme, and where it puts the
 * warmth and depth sliders. */
export interface Preset {
  id: string;
  name: string;
  dark: Hue;
  light: Hue;
  warmth: number;
  depth: number;
}

const both = (h: Hue) => ({ dark: h, light: h });

/** Arugula first, the default: Aaron's picks, a near-black with a faint
 * blue tint and a saturated green (#78ce57), and a flat grey paper with a
 * deep olive-green (#3e5a05) and bright fills. */
export const PRESETS: Preset[] = [
  {
    id: "arugula",
    name: "Arugula",
    dark: { hue: 138, chroma: 1.4, tint: 243, tintStrength: 1 },
    light: { hue: 128, chroma: 1.38, tint: 0, tintStrength: 0 },
    warmth: 0,
    depth: 0,
  },
  { id: "moss", name: "Moss", ...both({ hue: 132, chroma: 1.25, tint: 0, tintStrength: 0 }), warmth: 0.5, depth: 0 },
  { id: "sea-glass", name: "Sea glass", ...both({ hue: 178, chroma: 1.3, tint: 215, tintStrength: 0.25 }), warmth: 0.3, depth: 0 },
  { id: "stone-ink", name: "Stone & ink", ...both({ hue: 278, chroma: 1.3, tint: 250, tintStrength: 0.2 }), warmth: 0.42, depth: 0 },
  { id: "graphite-acid", name: "Graphite & acid", ...both({ hue: 114, chroma: 1.7, tint: 0, tintStrength: 0 }), warmth: 0.5, depth: 0.6 },
];

/** The preset with this id, or Arugula. */
export const preset = (id?: string): Preset => PRESETS.find((p) => p.id === id) ?? PRESETS[0];

/** Where a preset starts the sliders in a theme: the darkest dark, or a
 * grey paper (38 of the light range's 49), and the preset's warmth and
 * depth. */
export const sliders = (dark: boolean, p: Preset = PRESETS[0]): Sliders => ({ light: dark ? 1 : 38 / 49, warmth: p.warmth, depth: p.depth });

/** The fixed signals' OKLCH hues: needs-you amber, failing red, in-review
 * blue. */
export const SIGNAL_HUES = { needs: 69, fail: 27, review: 251 } as const;

/** The signals an accent hue sits too close to: 28° from amber and red,
 * 22° from in-review blue, or the accent reads as one. Every preset clears
 * it (look.test.mjs). */
export function tooClose(hue: number): string[] {
  const apart = (a: number, b: number) => {
    const d = Math.abs(a - b) % 360;
    return d > 180 ? 360 - d : d;
  };
  const near: string[] = [];
  if (apart(hue, SIGNAL_HUES.needs) < 28) near.push("needs");
  if (apart(hue, SIGNAL_HUES.fail) < 28) near.push("fail");
  if (apart(hue, SIGNAL_HUES.review) < 22) near.push("review");
  return near;
}

/** The brand's floor: a little over AA, so rounding never drops a word
 * under 4.5. */
const FLOOR = 4.6;
const worst = (c: Rgb, on: Rgb[]) => Math.min(...on.map((b) => contrast(toHex(c), toHex(b))));

/** `c`, stepped toward white (dark) or black (light) until it reads on
 * every one of `on`. */
function lift(c: Rgb, on: Rgb[], dark: boolean): Rgb {
  for (let i = 0; i < 50 && worst(c, on) < FLOOR; i++) c = c.map((v) => Math.round(dark ? v + (255 - v) * 0.06 : v * 0.94));
  return c;
}

/** The accent's words: its hue held, its lightness stepped until it reads
 * on every surface it sits on. */
function accentText(h: number, C: number, dark: boolean, on: Rgb[]): Rgb {
  let L = dark ? 0.771 : 0.519;
  let c = lch(L, C, h);
  for (let i = 0; i < 60 && worst(c, on) < FLOOR; i++) {
    L += dark ? 0.01 : -0.01;
    c = lch(L, C, h);
  }
  return c;
}

/** The neutrals, ink and fixed signals for a theme and sliders, tinted by
 * the preset's hue. */
function neutrals(dark: boolean, s: Sliders, hue: Hue) {
  // The light slider within the theme (brand v2's 0–49 light, 50–100
  // dark), as the canvas's HSL lightness: 96% to 82%, or 18% to 7%.
  const L = dark ? 18 - 11 * s.light : 96 - 14 * s.light;
  const dp = s.depth;
  // Warmth: a blue-grey (216°) at 0, grey at 0.5, stone (38°) at 1.
  const sf = Math.abs(s.warmth - 0.5) * 2;
  const wh = s.warmth < 0.5 ? 216 : 38;
  const sat = sf * (dark ? (s.warmth < 0.5 ? 16 : 14) : s.warmth < 0.5 ? 20 : 30);
  // The preset's tint, in OKLab, capped so a surface never reads as a
  // color.
  const tc = hue.tintStrength * (dark ? 0.026 : 0.02);
  const r = (hue.tint * Math.PI) / 180;
  const cap = dark ? 0.024 : 0.028;
  const N = (k: number, l: number): Rgb => {
    const [L0, a, b] = oklab(hsl(wh, sat * k, l));
    let [A, B] = [a + tc * Math.cos(r) * k, b + tc * Math.sin(r) * k];
    const c = Math.hypot(A, B);
    if (c > cap) [A, B] = [(A * cap) / c, (B * cap) / c];
    return gamma(linear(L0, A, B));
  };
  if (dark) {
    const v = {
      canvas: N(1, L),
      rail: N(1, L - 1.6),
      section: N(0.9, L + 2.6 + 1.6 * dp),
      card: N(0.85, L + 5.6 + 2.6 * dp),
      raised: N(0.8, L + 10 + 2 * dp),
      well: N(1, Math.max(2.5, L - 4.2)),
      paper: N(0.7, L + 7 + 2.5 * dp),
      ink: N(0.5, 92),
      wellInk: N(0.4, 89),
      faint: N(0.5, 46),
      edge: N(1.4, 88),
    };
    const on = [v.canvas, v.section, v.card, v.raised, v.paper];
    return {
      ...v,
      on,
      dim: lift(N(0.6, 65), on, true),
      wellDim: lift(N(0.5, 66), [v.well, v.raised], true),
      needs: lift(channels("#eea54a"), on, true),
      fail: lift(channels("#ec6c5f"), on, true),
      review: lift(channels("#8fb4dc"), on, true),
      onNeeds: channels("#2a1d00"),
      lines: [0.085, 0.16, 0.035],
      shadow: 1,
    };
  }
  const v = {
    canvas: N(1, L),
    rail: N(1, L - 2.6),
    section: N(0.8, Math.min(98.6, L + 2.6 + 1.4 * dp)),
    card: N(0.6, Math.min(99.5, L + 5 + 2.5 * dp)),
    raised: N(1, L - 5.5),
    well: N(0.8, L - 3.5),
    paper: N(0.35, 99.6),
    ink: N(0.9, 12),
    wellInk: N(0.8, 14),
    faint: N(0.5, 58),
    edge: N(1.4, 20),
  };
  const on = [v.canvas, v.section, v.card, v.raised, v.well];
  return {
    ...v,
    on,
    dim: lift(N(0.8, 40), on, false),
    wellDim: lift(N(0.8, 36), [v.well, v.raised], false),
    needs: lift(channels("#8f5000"), on, false),
    fail: lift(channels("#b3342a"), on, false),
    review: lift(channels("#2c64a3"), on, false),
    onNeeds: channels("#ffffff"),
    lines: [0.13, 0.25, 0.04],
    shadow: 0.5,
  };
}

// ---- legible whatever the sliders (M113, #1028)

/** `color`, moved toward `toward` only as far as it takes to read at 4.5:1
 * on every one of `on`. */
function legibleOn(color: string, on: string[], toward: string, also?: (c: string) => boolean): string {
  const ok = (c: string) => on.every((b) => contrast(c, b) >= 4.5) && (also?.(c) ?? true);
  if (ok(color)) return color;
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 12; i++) {
    const t = (lo + hi) / 2;
    if (ok(mix(color, toward, t))) hi = t;
    else lo = t;
  }
  return mix(color, toward, hi);
}

/** The words the chrome writes in each color: the dimmed, the accent and
 * the status colors. */
const TEXT = ["fg", "dim", "subtle", "accent", "warn", "ok", "danger", "info", "peach", "teal", "pink", "merged"] as const;

/** The "on" fill (buttons.css's --on-fill, 18% of the accent): `accent` over
 * `base`. */
export const onFill = (accent: string, base: string) => mix(base, accent, 0.18);

/** The selected row's tint (style.css's --selected, 14% of the accent over
 * nothing): `accent` over `base`. */
export const selectedFill = (accent: string, base: string) => mix(base, accent, 0.14);

/** A theme's colors as the terminals, editors and Docs use them (M113): a
 * word in any of them reads at WCAG AA (4.5:1) on the backgrounds it sits
 * on (bg, surface and raised, and the selected row's tint). A color that
 * already does stays; one that doesn't is moved toward white (dark) or
 * black (light) as little as it takes, so it keeps its hue. A primary
 * button's label likewise reads on the accent. */
export function fix(ui: Ui, dark: boolean): Ui {
  const u = { ...ui };
  const on = [u.bg, u.surface, u.raised];
  const end = dark ? "#ffffff" : "#000000";
  for (const k of TEXT) {
    // The accent is also the "on" text (--on-fill's), so it reads on its own
    // tint over the page and over a card: block bars and popovers sit on
    // surface.
    const onTint = (c: string) => [u.bg, u.surface].every((b) => contrast(c, onFill(c, b)) >= 4.5);
    u[k] = legibleOn(u[k], on, end, k === "accent" ? onTint : undefined);
  }
  // A word on the selected row's tint (--selected, #756) too, over the page
  // and over a sidebar. The accent is final by now, so the tint is.
  const picked = [u.bg, u.surface].map((b) => selectedFill(u.accent, b));
  for (const k of TEXT) if (k !== "accent") u[k] = legibleOn(u[k], [...on, ...picked], end);
  const label = [u["on-color"], "#000000", "#ffffff"].sort((a, b) => contrast(b, u.accent) - contrast(a, u.accent));
  if (contrast(u["on-color"], u.accent) < 4.5) u["on-color"] = label[0];
  return u;
}

// ---- the role tokens

const rgba = (c: Rgb, a: number) => `rgb(${c.join(" ")} / ${a})`;
const n = (v: number) => +v.toFixed(3);

/** The custom properties a look sets on the root: tokens.css's roles, the
 * depth's shadows and highlights, and the few legacy colors no role covers
 * yet (the theme's `ui`, made legible on the brand's surfaces). */
export function paint(ui: Ui, dark: boolean, s: Sliders, p: Preset = PRESETS[0]): Record<string, string> {
  const hue = dark ? p.dark : p.light;
  const b = neutrals(dark, s, hue);
  const accent = accentText(hue.hue, (dark ? 0.126 : 0.14) * hue.chroma, dark, [...b.on, b.well]);
  // A fill (a primary button, send, a bar, the logo) can be brighter than
  // the accent's words: in light, a loud or yellow-green accent stays
  // bright, with dark words on it, rather than going olive.
  const lit = !dark && (hue.chroma > 1.3 || (hue.hue >= 85 && hue.hue <= 118));
  const fill = lit ? lch(0.86, (0.14 + 0.02 * hue.chroma) * hue.chroma, hue.hue) : accent;
  let onAccent: Rgb = [255, 255, 255];
  if (dark || lit) {
    let l = 0.26;
    onAccent = lch(l, 0.05, hue.hue);
    for (let i = 0; i < 20 && contrast(toHex(onAccent), toHex(fill)) < FLOOR; i++) onAccent = lch((l -= 0.02), 0.05, hue.hue);
  }
  const surfaces = b.on.map(toHex);
  const legacy = (c: string) => legibleOn(c, surfaces, dark ? "#ffffff" : "#000000");
  const [line, lineStrong, hover] = b.lines;
  const [dp, k] = [s.depth, b.shadow];
  return {
    canvas: toHex(b.canvas),
    rail: toHex(b.rail),
    section: toHex(b.section),
    card: toHex(b.card),
    "card-raised": toHex(b.raised),
    paper: toHex(b.paper),
    well: toHex(b.well),
    ink: toHex(b.ink),
    "ink-dim": toHex(b.dim),
    "ink-faint": toHex(b.faint),
    "well-ink": toHex(b.wellInk),
    "well-dim": toHex(b.wellDim),
    "on-accent": toHex(onAccent),
    "on-needs": toHex(b.onNeeds),
    line: rgba(b.edge, line),
    "line-strong": rgba(b.edge, lineStrong),
    hover: rgba(b.edge, hover),
    focus: toHex(accent),
    accent: toHex(accent),
    "accent-fill": toHex(fill),
    needs: toHex(b.needs),
    fail: toHex(b.fail),
    review: toHex(b.review),
    done: legacy(ui.ok),
    // Flat to deep: the shadows, a top highlight and a sheen, none at 0.
    "shadow-card": `0 1px 2px rgb(0 0 0 / ${n(0.22 * dp * k)}), 0 ${n(1 + 4 * dp)}px ${n(4 + 8 * dp)}px -${n(2 + 2 * dp)}px rgb(0 0 0 / ${n(0.4 * dp * k)})`,
    "shadow-raised": `0 2px 6px rgb(0 0 0 / ${n(0.3 * k)}), 0 18px 40px -10px rgb(0 0 0 / ${n(0.6 * k)})`,
    highlight: `inset 0 1px 0 rgb(255 255 255 / ${n((dark ? 0.07 : 0.9) * dp)})`,
    sheen: `linear-gradient(180deg, rgb(255 255 255 / ${n((dark ? 0.025 : 0.4) * dp)}), transparent 90px)`,
    // Legacy colors with no role: each screen's phase drops its uses.
    subtle: legacy(ui.subtle),
    peach: legacy(ui.peach),
    teal: legacy(ui.teal),
    pink: legacy(ui.pink),
    merged: legacy(ui.merged),
  };
}
