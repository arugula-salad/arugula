// The icon rule (#691): an icon is drawn from shared/ui/icons.tsx, never typed
// as a glyph, and a button that is only an icon says what it does.
//
//   1. No glyph of the set (▾ ▸ ⋯ ✕ × ✓ ↗ ‹ › ↻ ✎ ⤢ ★ ☆ ↩ ↑ ↓ →) as JSX text or
//      as a string a JSX child chooses (`{open ? "▾" : "▸"}`), or in a
//      stylesheet's `content:`. Use an icon: `ChevronIcon`, `CloseIcon`,
//      `MoreIcon`, `ExternalIcon`, `CheckIcon`…
//      What stays text: `…` and `⌘` (not in the set), a glyph in the middle of a
//      sentence ("Share › Add to Home Screen"), a key's name inside `<kbd>`, a
//      count (`×2`, `×{n}`), `›_` (a prompt), and an arrow between two
//      values (`{from} → {to}`). Anything else says so on its line, or the
//      line above, with `lint-icons: text — why` (a comment).
//   2. A size is a name, never a number: `size="xs|sm|md|lg"`, not `size={13}`.
//   3. No `<svg>` outside shared/ui/icons.tsx, except what isn't an icon (a QR code,
//      a chart, an illustration, a layout thumbnail): ALLOWED_SVG below says
//      why, file by file.
//   4. A `<button>` with an icon and nothing else needs both a `title` and an
//      `aria-label`. `IconButton` (icons.tsx) takes one `label` and sets both.
//
//   node scripts/check-icons.mjs [src]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const GLYPHS = "▾▸⋯✕×✓↗‹›↻✎⤢★☆↩↑↓→";
const G = `[${GLYPHS}]`;
const MARK = /lint-icons:\s*text\s*[—–-]+\s*\S/;
const ICONS_FILE = "shared/ui/icons.tsx";
/** Files with an `<svg>` that is not an icon, and why. */
export const ALLOWED_SVG = {
  "shell/doc-view.tsx": "the leaf a Doc made by Arugula carries: an illustration",
  "shell/ui/welcome.tsx": "a QR code, drawn from the code's modules",
  "shell/trust/common.tsx": "a QR code, drawn from the code's modules",
  "shell/ui/shell/pane-toolbar.tsx": "the layout presets' thumbnails: rectangles of the layout, not an icon",
  "shared/boards/steps-view.tsx": "the lines between a board's steps: a graph, sized by what it joins",
};

/** A path as the rules name it: forward slashes, as on every platform. */
export const normalise = (p) => p.replaceAll("\\", "/");

const blank = (s) => s.replace(/[^\n]/g, " ");

/** `src` with comments and `<kbd>` (a key's name) set to spaces, lines kept. */
function stripped(src) {
  return src
    .replace(/\{\s*\/\*[\s\S]*?\*\/\s*\}/g, blank)
    .replace(/\/\*[\s\S]*?\*\//g, blank)
    .replace(/^([ \t]*)\/\/.*$/gm, (m) => blank(m))
    .replace(/<kbd\b[^>]*>[\s\S]*?<\/kbd>/g, blank);
}

const lineOf = (text, i) => text.slice(0, i).split("\n").length;

/** The end of the opening tag that starts at `start` (after `<name`): the index of its `>`. */
function tagEnd(text, start) {
  let depth = 0;
  let quote = null;
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if ((c === '"' || c === "'") && depth === 0) quote = c;
    else if (c === "{") depth++;
    else if (c === "}") depth--;
    else if (c === ">" && depth === 0 && text[i - 1] !== "=") return i;
  }
  return -1;
}

