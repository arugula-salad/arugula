// How the page looks, on this device: a palette (the page's colors and the
// terminals'), a finish (flat, Aqua, brushed metal, glass) and how round
// the panes' corners are. Kept in this browser, applied before the first
// draw so a reload doesn't flash the default.

import type { ITheme } from "@xterm/xterm";
import { theme as mochaTerm } from "./theme";

/** The page's colors: style.css's custom properties, without the `--`. */
interface Ui {
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
  muted: string;
  subtle: string;
  /** Text on an accent, danger or ok fill. */
  "on-color": string;
}

export interface Palette {
  id: string;
  name: string;
  light: boolean;
  ui: Ui;
  term: ITheme;
}

/** The sixteen colors, in xterm's order, with fg/bg/cursor around them. */
function term(fg: string, bg: string, cursor: string, sel: string, c: string[]): ITheme {
  const [black, red, green, yellow, blue, magenta, cyan, white, ...b] = c;
  return {
    foreground: fg,
    background: bg,
    cursor,
    cursorAccent: bg,
    selectionBackground: sel,
    black,
    red,
    green,
    yellow,
    blue,
    magenta,
    cyan,
    white,
    brightBlack: b[0],
    brightRed: b[1],
    brightGreen: b[2],
    brightYellow: b[3],
    brightBlue: b[4],
    brightMagenta: b[5],
    brightCyan: b[6],
    brightWhite: b[7],
  };
}

