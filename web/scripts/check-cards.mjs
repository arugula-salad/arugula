// The card rule (#698): a card is drawn once, by `Card` (src/shared/ui/card.tsx,
// card.css). Three things fail:
//   1. a rule for a class ending in `-card` outside shared/ui/card.css (a
//      second card look). Style a card's place (its width, its grid) in a
//      class of your own that doesn't end in `-card`.
//   2. a literal `border-radius` on a `.card` (or `.card-…`): its corners are
//      `--card-radius` (a dense one's `--field-radius`), so Corners holds.
//      The 1-3px bar of a tone is a mark: a `::before` or `::after` is exempt.
//   3. a new `.sw-card` or `.sw-cards`, the swarm card Card replaced.
//
//   node scripts/check-cards.mjs [src]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const HOME = "shared/ui/card.css";

/** `-card` classes that predate the rule: each is a card of its own, to move
 * onto `Card` in a follow-up. Don't add one; the list only gets shorter. */
export const ALLOWED = new Set([
  "agent-card", // settings' agent grants: a Card, with its own table inside
  "block-error-card", // a block's error
  "browser-card", // the browser block's prompt
  "control-both-card", // control's trust UI
  "deploy-card", // a deploy's card, also drawn headless (no Card)
  "diff-card", // the review block's diff
  "empty-state-card", // an empty state's dashed card
  "env-card", // settings' environment: a Card, with its own head
  "hand-card", // a hand's prompt
  "link-card", // a chat link's preview
  "offer-card", // the agents offer
  "session-card", // the session's own width and arrival, on a Card
  "sheet-card", // the phone sheet's
  "swarm-card", // the shell swarm's tiles
  "telemetry-card", // the telemetry opt-in
  "ws-card", // a workspace's
]);

const CARD_CLASS = /\.([A-Za-z][\w-]*-card)(?![\w-])/g;
const ON_CARD = /\.card(?:-[A-Za-z][\w-]*)?(?![\w-])/;
const SWARM = /(?<![\w-])sw-cards?(?![\w-])/;
const LITERAL = /(?:^|[^\w.-])\d*\.?\d+(?:px|em|rem|%|vw|vh|ch)\b/;

function strip(css) {
  return css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
}

/** The offending things in `css` (read from `file`, relative to src), with their lines. */
export function offenders(css, file) {
  file = file.replaceAll("\\", "/");
  const out = [];
  const text = strip(css);
  for (const rule of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = rule[1].replace(/^[\s\S]*;\s*/, "");
    const lead = selector.length - selector.trimStart().length;
    const at = (i) => text.slice(0, i).split("\n").length;
    const line = at(rule.index + rule[1].length - selector.length + lead);
    const sels = selector.split(",").map((s) => s.trim());
    if (file !== HOME) {
      for (const m of selector.matchAll(CARD_CLASS)) {
        if (m[1] !== "sw-card" && !ALLOWED.has(m[1])) out.push({ file, line, rule: "card", found: `.${m[1]}` });
      }
    }
    if (sels.some((s) => ON_CARD.test(s) && !/::?(?:before|after)\s*$/.test(s))) {
      for (const d of rule[2].matchAll(/(?:^|[;\s])border-radius\s*:\s*([^;}]+)/g)) {
        const value = d[1].replace(/var\([^)]*\)/g, "var()");
        if (LITERAL.test(value)) out.push({ file, line: at(rule.index + rule[1].length + 1 + d.index + 1), rule: "radius", found: d[1].trim() });
      }
    }
    if (SWARM.test(selector)) out.push({ file, line, rule: "swarm", found: selector.trim() });
  }
  return out;
}

/** A `sw-card` or `sw-cards` in a TSX or TS file's code. */
export function swarmOffenders(source, file) {
  const out = [];
  source.split("\n").forEach((line, i) => {
    if (/^\s*(?:\/\/|\*|\/\*)/.test(line)) return;
    if (SWARM.test(line)) out.push({ file, line: i + 1, rule: "swarm", found: line.trim() });
  });
  return out;
}

function walk(dir, acc = []) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) walk(p, acc);
    else if (/\.(css|tsx?)$/.test(p)) acc.push(p);
  }
  return acc;
}

const WHY = {
  card: (f) => `${f} is a second card look. Use Card (shared/ui/card.tsx); its place goes in a class that doesn't end in -card (scripts/check-cards.mjs).`,
  radius: (f) => `border-radius: ${f} on a card. Its corners are --card-radius (scripts/check-cards.mjs).`,
  swarm: () => "sw-card is gone: use Card tone (shared/ui/card.tsx) (scripts/check-cards.mjs).",
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const src = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "..", "src");
  const bad = walk(src).flatMap((p) => {
    const file = relative(src, p).replaceAll("\\", "/");
    const text = readFileSync(p, "utf8");
    return p.endsWith(".css") ? offenders(text, file) : swarmOffenders(text, file);
  });
  for (const b of bad) console.error(`${b.file}:${b.line}: ${WHY[b.rule](b.found)}`);
  process.exit(bad.length ? 1 : 0);
}
