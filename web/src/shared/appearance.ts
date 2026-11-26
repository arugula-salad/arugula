// How the page looks, on this device: a theme (dark, light, or the
// system's pick), a preset (Arugula unless picked) and Look's three
// sliders (kit/look.ts).
// Kept in this browser (and, on arugula.io, in a cookie every arugula.io
// host reads: appearance-cookie.ts), applied before the first draw so a
// reload doesn't flash the default.

import type { ITheme } from "@xterm/xterm";
import { cookieDomain, cookieLine, pickStored, readCookie } from "./appearance-cookie";
import { fix, mix, paint, preset, PRESETS, sliders, type Sliders, type Ui } from "./kit/look";

export { contrast, onFill, selectedFill } from "./kit/look";

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
    // M108 (#1023): the workspace's own dark palette, from #1018's mission
    // control: near-black slate, blue-gray lines, a restrained cyan for
    // selection and focus, lavender second, amber for attention. The dark
    // default since M113. Every text color passes WCAG AA (4.5)
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
      merged: "#b49cf2",
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
      merged: "#8250df",
      muted: "#9ca0b0",
      subtle: "#5c5f77",
      "on-color": "#ffffff",
    },
    term: term("#4c4f69", "#eff1f5", "#dc8a78", "#acb0be80", [
      "#5c5f77", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#acb0be",
      "#6c6f85", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#bcc0cc",
    ]),
  },
];

/** What Theme offers: the system's pick, Latte (light) or Slate (dark,
 * the default). */
export const THEMES = [
  { id: "system", name: "System" },
  { id: "light", name: "Light" },
  { id: "dark", name: "Dark" },
] as const;

export type Theme = (typeof THEMES)[number]["id"];

export interface Appearance extends Omit<Sliders, "light"> {
  /** Dark unless picked; "system" follows the system's. */
  theme: Theme;
  /** A preset's id (kit/look.ts's PRESETS): Arugula unless picked. */
  preset: string;
  /** The light slider, or null for the theme's own (each theme starts
   * somewhere else). */
  light: number | null;
}

const KEY = "arugula-appearance";
const DEFAULT: Appearance = { theme: "dark", preset: PRESETS[0].id, ...sliders(true), light: null };
const lightQuery = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: light)") : null;

const LIGHT_PALETTES = ["latte", "platinum", "solarized-light"];
const slider = <D,>(v: unknown, d: D) => (typeof v === "number" && v >= 0 && v <= 1 ? v : d);

/** A stored appearance, or one from before the brand: its theme stays (a
 * light palette from before Look is Light, "system" is System, any other
 * Dark), and sliders set for the interim look start again at Arugula's. */
export function parseAppearance(raw: string): Appearance | null {
  try {
    const v = JSON.parse(raw) as (Partial<Appearance> & { palette?: string }) | null;
    if (!v || typeof v !== "object") return null;
    const old = v.palette === "system" ? "system" : LIGHT_PALETTES.includes(v.palette ?? "") ? "light" : "dark";
    const theme = THEMES.some((t) => t.id === v.theme) ? v.theme! : old;
    if (typeof v.preset !== "string") return { ...DEFAULT, theme };
    const p = preset(v.preset);
    return {
      theme,
      preset: p.id,
      light: v.light === null ? null : slider(v.light, null),
      warmth: slider(v.warmth, p.warmth),
      depth: slider(v.depth, p.depth),
    };
  } catch {
    return null;
  }
}

let current: Appearance = load();
const listeners = new Set<() => void>();

/** The cookie's copy (on arugula.io), then this origin's localStorage. */
function load(): Appearance {
  let cookie: string | null = null;
  let stored: string | null = null;
  try {
    if (cookieDomain(location.hostname)) cookie = readCookie(document.cookie, KEY);
  } catch {
    // No document (a test) or cookies blocked: localStorage alone.
  }
  try {
    stored = localStorage.getItem(KEY);
  } catch {
    // Private mode or blocked storage.
  }
  const a = pickStored(cookie, stored, parseAppearance);
  if (!a) return DEFAULT;
  // Keep the two in step: a choice made on another arugula.io host lands
  // in this origin's localStorage, one kept only in localStorage (from
  // before the cookie) goes into the cookie, and what came before Look
  // (a palette, a finish, corners, density) drops out of both.
  const json = JSON.stringify(a);
  if (stored !== json) {
    try {
      localStorage.setItem(KEY, json);
    } catch {
      // Private mode.
    }
  }
  store(json);
  return a;
}

/** Sets the cookie on arugula.io (renewing it); nothing elsewhere. */
function store(json: string) {
  try {
    const domain = cookieDomain(location.hostname);
    if (domain) document.cookie = cookieLine(KEY, json, domain);
  } catch {
    // Cookies blocked: localStorage still has it.
  }
}

export const appearance = () => current;

/** Whether the page is light now: Light picked, or the system's light
 * with System picked. */
export const isLight = (a: Appearance = current) => a.theme === "light" || (a.theme === "system" && !!lightQuery?.matches);

/** The theme's palette in use: Latte when light, else Slate. */
export function palette(a: Appearance = current): Palette {
  return PALETTES.find((p) => p.id === (isLight(a) ? "latte" : "slate"))!;
}