export const PALETTES: Palette[] = [
  {
    id: "mocha",
    name: "Mocha",
    light: false,
    ui: {
      bg: "#1e1e2e",
      fg: "#cdd6f4",
      dim: "#7f849c",
      surface: "#181825",
      letterbox: "#11111b",
      raised: "#313244",
      "raised-hover": "#45475a",
      accent: "#cba6f7",
      danger: "#f38ba8",
      ok: "#a6e3a1",
      warn: "#f9e2af",
      info: "#89b4fa",
      peach: "#fab387",
      teal: "#94e2d5",
      pink: "#f5c2e7",
      muted: "#585b70",
      subtle: "#a6adc8",
      "on-color": "#11111b",
    },
    term: mochaTerm,
  },
  {
    // M108 (#1023): the workspace's own dark palette, from #1018's mission
    // control: near-black slate, blue-gray lines, a restrained cyan for
    // selection and focus, lavender second, amber for attention. The dark
    // default with the new workspace. Every text color passes WCAG AA (4.5)
    // on bg, surface and raised; muted is only ever a fill.
    id: "slate",
    name: "Slate",
    light: false,
    ui: {
      bg: "#0c1118",
      fg: "#d6dee9",
      dim: "#8b98ab",
      surface: "#090d13",
      letterbox: "#06090d",
      raised: "#1a2230",
      "raised-hover": "#243042",
      accent: "#56c8dc",
      danger: "#f2778a",
      ok: "#5fd394",
      warn: "#f0b45a",
      info: "#7aa7f0",
      peach: "#f29b6b",
      teal: "#5fd3c4",
      pink: "#ad9cf2",
      muted: "#3a4658",
      subtle: "#b4c0d0",
      "on-color": "#06090d",
    },
    term: term("#d6dee9", "#0c1118", "#56c8dc", "#56c8dc40", [
      "#1a2230", "#f2778a", "#5fd394", "#f0b45a", "#7aa7f0", "#ad9cf2", "#56c8dc", "#b4c0d0",
      "#5c6a80", "#ff96a6", "#82e3ad", "#ffcb7d", "#9dc0ff", "#c6b9ff", "#84dceb", "#eef3f8",
    ]),
  },
  {
    id: "latte",
    name: "Latte",
    light: true,
    ui: {
      bg: "#eff1f5",
      fg: "#4c4f69",
      dim: "#6c6f85",
      surface: "#e6e9ef",
      letterbox: "#dce0e8",
      raised: "#ccd0da",
      "raised-hover": "#bcc0cc",
      accent: "#8839ef",
      danger: "#d20f39",
      ok: "#40a02b",
      warn: "#df8e1d",
      info: "#1e66f5",
      peach: "#fe640b",
      teal: "#179299",
      pink: "#ea76cb",
      muted: "#9ca0b0",
      subtle: "#5c5f77",
      "on-color": "#ffffff",
    },
    term: term("#4c4f69", "#eff1f5", "#dc8a78", "#acb0be80", [
      "#5c5f77", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#acb0be",
      "#6c6f85", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#bcc0cc",
    ]),
  },
  {
    id: "platinum",
    name: "Platinum",
    light: true,
    ui: {
      bg: "#ffffff",
      fg: "#1d1d1f",
      dim: "#6e6e73",
      surface: "#e8e8ea",
      letterbox: "#d5d5d8",
      raised: "#f4f4f5",
      "raised-hover": "#dcdce0",
      accent: "#2f7cf6",
      danger: "#e0393e",
      ok: "#2e9d3c",
      warn: "#b07c00",
      info: "#2f7cf6",
      peach: "#e86a17",
      teal: "#16978e",
      pink: "#d23f8f",
      muted: "#a0a0a6",
      subtle: "#3c3c43",
      "on-color": "#ffffff",
    },
    term: term("#1d1d1f", "#ffffff", "#2f7cf6", "#2f7cf633", [
      "#1d1d1f", "#c23621", "#1f9e1f", "#9a8600", "#2f52d8", "#b72fb7", "#178fa0", "#bdbdc2",
      "#6e6e73", "#e0393e", "#2e9d3c", "#b07c00", "#2f7cf6", "#d23f8f", "#16978e", "#8e8e93",
    ]),
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    light: true,
    ui: {
      bg: "#fdf6e3",
      fg: "#586e75",
      dim: "#93a1a1",
      surface: "#eee8d5",
      letterbox: "#e6dfca",
      raised: "#f6efdb",
      "raised-hover": "#e3dcc6",
      accent: "#268bd2",
      danger: "#dc322f",
      ok: "#859900",
      warn: "#b58900",
      info: "#268bd2",
      peach: "#cb4b16",
      teal: "#2aa198",
      pink: "#d33682",
      muted: "#b5b9b0",
      subtle: "#657b83",
      "on-color": "#fdf6e3",
    },
    term: term("#586e75", "#fdf6e3", "#586e75", "#eee8d5", [
      "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
      "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
    ]),
  },
  {
    id: "nord",
    name: "Nord",
    light: false,
    ui: {
      bg: "#2e3440",
      fg: "#d8dee9",
      dim: "#7b88a1",
      surface: "#272c36",
      letterbox: "#21252e",
      raised: "#3b4252",
      "raised-hover": "#434c5e",
      accent: "#88c0d0",
      danger: "#bf616a",
      ok: "#a3be8c",
      warn: "#ebcb8b",
      info: "#81a1c1",
      peach: "#d08770",
      teal: "#8fbcbb",
      pink: "#b48ead",
      muted: "#4c566a",
      subtle: "#e5e9f0",
      "on-color": "#21252e",
    },
    term: term("#d8dee9", "#2e3440", "#d8dee9", "#4c566a80", [
      "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
      "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4",
    ]),
  },
  {
    id: "gruvbox",
    name: "Gruvbox",
    light: false,
    ui: {
      bg: "#282828",
      fg: "#ebdbb2",
      dim: "#928374",
      surface: "#1d2021",
      letterbox: "#161819",
      raised: "#3c3836",
      "raised-hover": "#504945",
      accent: "#fe8019",
      danger: "#fb4934",
      ok: "#b8bb26",
      warn: "#fabd2f",
      info: "#83a598",
      peach: "#fe8019",
      teal: "#8ec07c",
      pink: "#d3869b",
      muted: "#665c54",
      subtle: "#d5c4a1",
      "on-color": "#1d2021",
    },
    term: term("#ebdbb2", "#282828", "#ebdbb2", "#50494580", [
      "#3c3836", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
      "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
    ]),
  },
  {
    id: "tokyo-night",
    name: "Tokyo Night",
    light: false,
    ui: {
      bg: "#1a1b26",
      fg: "#c0caf5",
      dim: "#565f89",
      surface: "#16161e",
      letterbox: "#101014",
      raised: "#292e42",
      "raised-hover": "#3b4261",
      accent: "#7aa2f7",
      danger: "#f7768e",
      ok: "#9ece6a",
      warn: "#e0af68",
      info: "#7dcfff",
      peach: "#ff9e64",
      teal: "#73daca",
      pink: "#bb9af7",
      muted: "#414868",
      subtle: "#a9b1d6",
      "on-color": "#101014",
    },
    term: term("#c0caf5", "#1a1b26", "#c0caf5", "#33467c", [
      "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
      "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
    ]),
  },
  {
    id: "dracula",
    name: "Dracula",
    light: false,
    ui: {
      bg: "#282a36",
      fg: "#f8f8f2",
      dim: "#6272a4",
      surface: "#21222c",
      letterbox: "#191a21",
      raised: "#44475a",
      "raised-hover": "#565970",
      accent: "#bd93f9",
      danger: "#ff5555",
      ok: "#50fa7b",
      warn: "#f1fa8c",
      info: "#8be9fd",
      peach: "#ffb86c",
      teal: "#8be9fd",
      pink: "#ff79c6",
      muted: "#6272a4",
      subtle: "#e0e0e0",
      "on-color": "#191a21",
    },
    term: term("#f8f8f2", "#282a36", "#f8f8f2", "#44475a", [
      "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
      "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
    ]),
  },
];

export const FINISHES = [
  { id: "flat", name: "Flat", hint: "Plain fills, as Arugula has always looked" },
  { id: "aqua", name: "Aqua", hint: "Glossy gel buttons, pinstripes, a lit title bar" },
  { id: "brushed", name: "Brushed metal", hint: "Machined aluminum, bevelled buttons" },
  { id: "glass", name: "Glass", hint: "Frosted bars and menus over a soft glow" },
] as const;

