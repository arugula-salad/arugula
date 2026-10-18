// Fences in a Doc section's Markdown, and the two that are more than code:
// ```chart (a JSON spec, `chart.tsx`) and ```svg (one drawing, shown only
// through <img>). Plain TypeScript, so a spec can import it. The daemon
// checks both before it writes a section (`crates/daemon/src/docs/embeds.rs`,
// which has the grammar); `checkSvg` and `plain` here say the same things,
// and a spec runs both over the same files (`tests/fixtures/docs/`).

import { checkChart, summary } from "./chart";

/** A fence from line `i`: its language (the first word after the mark), the
 * rest of that line, what is inside (each line without up to `strip` columns
 * of indent), and the line after it. One unclosed runs to the end. */
export function readFence(lines: string[], i: number, strip = 0): { lang: string; alt: string; code: string[]; next: number } {
  const open = lines[i].trimStart();
  const mark = open.slice(0, 3);
  const info = open.slice(3).trim();
  const [lang, ...rest] = info.split(/\s+/);
  const code: string[] = [];
  for (i++; i < lines.length && !lines[i].trimStart().startsWith(mark); i++) {
    code.push(lines[i].slice(Math.min(strip, lines[i].length - lines[i].trimStart().length)));
  }
  return { lang, alt: rest.join(" "), code, next: i + 1 };
}

const OPEN = /^ *(```|~~~)/;

/** A body's lines as the Markdown renderer reads them. */
export const bodyLines = (text: string) => text.replace(/\r/g, "").replace(/\t/g, "    ").split("\n");

/** Every fence in a body, with the lines it takes. */
export function fences(text: string): { lang: string; alt: string; code: string; start: number; end: number }[] {
  const lines = bodyLines(text);
  const out = [];
  for (let i = 0; i < lines.length; ) {
    if (!OPEN.test(lines[i])) {
      i++;
      continue;
    }
    const f = readFence(lines, i);
    out.push({ lang: f.lang, alt: f.alt, code: f.code.join("\n"), start: i, end: Math.min(f.next, lines.length) });
    i = f.next;
  }
  return out;
}

/** The first problem with a body's charts and drawings, or null. */
export function checkEmbeds(text: string): string | null {
  for (const f of fences(text)) {
    const why = f.lang === "chart" ? checkChart(f.code) : f.lang === "svg" ? checkSvg(f.code) : null;
    if (why) return why;
  }
  return null;
}

/** The body as plain Markdown: a chart is a line saying what it is, a
 * drawing is `[drawing]`. */
export function plain(text: string): string {
  const lines = bodyLines(text);
  const out: string[] = [];
  let at = 0;
  for (const f of fences(text)) {
    const stand = f.lang === "chart" ? summary(f.code) : f.lang === "svg" ? "[drawing]" : null;
    if (stand === null) continue;
    out.push(...lines.slice(at, f.start), stand);
    at = f.end;
  }
  out.push(...lines.slice(at));
  return out.join("\n");
}

const SVG_NS = "http://www.w3.org/2000/svg";
/** A drawing's cap, in bytes: a section's. */
export const SVG_MAX = 64 * 1024;

/** Null if the drawing can be shown through <img>, else why not (as the
 * daemon's `check_svg` says it). */
export function checkSvg(svg: string): string | null {
  const bad = (why: string) => `svg: ${why}`;
  const bytes = new TextEncoder().encode(svg).length;
  if (bytes > SVG_MAX) return bad(`${bytes} bytes; the cap is ${SVG_MAX} (64 KB)`);
  let rest = svg.trim();
  if (!rest) return bad("the fence is empty: put one <svg> drawing in it");
  // An XML declaration and comments may come before the root.
  for (;;) {
    const end = rest.startsWith("<?") ? rest.indexOf("?>") + 2 : rest.startsWith("<!--") ? rest.indexOf("-->") + 3 : 0;
    if (end < 3) break;
    rest = rest.slice(end).trimStart();
  }
  const root = /^<svg(?=[\s>/])/.exec(rest);
  if (!root) return bad("a drawing must be one <svg> element: its text has to start with <svg");
  const gt = rest.indexOf(">");
  const tag = gt < 0 ? rest : rest.slice(0, gt);
  if (!tag.includes(`xmlns="${SVG_NS}"`) && !tag.includes(`xmlns='${SVG_NS}'`)) {
    return bad(`the <svg> needs xmlns="${SVG_NS}", or the picture shows nothing`);
  }
  return null;
}

/** The URL an <img> shows a drawing from. A data: URL and not a blob: URL:
 * control serves this client with `img-src 'self' data: https:`, which has no
 * blob:, and a data: URL needs no revoking. */
export const svgUrl = (svg: string) => "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
