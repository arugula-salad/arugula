// The colour rule (#689): a stylesheet takes its colours from the palette.
// A raw colour (a hex, `rgb()`, `rgba()`, `hsl()`, `hsla()`) fails, and so
// does `var(--x)` followed by hex digits (`var(--accent)40` is not a tint;
// it's invalid, and the declaration is dropped). Use the token, or
// `color-mix(in srgb, var(--token) N%, transparent)` for a tint. Shadows are
// `--shadow-sm`, `--shadow-md` and `--shadow-lg`; what dims a page is
// `--backdrop`. The tokens are in shared/style.css and appearance.ts.
//
// What is allowed:
//   1. The tokens' own definitions: a custom property in a `:root` or an
//      `html[data-…]` rule in shared/style.css. A custom property set to a
//      raw colour anywhere else fails.
//   2. The finish: a rule whose selectors all start with `html[data-finish`
//      (or `[data-look`, the settings' swatch of a finish), and platform/finish.css and shared/buttons.css. There only white and
//      black as an alpha base: `#fff`, `#000`, `rgb(255 255 255 / a)`,
//      `rgb(0 0 0 / a)`. Any other colour there still fails.
//   3. A mask: `mask`, `mask-image` and `-webkit-mask*` (`#000` is an alpha
//      mask, not a colour).
//   4. A media fill that has to stay literal (a browser frame's white, a
//      camera's black): `/* lint: literal — why */` on the declaration's
//      line or the line above. The reason after the dash is required.
//
//   node scripts/check-colour.mjs [src]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const HEX = /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![\w-])/g;
const FUNC = /\b(?:rgba?|hsla?)\(/gi;
const TINT = /var\(--[\w-]+\)[0-9a-fA-F]{2,8}(?![\w.%-])/g;
const MARK = /lint:\s*literal\s*[—–-]+\s*\S/;
const MASK = /^(?:-webkit-)?mask(?:-[\w-]+)?$/;
const TOKEN_SELECTOR = /^(?::root|html\[data-[^\]]+\])$/;
const FINISH_SELECTOR = /^(?:html)?\[data-(?:finish|look)\b/;
const TOKEN_FILE = "shared/style.css";
const FINISH_FILES = new Set(["platform/finish.css", "shared/buttons.css"]);
// White or black, as the base of an alpha.
const ALPHA_HEX = /^#(?:(?:fff|000)[0-9a-f]?|(?:ffffff|000000)(?:[0-9a-f]{2})?)$/i;
const ALPHA_FUNC = /^rgba?\(\s*(?:255[\s,]+255[\s,]+255|0[\s,]+0[\s,]+0)\s*[/,]\s*[^)]+\)$/i;

/** Every raw colour in a value, with url(…) set aside (its `#id` is no colour). */
function colours(value) {
  const v = value.replace(/url\([^)]*\)/g, "url()");
  const found = [...v.matchAll(HEX)].map((m) => m[0]);
  for (const m of v.matchAll(FUNC)) {
    let depth = 1;
    let i = m.index + m[0].length;
    for (; i < v.length && depth; i++) depth += v[i] === "(" ? 1 : v[i] === ")" ? -1 : 0;
    found.push(v.slice(m.index, i));
  }
  return found;
}

/** The offending declarations in `css` (read from `file`, relative to src), with their lines. */
export function offenders(css, file) {
  const out = [];
  const marked = new Set();
  for (const c of css.matchAll(/\/\*([\s\S]*?)\*\//g)) {
    if (!MARK.test(c[1])) continue;
    const line = css.slice(0, c.index + c[0].length).split("\n").length;
    marked.add(line);
    marked.add(line + 1);
  }
  const text = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  for (const rule of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const sels = rule[1].split(",").map((s) => s.trim().replace(/^[\s\S]*;\s*/, ""));
    const tokens = file === TOKEN_FILE && sels.every((s) => TOKEN_SELECTOR.test(s));
    const finish = FINISH_FILES.has(file) || sels.every((s) => FINISH_SELECTOR.test(s));
    const start = text.slice(0, rule.index + rule[1].length + 1).split("\n").length - 1;
    const body = rule[2];
    for (const d of body.matchAll(/(?:^|[;\s])([\w-]+)\s*:\s*([^;}]+)/g)) {
      const prop = d[1];
      const value = d[2].trim();
      const line = start + body.slice(0, d.index + d[0].length).split("\n").length;
      if (marked.has(line) || MASK.test(prop)) continue;
      const raw = colours(value);
      const tint = [...value.matchAll(TINT)].map((m) => m[0]);
      if (tokens && prop.startsWith("--") && !tint.length) continue;
      const stray = finish ? raw.filter((c) => !ALPHA_HEX.test(c) && !ALPHA_FUNC.test(c)) : raw;
      if (stray.length || tint.length) out.push({ file, line, prop, value, found: [...stray, ...tint] });
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
  for (const b of bad) console.error(`${b.file}:${b.line}: ${b.prop}: ${b.found.join(" ")} is a raw colour. Use a palette token, or color-mix() of one (scripts/check-colour.mjs).`);
  process.exit(bad.length ? 1 : 0);
}
