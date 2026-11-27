// How the page looks, on this device: a theme (dark, light, or the
// system's pick), a preset (Arugula unless picked) and Look's three
// sliders (kit/look.ts).
// Kept in this browser (and, on arugula.io, in a cookie every arugula.io
// host reads: appearance-cookie.ts), applied before the first draw so a
// reload doesn't flash the default.

import type { ITheme } from "@xterm/xterm";
import { cookieDomain, cookieLine, pickStored, readCookie } from "./appearance-cookie";
import { ansi, house, paint, preset, PRESETS, sliders, uiOf, type Sliders, type Ui } from "./kit/look";

export { contrast, onFill, selectedFill } from "./kit/look";

/** What the terminals and editors are drawn in now: the look's roles
 * (kit/look.ts's paint) under their old names, and a terminal's colors. */
export interface Palette {
  /** The preset's id. */
  id: string;
  light: boolean;
  ui: Ui;
  term: ITheme;
}

const ANSI = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] as const;

/** A terminal's colors, on the well it sits in. */
function term(t: Record<string, string>, ui: Ui, dark: boolean): ITheme {
  const c = ansi(t, dark);
  const theme: ITheme = { foreground: ui.fg, background: ui.bg, cursor: ui.accent, cursorAccent: ui.bg, selectionBackground: `${ui.accent}40` };
  ANSI.forEach((k, i) => {
    theme[k] = c[i];
    theme[`bright${k[0].toUpperCase()}${k.slice(1)}` as "brightBlack"] = c[i + 8];
  });
  return theme;
}

/** What Theme offers: the system's pick, light or dark (the default). */
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

/** The look's roles as paint makes them, and its palette, kept until the
 * appearance changes. */
let painted: { key: string; tokens: Record<string, string>; palette: Palette } | null = null;
function look(a: Appearance = current) {
  const key = `${isLight(a)}${JSON.stringify(a)}`;
  if (painted?.key !== key) {
    const dark = !isLight(a);
    const p = preset(a.preset);
    const tokens = paint(dark, lookSliders(a), p);
    const ui = uiOf(tokens, dark);
    painted = { key, tokens, palette: { id: p.id, light: !dark, ui, term: term(tokens, ui, dark) } };
  }
  return painted;
}

/** What the terminals and editors are drawn in now. */
export const palette = (a: Appearance = current): Palette => look(a).palette;

/** The terminals' face: the brand's mono (kit/tokens.css's --font-mono),
 * SF Mono where there is one, else the system's. */
export const TERMINAL_FONT = '"SF Mono", ui-monospace, Menlo, monospace';

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

// ---- the house pair (#729): what Docs wear

export type Scheme = "light" | "dark";

/** A Doc's colors in a scheme, whatever the reader picked (kit/look.ts's
 * house): a Doc's tokens without the `--doc-`, which apply() and the share
 * page set, and shared/docs/colours.json holds. */
export const houseColours = (scheme: Scheme): Record<string, string> => house(scheme === "dark");

/** The scheme's house pair as `--doc-*` custom properties on the root: a
 * Doc's tokens. apply() sets them with the palette's. */
function applyHouse(scheme: Scheme, root: HTMLElement) {
  for (const [k, v] of Object.entries(houseColours(scheme))) root.style.setProperty(`--doc-${k}`, v);
}

/** Puts the appearance on the document: kit/tokens.css's role tokens, as
 * kit/look.ts paints them from the theme, the preset and the sliders
 * (tokens.css holds Arugula dark's, for a page that never runs this). */
export function apply() {
  const { tokens, palette: p } = look();
  const root = document.documentElement;
  for (const [k, v] of Object.entries(tokens)) root.style.setProperty(`--${k}`, v);
  applyHouse(p.light ? "light" : "dark", root);
  root.style.colorScheme = p.light ? "light" : "dark";
  root.dataset.scheme = p.light ? "light" : "dark";
  root.dataset.preset = preset(current.preset).id;
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", tokens.canvas);
  listeners.forEach((fn) => fn());
}

lightQuery?.addEventListener("change", () => current.theme === "system" && apply());
