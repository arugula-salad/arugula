// A Doc's named colours (#256): leaf, pepper, sky, plum, sun, stone, ink and
// paper, each with a `-soft` fill, which an agent writes in a drawing
// (`fill="leaf-soft"`) or a chart's `colors` and which resolve to the
// current scheme's value. The values are colours.json, which the daemon
// reads too (`crates/arugulad-docs/src/docs/embeds.rs`); style.css's `--doc-*`
// tokens hold the same ones (a spec checks).

import table from "./colours.json" with { type: "json" };

export type Scheme = "light" | "dark";

/** The six hues, in a chart's order: what `colors` may name. */
export const HUES: readonly string[] = table.hues;
/** Every name a drawing may use: each colour and its `-soft` form. */
export const NAMES: readonly string[] = table.names.flatMap((n) => [n, `${n}-soft`]);
const CSS = new Set(table.css);

/** A name's value in a scheme, or undefined for a word that isn't one. */
export const hex = (name: string, scheme: Scheme): string | undefined => (table[scheme] as Record<string, string>)[name];

/** The custom property style.css gives a name, inside `.doc`. */
export const cssVar = (name: string) => (name === "paper-soft" ? "--doc-tint" : `--doc-${name}`);

/** A chart's colour for a hue: the Doc's token, the light value outside one. */
export const chartColour = (name: string) => `var(${cssVar(name)}, ${hex(name, "light")})`;

/** The attributes of a drawing that take a colour. */
const PAINT = /(\s(?:fill|stroke|stop-color|color)\s*=\s*)(["'])([^"']*)\2/g;

const isHex = (v: string) => /^#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/.test(v);

/** Whether a paint attribute's value is a colour a drawing can use: a name,
 * a hex value, a function (`url(#g)`, `rgb(…)`) or a CSS keyword. */
export function isColour(value: string): boolean {
  const v = value.trim().toLowerCase();
  return !v || NAMES.includes(v) || isHex(v) || /^[a-z-]+\(.*\)$/.test(v) || CSS.has(v);
}

/** The first paint attribute whose value isn't a colour, as one line
 * (`svg: fill="leafy" is not a colour: …`, as `check_svg` says it), or null. */
export function checkPaint(svg: string): string | null {
  for (const m of svg.matchAll(PAINT)) {
    if (!isColour(m[3])) {
      const attr = m[1].trim().replace(/\s*=\s*$/, "");
      return `svg: ${attr}="${m[3].trim()}" is not a colour: use one of ${NAMES.join(", ")}, or a hex value like #3f6b2a`;
    }
  }
  return null;
}

/** Whether a drawing names any of the colours. */
export const usesNames = (svg: string) => [...svg.matchAll(PAINT)].some((m) => NAMES.includes(m[3].trim().toLowerCase()));

/** The drawing with each name replaced by its value in the scheme. What it
 * doesn't paint (text, most of all) is ink, as on the page, not SVG's black:
 * the root gets `fill="ink"` unless it has a fill of its own. */
export function resolve(svg: string, scheme: Scheme): string {
  const out = svg.replace(PAINT, (all, pre: string, q: string, v: string) => {
    const value = hex(v.trim().toLowerCase(), scheme);
    return value ? `${pre}${q}${value}${q}` : all;
  });
  const root = /<svg(?=[\s>/])([^>]*)/.exec(out);
  if (!root || /\sfill\s*=/.test(root[1])) return out;
  const at = root.index + 4;
  return `${out.slice(0, at)} fill="${hex("ink", scheme)}"${out.slice(at)}`;
}
