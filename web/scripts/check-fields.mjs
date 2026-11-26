// The field rule (#1102): a field is drawn once, by the kit
// (shared/fields.css, Doc "Form controls: one kit"). A rule outside the kit
// whose subject is an `input`, `select` or `textarea` may place it (its
// width, its margin, its font) but not draw it: no `border*`, `background*`,
// `height`, `min-height` or `box-shadow` (a textarea's height is how much
// it shows, so that may stay). A text box that sits in a
// wrapper drawn as the field (the chat composer, the room switcher) is
// allowed below, by selector; don't add one for a look of your own: use
// `.field`, `.small` or `SearchField` (shared/ui/search-field.tsx).
//
// A class counts as a field when the app puts it only on an `input`,
// `select` or `textarea` (`.picker-filter` in TSX, or a `className` on one
// made by `document.createElement`), so a rule by class is read too;
// the kit's own `.field` and `.small` don't.
//
// What isn't a field is exempt: a range, a colour well, a file picker, a
// switch (`.sw-switch`), and a pseudo-element (`::placeholder`, a slider's
// thumb).
//
//   node scripts/check-fields.mjs [src]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

/** Today's field kit, kept until the last screen's lift moves off it: with
 * shared/kit/ (design lift §4.1: Field), the only files that draw a field. */
export const KIT = new Set([
  "shared/fields.css",
  "shared/switch.css",
  "shared/ui/search-field.css",
  "shared/ui/schedule.css",
]);

/** Embedded text boxes: the wrapper is the field, so the box inside takes
 * the kit's line, fill and height away. Keyed by file, then selector as
 * written (whitespace collapsed). Don't add a look of your own here. */
export const EMBEDDED = new Map([
  [
    "shared/style.css",
    new Set([
      ".control-rest-field input", // a join code's secret, after its fixed half
      ".thread-compose textarea", // the chat composer: .compose-box is the field
      ".chat-switcher input", // the room switcher's search, the palette's top row
    ]),
  ],
  ["shell/interaction-chrome.css", new Set([".palette-search .picker-filter"])], // the palette's search, its top row
]);

