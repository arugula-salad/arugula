// The chip rule (#730): chips come in five shapes, and every one is drawn in
// src/shared/ui/state-chip.css (status `.chip`, tag `.chip-tag`, `.count`,
// `.chip-object`, `.hint`). No other stylesheet defines a new class ending
// in `-chip`, `-tag` or `-badge`: use the shape (shared/ui/state-chip.tsx),
// and put only its place (a margin, a position) in a class of your own.
//
//   node scripts/check-chips.mjs [src]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

/** The shared chip file, and the shared classes other files may name. */
const HOME = "shared/ui/state-chip.css";
const SHARED = new Set(["chip-tag"]);

/** Classes that predate the rule and haven't moved onto a shape yet. None are
 * left; don't add one. */
export const ALLOWED = new Set([]);

const CLASS = /\.([A-Za-z][\w-]*-(?:chip|tag|badge))(?![\w-])/g;

/** The offending class names in `css`, with their lines. */
export function offenders(css, file) {
  const out = [];
  const text = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  text.split("\n").forEach((line, i) => {
    for (const m of line.matchAll(CLASS)) {
      if (file === HOME || SHARED.has(m[1]) || ALLOWED.has(m[1])) continue;
      out.push({ file, line: i + 1, name: m[1] });
    }
  });
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
  for (const b of bad) console.error(`${b.file}:${b.line}: .${b.name} is a new chip. Use a shape from shared/ui/state-chip.tsx (scripts/check-chips.mjs).`);
  process.exit(bad.length ? 1 : 0);
}
