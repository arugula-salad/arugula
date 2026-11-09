// Fences in a Doc section's Markdown, and the four that are more than code:
// ```chart (a JSON spec, `chart.tsx`), ```stats (tiles, `stats.tsx`),
// ```svg (one drawing, shown only through <img>) and ```prototype (a page in
// a sandboxed frame, `prototype-frame.tsx`, #506); and its pictures, `![alt](assets/<name>)` in a line of
// text (#965). Plain TypeScript, so a spec can import it. The daemon checks
// all of them before it writes a section (`crates/arugulad-docs/src/docs/embeds.rs`,
// which has the grammar); `checkSvg`, `checkPrototype`, `checkImages` and `plain` here say the
// same things, and a spec runs both over the same files
// (`tests/fixtures/docs/`).

import { checkPaint } from "../docs/colours";
import { checkChart, summary } from "./chart";
import { checkStats, statsSummary } from "./stats";

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

export interface ImageRef {
  start: number;
  end: number;
  alt: string;
  src: string;
}

/** The `![alt](src)` in a line, left to right, skipping inline code: as the
 * daemon finds them (`image_refs`), and as `inline()` in `markup.tsx` draws
 * them, except that it takes only a target `assetName` accepts. */
export function imageRefs(line: string): ImageRef[] {
  const out: ImageRef[] = [];
  for (let i = 0; i < line.length; ) {
    if (line[i] === "`") {
      const n = line.indexOf("`", i + 1) - (i + 1);
      i += n > 0 ? n + 2 : 1;
    } else if (line[i] === "!" && line[i + 1] === "[") {
      const close = line.indexOf("]", i + 2);
      const open = close < 0 || line[close + 1] !== "(" ? -1 : close + 1;
      const end = open < 0 ? -1 : line.indexOf(")", open + 1);
      const src = end < 0 ? "" : line.slice(open + 1, end);
      if (end < 0 || /\s/.test(src)) i++;
      else {
        out.push({ start: i, end: end + 1, alt: line.slice(i + 2, close), src });
        i = end + 1;
      }
    } else i++;
  }
  return out;
}

const NAME = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,99}$/;
const PICTURES = [".png", ".jpg", ".jpeg", ".gif", ".webp"];
const hasPictureExt = (n: string) => PICTURES.some((e) => n.endsWith(e));

/** The asset's name if `src` is `assets/` and a plain file name with a
 * picture extension (lowercase): the only target a Doc draws. */
export function assetName(src: string): string | null {
  const name = src.startsWith("assets/") ? src.slice(7) : "";
  return NAME.test(name) && hasPictureExt(name) ? name : null;
}

const oneLine = (s: string) => s.split(/\s+/).filter(Boolean).join(" ");

/** The first problem with the pictures in a body's text (not its fences)
 * given the Doc's asset names, or null. */
export function checkImages(text: string, assets: string[]): string | null {
  const lines = bodyLines(text);
  const inFence = new Set<number>();
  for (const f of fences(text)) for (let n = f.start; n < f.end; n++) inFence.add(n);
  for (const [n, line] of lines.entries()) {
    if (inFence.has(n)) continue;
    for (const r of imageRefs(line)) {
      const bad = (why: string) => `image ${oneLine(r.src)}: ${why}`;
      const name = r.src.startsWith("assets/") && NAME.test(r.src.slice(7)) ? r.src.slice(7) : null;
      if (name === null) return bad("must be assets/<file>, a plain file name in this Doc's assets (send one with doc asset first)");
      if (!hasPictureExt(name)) return bad(`must be one of ${PICTURES.join(" ")}`);
      if (!assets.includes(name)) return bad("no such asset in this Doc: send it with doc asset first");
    }
  }
  return null;
}

/** The first problem with a body's charts, tiles, drawings and pictures, or null.
 * `assets`: the names the Doc has. */
export function checkEmbeds(text: string, assets: string[] = []): string | null {
  for (const f of fences(text)) {
    const why =
      f.lang === "chart" ? checkChart(f.code) : f.lang === "svg" ? checkSvg(f.code) : f.lang === "stats" ? checkStats(f.code) : f.lang === "prototype" ? checkPrototype(f.code) : null;
    if (why) return why;
  }
  return checkImages(text, assets);
}

/** A line of text with each picture the page draws as `[image: alt]`. */
function standIn(line: string): string {
  let out = "";
  let at = 0;
  for (const r of imageRefs(line).filter((r) => assetName(r.src) !== null)) {
    const alt = oneLine(r.alt);
    out += line.slice(at, r.start) + (alt ? `[image: ${alt}]` : "[image]");
    at = r.end;
  }
  return out + line.slice(at);
}

/** The body as plain Markdown: a chart and tiles are a line saying what they
 * are, a drawing is `[drawing]`, a prototype `[prototype: title]` and a
 * picture `[image: alt]`. */