const PROPS = /^(?:border(?!-radius)(?:-[\w-]+)?|background(?:-[\w-]+)?|height|min-height|box-shadow)$/;
const FIELD = /^(?:input|select|textarea)(?![\w-])/;
const HEIGHT = /^(?:height|min-height)$/;
const KIT_CLASSES = new Set(["field", "small"]);
const NOT_A_FIELD = /\[type=["']?(?:range|color|file|hidden)["']?\]|\.sw-switch(?![\w-])|::/;

function strip(css) {
  return css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
}

/** `s` split on `sep` outside brackets and parentheses. */
function splitTop(s, sep) {
  const out = [];
  let depth = 0;
  let cur = "";
  for (const ch of s) {
    if (ch === "(" || ch === "[") depth++;
    else if (ch === ")" || ch === "]") depth--;
    if (depth === 0 && sep.test(ch)) {
      out.push(cur);
      cur = "";
    } else cur += ch;
  }
  out.push(cur);
  return out;
}

/** The compound a complex selector styles: the one after its last combinator. */
function subject(sel) {
  const parts = splitTop(sel.trim(), /[\s>+~]/).filter(Boolean);
  return parts[parts.length - 1] ?? "";
}

/** The class names in a `class=`/`className=` value: its string literals' words. */
function classWords(expr) {
  const out = [];
  for (const s of expr.matchAll(/["'`]([^"'`]*)["'`]/g)) {
    for (const w of s[1].replace(/\$\{[^}]*\}/g, " ").split(/\s+/)) if (/^-?[A-Za-z_][\w-]*$/.test(w)) out.push(w);
  }
  return out;
}

/** The value of the first `class=`/`className=` in `tag`, quotes or braces and all. */
function classAttr(tag) {
  const a = tag.match(/\bclass(?:Name)?=/);
  if (!a) return "";
  const rest = tag.slice(a.index + a[0].length);
  if (rest[0] === '"' || rest[0] === "'") return rest.slice(0, rest.indexOf(rest[0], 1) + 1);
  let depth = 0;
  let i = 0;
  for (; i < rest.length; i++) {
    if (rest[i] === "{") depth++;
    else if (rest[i] === "}" && --depth === 0) break;
  }
  return rest.slice(0, i + 1);
}

/** A JSX tag from `at` (its `<`) to its closing `>`, past braces, strings and comments. */
function tagAt(src, at) {
  let depth = 0;
  let quote = null;
  let i = at + 1;
  for (; i < src.length; i++) {
    const c = src[i];
    if (quote) {
      if (c === quote && src[i - 1] !== "\\") quote = null;
    } else if (depth > 0 && c === "/" && src[i + 1] === "/") i = src.indexOf("\n", i);
    else if (depth > 0 && c === "/" && src[i + 1] === "*") i = src.indexOf("*/", i) + 1;
    else if (c === '"' || c === "'" || c === "`") quote = c;
    else if (c === "{") depth++;
    else if (c === "}") depth--;
    else if (c === ">" && depth === 0) break;
    if (i < 0) return src.slice(at);
  }
  return src.slice(at, i);
}

/** The classes `sources` (TSX and TS) put only on an input, select or textarea
 * (each class word in a field tag's class, never in another tag's), each
 * to "textarea" when it's only ever on one, else "field". */
export function fieldClasses(sources) {
  const onField = new Map();
  const elsewhere = new Set();
  for (const src of sources) {
    for (const m of src.matchAll(/<([a-z][\w-]*)(?=[\s/>])/g)) {
      for (const w of classWords(classAttr(tagAt(src, m.index)))) {
        if (!FIELD.test(m[1])) elsewhere.add(w);
        else onField.set(w, onField.get(w) === "field" || m[1] !== "textarea" ? "field" : "textarea");
      }
    }
    for (const m of src.matchAll(/(\w+) = document\.createElement\("(input|select|textarea)"\)/g)) {
      const set = src.slice(m.index).match(new RegExp(`\\b${m[1]}\\.className = (["'\`][^"'\`]*["'\`])`));
      for (const w of set ? classWords(set[1]) : []) onField.set(w, onField.get(w) === "field" || m[2] !== "textarea" ? "field" : "textarea");
    }
  }
  return new Map([...onField].filter(([w]) => !elsewhere.has(w) && !KIT_CLASSES.has(w)));
}

/** What `sel` styles: "textarea" or "field" when its subject is one (an
 * input, select or textarea, bare or in `:is(...)`/`:where(...)`, or a
 * class in `classes`), else null. */
export function drawsOnField(sel, classes = new Map()) {
  const s = subject(sel);
  if (NOT_A_FIELD.test(s.replace(/:not\((?:[^()]|\([^()]*\))*\)/g, ""))) return null;
  if (FIELD.test(s)) return s.startsWith("textarea") ? "textarea" : "field";
  for (const m of s.matchAll(/\.(-?[A-Za-z_][\w-]*)/g)) if (classes.has(m[1])) return classes.get(m[1]);
  const inner = s.match(/^:(?:is|where)\(([\s\S]*)\)/);
  if (!inner) return null;
  const kinds = splitTop(inner[1], /,/).map((x) => x.trim()).filter((x) => FIELD.test(x));
  if (!kinds.length) return null;
  return kinds.every((x) => x.startsWith("textarea")) ? "textarea" : "field";
}

/** The offending declarations in `css` (read from `file`, relative to src), with their lines. */
export function offenders(css, file, classes = new Map()) {
  file = file.replaceAll("\\", "/");
  if (file.startsWith("shared/kit/") || KIT.has(file)) return [];
  const allowed = EMBEDDED.get(file) ?? new Set();
  const out = [];
  const text = strip(css);
  for (const rule of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = rule[1].replace(/^[\s\S]*;\s*/, "");
    if (selector.trim().startsWith("@")) continue;
    const sels = splitTop(selector, /,/).map((s) => s.trim().replace(/\s+/g, " "));
    const hit = sels.filter((s) => drawsOnField(s, classes) && !allowed.has(s));
    if (!hit.length) continue;
    const textareas = hit.every((s) => drawsOnField(s, classes) === "textarea");
    const start = text.slice(0, rule.index + rule[1].length + 1).split("\n").length - 1;
    const body = rule[2];
    for (const d of body.matchAll(/(?:^|[;\s])([\w-]+)\s*:\s*([^;}]+)/g)) {
      if (!PROPS.test(d[1]) || (textareas && HEIGHT.test(d[1]))) continue;
      const line = start + body.slice(0, d.index + d[0].length).split("\n").length;
      out.push({ file, line, selector: hit[0], prop: d[1], value: d[2].trim() });
    }
  }
  return out;
}

function walk(dir, ext, acc = []) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) walk(p, ext, acc);
    else if (ext.test(p)) acc.push(p);
  }
  return acc;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const src = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "..", "src");
  const classes = fieldClasses(walk(src, /\.tsx?$/).map((p) => readFileSync(p, "utf8")));
  const bad = walk(src, /\.css$/).flatMap((p) => offenders(readFileSync(p, "utf8"), relative(src, p), classes));
  for (const b of bad) console.error(`${b.file}:${b.line}: ${b.selector} { ${b.prop}: ${b.value} } draws a field. The kit draws it (shared/fields.css); place it, or use .field, .small or SearchField (scripts/check-fields.mjs).`);
  process.exit(bad.length ? 1 : 0);
}
