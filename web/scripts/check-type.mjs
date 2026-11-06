// The type rule (#687): a stylesheet sets a size only through the scale.
// `font-size` and the size in a `font:` shorthand are a `--text-*` or
// `--t-*` token, `inherit` or `0`; `letter-spacing` is `0`, `normal`,
// `inherit` or a `--track-*` token. The scale is in shared/style.css.
//
// What isn't read as text keeps its px: a glyph or avatar initials sized to
// its box, the start code. Say so on the declaration's line, or the line
// above it, with `/* off-scale: why */`. The Doc's paper (a rule whose
// selectors all start with `.doc`) is exempt too. The terminal's size is
// terminal-view.ts's, not CSS.
//
//   node scripts/check-type.mjs [src]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const SIZE_OK = /^(?:var\(--(?:text|t)-[\w-]+\)|inherit|0)$/;
const TRACK_OK = /^(?:0|normal|inherit|var\(--track-[\w-]+\))$/;
const MARK = /off-scale/;

/** The first size in a `font:` shorthand: the token before the line-height
 * slash or the family, after the optional style and weight words. */
function shorthandSize(v) {
  const m = v.match(/^(?:(?:italic|oblique|normal|small-caps|bold|bolder|lighter|\d{3})\s+)*(var\([^)]*\)|clamp\([^)]*\)|[\d.]+[a-z%]*|[a-z-]+)(?=\s|\/|$)/);
  return m ? m[1] : null;
}

/** The offending declarations in `css`, with their lines. */
export function offenders(css, file) {
  const out = [];
  const marked = new Set();
  const lines = css.split("\n");
  lines.forEach((l, i) => {
    if (MARK.test(l)) {
      marked.add(i + 1);
      marked.add(i + 2);
    }
  });
  const text = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  for (const rule of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const sels = rule[1].split(",").map((s) => s.trim().replace(/^[\s\S]*;\s*/, ""));
    if (sels.every((s) => /^\.doc(?![a-z])/.test(s))) continue;
    const start = text.slice(0, rule.index + rule[1].length + 1).split("\n").length - 1;
    const body = rule[2];
    for (const d of body.matchAll(/(?:^|[;\s])(font-size|font|letter-spacing)\s*:\s*([^;}]+)/g)) {
      const prop = d[1];
      const value = d[2].trim();
      const line = start + body.slice(0, d.index + d[0].length).split("\n").length;
      let bad;
      if (prop === "font-size") bad = !SIZE_OK.test(value);
      else if (prop === "letter-spacing") bad = !TRACK_OK.test(value);
      else if (/^(?:inherit|initial|unset)$/.test(value)) bad = false;
      else {
        const size = shorthandSize(value);
        bad = size === null || !SIZE_OK.test(size);
      }
      if (bad && !marked.has(line)) out.push({ file, line, prop, value });
    }
  }
  return out;
}

function walk(dir, acc = []) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) walk(p, acc);
    else if (p.endsWith(".css")) acc.push(p);
  }
  return acc;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const src = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "..", "src");
  const bad = walk(src).flatMap((p) => offenders(readFileSync(p, "utf8"), relative(src, p)));
  for (const b of bad) console.error(`${b.file}:${b.line}: ${b.prop}: ${b.value} is off the scale. Use a --text-* or --t-* size, or a --track-* spacing (scripts/check-type.mjs).`);
  process.exit(bad.length ? 1 : 0);
}