export function plain(text: string): string {
  const lines = bodyLines(text);
  const out: string[] = [];
  let at = 0;
  for (const f of fences(text)) {
    out.push(...lines.slice(at, f.start).map(standIn));
    if (f.lang === "chart") out.push(summary(f.code));
    else if (f.lang === "svg") out.push("[drawing]");
    else if (f.lang === "stats") out.push(statsSummary(f.code));
    else if (f.lang === "prototype") out.push(prototypeSummary(f.alt));
    else out.push(...lines.slice(f.start, f.end)); // code, pictures in it included
    at = f.end;
  }
  out.push(...lines.slice(at).map(standIn));
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
  return checkPaint(svg);
}

/** The URL an <img> shows a drawing from. A data: URL and not a blob: URL:
 * control serves this client with `img-src 'self' data: https:`, which has no
 * blob:, and a data: URL needs no revoking. */
export const svgUrl = (svg: string) => "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);

/** A prototype's default height, in pixels. */
export const PROTOTYPE_HEIGHT = 480;

/** A prototype's fence line after `prototype`: the title and the height. `height=N`
 * is any one word of it (the first), clamped to 120..2000, 480 without; the
 * rest, joined by single spaces, is the title (`prototype_info` in the daemon). */
export function prototypeInfo(alt: string): { title: string; height: number } {
  const title: string[] = [];
  let height: number | null = null;
  for (const w of alt.split(/\s+/).filter(Boolean)) {
    const n = /^height=(\d+)$/.exec(w);
    if (n && height === null) height = Math.min(2000, Math.max(120, Number(n[1])));
    else title.push(w);
  }
  return { title: title.join(" "), height: height ?? PROTOTYPE_HEIGHT };
}

/** `[prototype: Views motion]`; `[prototype]` without a title. */
function prototypeSummary(alt: string): string {
  const { title } = prototypeInfo(alt);
  return title ? `[prototype: ${title}]` : "[prototype]";
}

const isSpace = (c: string | undefined) => c !== undefined && /\s/.test(c);

/** The attribute names of each `<name …>` tag in `lower` (ASCII-lowercased
 * HTML): a tag is `<name` and then a space, `/` or `>`; its end is the first
 * `>` outside quotes (`tag_attrs` in the daemon's embeds.rs). */
function tagAttrs(lower: string, name: string): string[][] {
  const open = `<${name}`;
  const out: string[][] = [];
  for (let at = lower.indexOf(open); at >= 0; at = lower.indexOf(open, at + 1)) {
    let i = at + open.length;
    const c = lower[i];
    if (!(isSpace(c) || c === "/" || c === ">")) continue;
    const names: string[] = [];
    while (i < lower.length && lower[i] !== ">") {
      if (isSpace(lower[i]) || lower[i] === "/") {
        i++;
        continue;
      }
      const start = i;
      while (i < lower.length && !isSpace(lower[i]) && !"=>/".includes(lower[i])) i++;
      names.push(lower.slice(start, i));
      while (isSpace(lower[i])) i++;
      if (lower[i] === "=") {
        i++;
        while (isSpace(lower[i])) i++;
        const q = lower[i];
        if (q === '"' || q === "'") {
          i++;
          while (i < lower.length && lower[i] !== q) i++;
          i++;
        } else while (i < lower.length && !isSpace(lower[i]) && lower[i] !== ">") i++;
      }
    }
    out.push(names);
  }
  return out;
}

/** ASCII-only lower case, so the offsets stay those of the text. */
const asciiLower = (s: string) => s.replace(/[A-Z]/g, (c) => c.toLowerCase());

/** Null if the page can run in the sandboxed frame, else why not (as the
 * daemon's `check_prototype` says it). */
export function checkPrototype(html: string): string | null {
  const bad = (why: string) => `prototype: ${why}`;
  const bytes = new TextEncoder().encode(html).length;
  if (bytes > SVG_MAX) return bad(`${bytes} bytes; the cap is ${SVG_MAX} (64 KB)`);
  if (!html.trim()) return bad("the fence is empty: put one page in it");
  const lower = asciiLower(html);
  for (const open of ["<html", "<!doctype"]) {
    let n = 0;
    for (let at = lower.indexOf(open); at >= 0; at = lower.indexOf(open, at + 1)) {
      const c = lower[at + open.length];
      if (c === undefined || isSpace(c) || c === "/" || c === ">") n++;
    }
    if (n > 1) return bad(`one document only: found a second ${open}; put a single page in the fence`);
  }
  if (tagAttrs(lower, "script").some((a) => a.includes("src"))) {
    return bad("<script src> can't load anything: inline it, with the code between <script> and </script>");
  }
  if (tagAttrs(lower, "link").some((a) => a.includes("href"))) {
    return bad("<link href> can't load anything: inline it, with the styles between <style> and </style>");
  }
  for (const tag of ["iframe", "object", "embed", "base"]) {
    if (tagAttrs(lower, tag).length) return bad(`<${tag}> isn't allowed: the page is one document, so inline what it would show`);
  }
  if (tagAttrs(lower, "meta").some((a) => a.includes("http-equiv"))) {
    return bad("<meta http-equiv> isn't allowed: the frame sets its own policy, so remove it");
  }
  return null;
}
