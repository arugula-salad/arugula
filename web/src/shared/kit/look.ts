// Look (design lift §4.3): the page's role tokens (./tokens.css) painted
// from a theme's colors and three sliders, light↔dark, cool↔warm and
// flat↔deep, each 0 to 1. At 0.5 each, the default, a theme paints exactly
// the colors it has always had; moving one works in OKLCH on the neutrals
// (surfaces, ink and lines), never on the signal colors. Whatever the
// sliders, `fix()` keeps every word legible (WCAG AA, 4.5:1) on the
// surfaces it sits on.

/** A theme's colors: style.css's legacy custom properties, without the
 * `--`. */
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
  /** 0 cool (blue-grey), 1 warm (stone); the hue never passes through
   * green. */
  warmth: number;
  /** 0 flat, 1 deep: how far apart the surfaces step, and the shadows. */
  depth: number;
}

export const SLIDERS: Sliders = { light: 0.5, warmth: 0.5, depth: 0.5 };

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

// ---- OKLCH

type Lch = [l: number, c: number, h: number];

const toLinear = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const toGamma = (v: number) => (v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055);

export function oklch(hex: string): Lch {
  const [r, g, b] = channels(hex).map((v) => toLinear(v / 255));
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return [L, Math.hypot(A, B), ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360];
}

export function fromOklch([L, C, H]: Lch): string {
  const h = (H * Math.PI) / 180;
  const [A, B] = [C * Math.cos(h), C * Math.sin(h)];
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  const rgb = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
  return toHex(rgb.map((v) => toGamma(Math.min(1, Math.max(0, v))) * 255));
}

// ---- the sliders

const COOL = 250;
const WARM = 65;
/** The greens the neutral tint never passes through (v5). */
const GREEN = [100, 170];

/** From hue `a` to `b` by `t`, the way round that misses the greens. */
function turn(a: number, b: number, t: number): number {
  const short = ((b - a + 540) % 360) - 180;
  const long = short > 0 ? short - 360 : short + 360;
  const crosses = (d: number) => {
    for (let i = 1; i < 20; i++) {
      const h = (a + (d * i) / 20 + 360) % 360;
      if (h > GREEN[0] && h < GREEN[1]) return true;
    }
    return false;
  };
  const d = crosses(short) ? long : short;
  return (a + d * t + 360) % 360;
}

/** A neutral under the warmth slider: its hue turned toward cool or warm,
 * and its chroma toward a faint tint. */
function tint([l, c, h]: Lch, warmth: number, chroma: number): Lch {
  const t = (warmth - 0.5) * 2;
  if (!t) return [l, c, h];
  const to = t < 0 ? COOL : WARM;
  // A grey has no hue of its own: take the target's.
  const from = c < 0.002 ? to : h;
  return [l, c + (chroma - c) * Math.abs(t), turn(from, to, Math.abs(t))];
}

const SURFACES = ["bg", "surface", "letterbox", "raised", "raised-hover", "muted"] as const;
const INKS = ["fg", "dim", "subtle"] as const;

/** The theme's colors moved by the sliders, before `fix()`: the surfaces
 * step from `bg` (light and depth move them), and the warmth tints the
 * surfaces and ink. The signal colors stay as they are. */
export function slide(ui: Ui, dark: boolean, s: Sliders = SLIDERS): Ui {
  if (s.light === SLIDERS.light && s.warmth === SLIDERS.warmth && s.depth === SLIDERS.depth) return { ...ui };
  const out = { ...ui };
  const base = oklch(ui.bg);
  const shift = (SLIDERS.light - s.light) * 0.14;
  const step = 0.3 + 1.4 * s.depth;
  for (const k of SURFACES) {
    const [l, c, h] = oklch(ui[k]);
    const L = Math.min(0.99, Math.max(0.04, base[0] + shift + (l - base[0]) * step));
    out[k] = fromOklch(tint([L, c, h], s.warmth, dark ? 0.03 : 0.022));
  }
  for (const k of INKS) out[k] = fromOklch(tint(oklch(ui[k]), s.warmth, 0.02));
  return out;
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

/** The colors as the page uses them (M113): a word in any of them reads at
 * WCAG AA (4.5:1) on the backgrounds it sits on (bg, surface and raised, and
 * the selected row's tint). A color that already does stays; one that
 * doesn't is moved toward white (dark) or black (light) as little as it
 * takes, so it keeps its hue. A primary button's label likewise reads on
 * the accent. */
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

/** The custom properties a look sets on the root: tokens.css's roles, the
 * few legacy colors no role covers yet, and the depth's shadow strength and
 * highlights. */
export function paint(ui: Ui, dark: boolean, s: Sliders = SLIDERS): Record<string, string> {
  const u = fix(slide(ui, dark, s), dark);
  const deep = Math.max(0, s.depth - 0.5) * 2;
  return {
    canvas: u.bg,
    rail: u.surface,
    section: u.surface,
    card: u.surface,
    "card-raised": u.raised,
    paper: u.bg,
    well: u.letterbox,
    ink: u.fg,
    "ink-dim": u.dim,
    "ink-faint": u.muted,
    "well-ink": u.fg,
    "well-dim": u.dim,
    "on-accent": u["on-color"],
    "on-needs": u["on-color"],
    line: u.raised,
    hover: u["raised-hover"],
    focus: u.accent,
    accent: u.accent,
    needs: u.warn,
    fail: u.danger,
    review: u.info,
    done: u.ok,
    // Flat to deep: the shadows' strength (1 at the middle), and a top
    // highlight and sheen only past it.
    "shadow-k": String(+((dark ? 1 : 0.35) * s.depth * 2).toFixed(3)),
    highlight: `inset 0 1px 0 rgb(255 255 255 / ${+((dark ? 0.07 : 0.9) * deep).toFixed(3)})`,
    sheen: `linear-gradient(180deg, rgb(255 255 255 / ${+((dark ? 0.03 : 0.5) * deep).toFixed(3)}), transparent 90px)`,
    // Legacy colors with no role: each screen's phase drops its uses.
    subtle: u.subtle,
    peach: u.peach,
    teal: u.teal,
    pink: u.pink,
    merged: u.merged,
  };
}