/** The offending lines in a `.tsx` file's `src`, read from `file` (relative to src). */
function tsx(src, file) {
  const out = [];
  const text = stripped(src);
  const raw = src.split("\n");
  const marked = (line) => MARK.test(raw[line - 1] ?? "") || MARK.test(raw[line - 2] ?? "");
  const add = (line, rule, found) => {
    if (!marked(line)) out.push({ file, line, rule, found });
  };

  // 1. A glyph as JSX text: between `>` or `}` and `<` or `{`, at either end of what's said.
  for (const m of text.matchAll(/(?<=[>}])([^<>{}]+)(?=[<{])/g)) {
    const t = m[1];
    const trimmed = t.trim();
    if (!trimmed) continue;
    const before = text[m.index - 1];
    const after = text[m.index + t.length];
    for (const first of trimmed.length > 1 ? [true, false] : [true]) {
      const at = first ? trimmed[0] : trimmed[trimmed.length - 1];
      if (!GLYPHS.includes(at)) continue;
      const pos = m.index + t.indexOf(trimmed) + (first ? 0 : trimmed.length - 1);
      if (at === "×" && (after === "{" || /\d/.test(text[pos + 1] ?? "") || /\d/.test(text[pos - 1] ?? ""))) continue;
      if (at === "›" && text[pos + 1] === "_") continue;
      if (at === "→" && trimmed === "→" && before === "}" && after === "{") continue;
      add(lineOf(text, pos), "glyph", at);
    }
  }
  // 1b. A glyph a JSX child chooses: `{open ? "▾" : "▸"}`, `{on && "✓"}`.
  const lit = `["'\`]\\s*${G}+\\s*["'\`]`;
  for (const re of [new RegExp(`\\{[^{}\\n]*\\?\\s*${lit}`, "g"), new RegExp(`:\\s*${lit}\\s*\\}`, "g"), new RegExp(`&&\\s*${lit}`, "g")]) {
    for (const m of text.matchAll(re)) add(lineOf(text, m.index + m[0].search(G)), "glyph", m[0].match(G)[0]);
  }

  // 2. A number as an icon's size.
  for (const m of text.matchAll(/<(?:[A-Z]\w*Icon|Svg)\b[^<>]*?\bsize=\{\s*\d/g)) add(lineOf(text, m.index), "size", "size={n}");

  // 3. An `<svg>` that isn't in icons.tsx.
  if (file !== ICONS_FILE && !(file in ALLOWED_SVG)) for (const m of text.matchAll(/<svg\b/g)) add(lineOf(text, m.index), "svg", "<svg>");

  // 4. A button that is only an icon, without both names.
  for (const m of text.matchAll(/<button\b/g)) {
    const end = tagEnd(text, m.index + 7);
    if (end < 0 || text[end - 1] === "/") continue;
    const close = text.indexOf("</button>", end);
    if (close < 0) continue;
    const attrs = text.slice(m.index + 7, end);
    const kids = text.slice(end + 1, close).trim();
    if (!/^(?:<(?:\w*Icon|Caret)\b[^<>]*\/>\s*)+$/.test(kids)) continue;
    if (/\{\s*\.\.\./.test(attrs)) continue;
    const has = (name) => new RegExp(`(?:^|\\s)${name}\\s*=`).test(attrs);
    if (!has("title") || !has("aria-label")) add(lineOf(text, m.index), "button", has("title") ? "aria-label" : has("aria-label") ? "title" : "title and aria-label");
  }
  return out;
}

/** The offending declarations in a stylesheet's `css`. */
function css(src, file) {
  const out = [];
  const text = src.replace(/\/\*[\s\S]*?\*\//g, blank);
  for (const m of text.matchAll(new RegExp(`content\\s*:[^;}]*${G}`, "g"))) out.push({ file, line: lineOf(text, m.index), rule: "content", found: m[0].match(G)[0] });
  return out;
}

/** The offences in `src` (a file's text), read from `file`, a path relative to src. */
export function offenders(src, file) {
  const f = normalise(file);
  return f.endsWith(".css") ? css(src, f) : tsx(src, f);
}

const SAYS = {
  glyph: (f) => `${f} is a glyph. Draw it with an icon from shared/ui/icons.tsx (ChevronIcon, CloseIcon, MoreIcon, ExternalIcon, CheckIcon…).`,
  content: (f) => `${f} in content: is a glyph. Put an icon in the markup (shared/ui/icons.tsx).`,
  size: () => `an icon's size is a name (size="sm"), not a number.`,
  svg: () => `an <svg> belongs in shared/ui/icons.tsx. If this one isn't an icon, say why in ALLOWED_SVG (scripts/check-icons.mjs).`,
  button: (f) => `an icon-only button needs a ${f}. Use IconButton (shared/ui/icons.tsx): its label sets both.`,
};

function walk(dir, acc = []) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) walk(p, acc);
    else if (p.endsWith(".tsx") || p.endsWith(".css")) acc.push(p);
  }
  return acc;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const src = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "..", "src");
  const bad = walk(src).flatMap((p) => offenders(readFileSync(p, "utf8"), normalise(relative(src, p))));
  for (const b of bad) console.error(`${b.file}:${b.line}: ${SAYS[b.rule](b.found)} (scripts/check-icons.mjs)`);
  process.exit(bad.length ? 1 : 0);
}
