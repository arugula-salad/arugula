// The kit rule (design lift §4.1, Arugula #1478): a surface, a row, a chip,
// a badge or a pill is a part of the kit (src/shared/kit/), never a screen's
// own. A .tsx outside shared/kit/ that sets a class ending in `-card`,
// `-chip`, `-badge`, `-pill` or `-row` fails: use Surface, Row, NeedRow or
// StateMark, and put only the part's place in a class of your own. A part the
// kit doesn't have yet needs a design-session sign-off in its PR, and deletes
// one.
//
//   node scripts/check-kit.mjs [src]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

/** Classes that predate the kit: each screen's lift moves its own onto a
 * part and takes it off. Don't add one; the list only gets shorter. */
export const ALLOWED = new Set([
  "add-pane-row",
  "app-row",
  "block-error-card",
  "board-doc-row",
  "board-row",
  "browser-card",
  "chat-row",
  "deploy-card",
  "deploy-row",
  "diff-card",
  "doc-head-row",
  "empty-state-card",
  "env-card",
  "env-secret-row",
  "forge-row",
  "link-card",
  "list-row",
  "mention-machine-row",
  "menu-filter-row",
  "offer-card",
  "ov-ask-row",
  "ov-row",
  "palette-row",
  "pane-ask-pill",
  "panes-row",
  "pick-row",
  "picker-row",
  "proposal-card",
  "react-row",
  "run-row",
  "session-card",
  "session-stat-row",
  "settings-row",
  "sheet-card",
  "sheet-row",
  "steps-row",
  "switcher-row",
  "telemetry-card",
  "thread-row",
  "triage-row",
  "ws-activity-row",
  "ws-card",
  "ws-rail-pill",
  "ws-row",
  "ws-why-row",
  "yw-row",
]);

const NAME = /^[a-z][\w-]*-(?:card|chip|badge|pill|row)$/;

/** Each class value a .tsx sets: `class="…"`, `className="…"`, or the
 * whole of `class={…}` (its braces balanced), with its offset. */
function classValues(source) {
  const out = [];
  for (const m of source.matchAll(/\bclass(?:Name)?=(["{])/g)) {
    const start = m.index + m[0].length;
    if (m[1] === '"') {
      out.push([source.slice(start, source.indexOf('"', start)), m.index]);
      continue;
    }
    let depth = 1;
    let i = start;
    for (; i < source.length && depth; i++) depth += source[i] === "{" ? 1 : source[i] === "}" ? -1 : 0;
    // A hyphened word in it can only be in a string: a class.
    out.push([source.slice(start, i - 1), m.index]);
  }
  return out;
}

/** The offending class names a .tsx sets, with their lines. */
export function offenders(source, file) {
  if (file.startsWith("shared/kit/")) return [];
  const out = [];
  for (const [body, at] of classValues(source)) {
    const line = source.slice(0, at).split("\n").length;
    for (const name of body.split(/[^\w-]+/)) {
      if (NAME.test(name) && !ALLOWED.has(name)) out.push({ file, line, name });
    }
  }
  return out;
}

function walk(dir, acc = []) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) walk(p, acc);
    else if (p.endsWith(".tsx")) acc.push(p);
  }
  return acc;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const src = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "..", "src");
  const bad = walk(src).flatMap((p) => offenders(readFileSync(p, "utf8"), relative(src, p).split("\\").join("/")));
  for (const b of bad) console.error(`${b.file}:${b.line}: .${b.name} is a part of its own. Use the kit's (src/shared/kit/: Surface, Row, NeedRow, StateMark) and give only its place a class (scripts/check-kit.mjs).`);
  process.exit(bad.length ? 1 : 0);
}
