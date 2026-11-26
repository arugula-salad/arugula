#!/usr/bin/env node
// The design lift's ratchet (plan §9): every number `measure.mjs` prints
// under `budget.json`'s `max` stays at or under its ceiling. A ceiling is
// main's number when it was last lowered plus that number's `headroom`, so
// everyday web PRs (a few lines of CSS) pass while the web client can't
// drift up for long. A PR that grows one past its ceiling deletes
// something first, or raises the ceiling in `budget.json` and says why in
// its description. `--write` lowers each ceiling to today's number plus its
// headroom, never raises one: run it in a PR that shrinks something. The
// numbers under `report` are printed, never enforced: feature work grows
// them, and the lift's targets for them are end-of-overhaul totals.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { measure } from "./measure.mjs";

const file = fileURLToPath(new URL("../budget.json", import.meta.url));

/** The ceilings `now` breaks, as lines to print. */
export function over(budget, now) {
  return Object.entries(budget.max)
    .filter(([key, max]) => (now[key] ?? 0) > max)
    .map(([key, max]) => `${key}: ${now[key]} > ${max} (+${now[key] - max})`);
}

/** `budget` with each ceiling lowered to `now` plus its headroom, where that's lower. */
export function lowered(budget, now) {
  const max = Object.fromEntries(Object.entries(budget.max).map(([key, n]) => [key, key in now ? Math.min(n, now[key] + (budget.headroom?.[key] ?? 0)) : n]));
  return { ...budget, max };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const budget = JSON.parse(readFileSync(file, "utf8"));
  const now = measure();
  if (process.argv.includes("--write")) {
    writeFileSync(file, JSON.stringify(lowered(budget, now), null, 2) + "\n");
    process.exit(0);
  }
  const broken = over(budget, now);
  if (broken.length) {
    // On stdout as well as stderr, and as an annotation on GitHub, so CI's
    // log and summary show which number went over.
    const lines = ["check-budget: the web client grew past web/budget.json (design lift, plan §9):", ...broken.map((l) => `  ${l}`), "Delete what this replaces, or raise the ceiling in web/budget.json and say why in the PR."];
    console.log(lines.join("\n"));
    console.error(lines.join("\n"));
    if (process.env.GITHUB_ACTIONS) console.log(`::error title=check-budget::${broken.join("; ")}`);
    process.exit(1);
  }
  const lower = Object.keys(budget.max).filter((key) => (now[key] ?? 0) + (budget.headroom?.[key] ?? 0) < budget.max[key]);
  if (lower.length) console.log(`check-budget: ${lower.join(", ")} went down; \`node scripts/check-budget.mjs --write\` lowers the budget.`);
}