/** The terminals' colors, for a new terminal; `onAppearance` for changes. */
export const terminalTheme = (): ITheme => palette().term;

export function onAppearance(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** The sliders as they paint: the light slider's own value, or the
 * theme's. */
export function lookSliders(a: Appearance = current): Sliders {
  return { light: a.light ?? sliders(!isLight(a)).light, warmth: a.warmth, depth: a.depth };
}

/** Picks a preset: its hues, and the warmth and depth it starts at. */
export function setPreset(id: string) {
  const p = preset(id);
  setAppearance({ preset: p.id, warmth: p.warmth, depth: p.depth });
}

/** Back to nothing picked: Dark, Arugula, its sliders. */
export const resetAppearance = () => setAppearance(DEFAULT);

export const isDefaultAppearance = (a: Appearance = current) => (Object.keys(DEFAULT) as (keyof Appearance)[]).every((k) => a[k] === DEFAULT[k]);

export function setAppearance(change: Partial<Appearance>) {
  // A new theme starts at its own lightness.
  if (change.theme && change.theme !== current.theme && !("light" in change)) change = { ...change, light: null };
  current = { ...current, ...change };
  const json = JSON.stringify(current);
  try {
    localStorage.setItem(KEY, json);
  } catch {
    // Private mode: it lasts for this page (and, on arugula.io, the cookie).
  }
  store(json);
  apply();
}

// ---- legible on every palette (M113, #1028)

/** The palette's UI colors as the page uses them: kit/look.ts's `fix()`, a
 * word in any of them reading at WCAG AA on what it sits on. */
export const legible = (p: Palette): Ui => fix(p.ui, !p.light);

// ---- the house pair (#729): what Docs wear

export type Scheme = "light" | "dark";

/** The palette a Doc wears in each scheme, whatever palette the viewer
 * picked: a shared Doc then reads the same for everyone, and the named hues
 * are checked against two papers, not nine. */
const HOUSE: Record<Scheme, string> = { dark: "slate", light: "latte" };

/** A Doc's reading text, stronger than the palette's `fg`: Slate's own bright
 * white (its terminal set's) and a deeper Latte text. */
const HOUSE_INK: Record<Scheme, string> = { dark: "#eef3f8", light: "#2b2d3d" };

/** The six named hues and the palette color each is (leaf is the palette's
 * ok, and so on), in a chart's order. */
const HUE_OF = { leaf: "ok", pepper: "peach", sky: "info", plum: "merged", sun: "warn", stone: "dim" } as const;

const palettes = (id: string) => PALETTES.find((p) => p.id === id)!;

/** The house pair's colors as `legible()` paints them, for a scheme: a Doc's
 * tokens without the `--doc-` (`paper`, `ink-soft`, `leaf-soft`…), which
 * apply() and the share viewer set, and shared/docs/colours.json holds. */
export function houseColours(scheme: Scheme): Record<string, string> {
  const u = legible(palettes(HOUSE[scheme]));
  const out: Record<string, string> = {
    paper: u.bg,
    sunken: u.surface,
    tint: u.raised,
    ink: HOUSE_INK[scheme],
    "ink-soft": u.subtle,
    "ink-faint": u.dim,
    // style.css's --line-strong, which `:root` works out from the picked
    // palette, here from the house one.
    line: mix(u.surface, u.muted, 0.55),
    "line-quiet": mix(u.bg, mix(u.surface, u.muted, 0.55), 0.5),
    rule: mix(u.surface, u.muted, 0.55),
    // A Doc's links are info; accent means live (a fresh fill), as
    // everywhere (docs/design/today.md).
    link: u.info,
    live: u.accent,
    // What a drawing sits on in either scheme: the light house's paper.
    drawing: legible(palettes(HOUSE.light)).bg,
  };
  for (const [hue, key] of Object.entries(HUE_OF)) {
    out[hue] = u[key];
    out[`${hue}-soft`] = mix(u.bg, u[key], 0.2);
  }
  return out;
}

/** The scheme's house pair as `--doc-*` custom properties on the root: a
 * Doc's tokens. apply() sets them with the palette's. */
function applyHouse(scheme: Scheme, root: HTMLElement) {
  for (const [k, v] of Object.entries(houseColours(scheme))) root.style.setProperty(`--doc-${k}`, v);
}

/** Puts the appearance on the document: kit/tokens.css's role tokens, as
 * kit/look.ts paints them from the theme, the preset and the sliders
 * (tokens.css holds Arugula dark's, for a page that never runs this). */
export function apply() {
  const p = palette();
  const root = document.documentElement;
  const tokens = paint(p.ui, !p.light, lookSliders(), preset(current.preset));
  for (const [k, v] of Object.entries(tokens)) root.style.setProperty(`--${k}`, v);
  applyHouse(p.light ? "light" : "dark", root);
  root.style.colorScheme = p.light ? "light" : "dark";
  root.dataset.scheme = p.light ? "light" : "dark";
  root.dataset.palette = p.id;
  root.dataset.preset = preset(current.preset).id;
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", tokens.canvas);
  listeners.forEach((fn) => fn());
}

lightQuery?.addEventListener("change", () => current.theme === "system" && apply());