export const CORNERS = [
  { id: "square", name: "Square" },
  { id: "soft", name: "Soft" },
  { id: "round", name: "Round" },
] as const;

export type Finish = (typeof FINISHES)[number]["id"];
export type Corners = (typeof CORNERS)[number]["id"];

export interface Appearance {
  /** A palette's id, "system" (the dark default, or Latte when the system
   * is light) or "default": nothing picked, which is Slate (M113; Mocha
   * before it). */
  palette: string;
  finish: Finish;
  corners: Corners;
}

const KEY = "arugula-appearance";
const DEFAULT: Appearance = { palette: "default", finish: "flat", corners: "square" };
const lightQuery = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: light)") : null;

let current: Appearance = load();
const listeners = new Set<() => void>();

function load(): Appearance {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "null") as (Partial<Appearance> & { workspace?: unknown }) | null;
    if (!v) return DEFAULT;
    const a: Appearance = {
      palette: v.palette === "system" || v.palette === "default" || PALETTES.some((p) => p.id === v.palette) ? v.palette! : DEFAULT.palette,
      finish: FINISHES.some((f) => f.id === v.finish) ? v.finish! : DEFAULT.finish,
      corners: CORNERS.some((c) => c.id === v.corners) ? v.corners! : DEFAULT.corners,
    };
    // M108–M112's "New workspace" switch (`workspace: "classic" | "new"`)
    // is gone: the new workspace is the only one. Forget it, whatever it
    // said.
    if ("workspace" in v) localStorage.setItem(KEY, JSON.stringify(a));
    return a;
  } catch {
    return DEFAULT;
  }
}

export const appearance = () => current;

/** The dark palette nothing picked means: Slate, the workspace's own
 * (M113). Anyone who picked one keeps it. */
export const darkDefault = () => "slate";

/** The palette in use: "system" and "default" resolved. */
export function palette(a: Appearance = current): Palette {
  const id = a.palette === "system" ? (lightQuery?.matches ? "latte" : darkDefault()) : a.palette === "default" ? darkDefault() : a.palette;
  return PALETTES.find((p) => p.id === id) ?? PALETTES[0];
}

/** The terminals' colors, for a new terminal; `onAppearance` for changes. */
export const terminalTheme = (): ITheme => palette().term;

export function onAppearance(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

export function setAppearance(change: Partial<Appearance>) {
  current = { ...current, ...change };
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Private mode: it lasts for this page.
  }
  apply();
}

// ---- legible on every palette (M113, #1028)

const channels = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const toHex = (c: number[]) => `#${c.map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;

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

const mix = (a: string, b: string, t: number) => {
  const [x, y] = [channels(a), channels(b)];
  return toHex(x.map((v, i) => v + (y[i] - v) * t));
};

/** `color`, moved toward `toward` only as far as it takes to read at 4.5:1
 * on every one of `on`. */
function legibleOn(color: string, on: string[], toward: string): string {
  const ok = (c: string) => on.every((b) => contrast(c, b) >= 4.5);
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
const TEXT = ["fg", "dim", "subtle", "accent", "warn", "ok", "danger", "info", "peach", "teal", "pink"] as const;

/** The palette's UI colors as the page uses them (M113): a word in any of
 * them reads at WCAG AA (4.5:1) on the backgrounds it sits on (bg, surface
 * and raised, the hovered and picked rows'). Each palette keeps its
 * colors where they already do; one that doesn't is moved toward white
 * (on a dark palette) or black (on a light one) as little as it takes. A primary button's label likewise reads on the accent. */
export function legible(p: Palette): Ui {
  const u = { ...p.ui };
  const on = [u.bg, u.surface, u.raised];
  const end = p.light ? "#000000" : "#ffffff";
  // Toward black or white, so a color keeps its hue (amber stays amber,
  // only darker or lighter).
  for (const k of TEXT) u[k] = legibleOn(u[k], on, end);
  const label = [u["on-color"], "#000000", "#ffffff"].sort((a, b) => contrast(b, u.accent) - contrast(a, u.accent));
  if (contrast(u["on-color"], u.accent) < 4.5) u["on-color"] = label[0];
  return u;
}

/** Puts the appearance on the document: the palette as custom properties
 * (style.css's :root holds Mocha's, for a page that never runs this), the
 * finish and corners as attributes style.css keys on. */
export function apply() {
  const p = palette();
  const root = document.documentElement;
  for (const [k, v] of Object.entries(legible(p))) root.style.setProperty(`--${k}`, v);
  root.style.setProperty("--divider", p.ui.raised);
  root.style.colorScheme = p.light ? "light" : "dark";
  root.dataset.scheme = p.light ? "light" : "dark";
  root.dataset.finish = current.finish;
  root.dataset.corners = current.corners;
  root.dataset.palette = p.id;
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", p.ui.surface);
  listeners.forEach((fn) => fn());
}

lightQuery?.addEventListener("change", () => current.palette === "system" && apply());
