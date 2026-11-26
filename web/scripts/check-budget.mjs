#!/usr/bin/env node
// The design lift's ratchet (plan §9): every number `measure.mjs` prints
// under `budget.json`'s `max` may only go down. A PR that grows one deletes
// something first, or raises the ceiling in `budget.json` and says why in its
// description. `--write` lowers each ceiling to today's number; run it in a
// PR that shrinks something. The numbers under `report` are printed, never
// enforced: feature work grows them, and the lift's targets for them are
// end-of-overhaul totals.
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

/** `budget` with each ceiling lowered to `now` where `now` is lower. */
export function lowered(budget, now) {
  const max = Object.fromEntries(Object.entries(budget.max).map(([key, n]) => [key, Math.min(n, now[key] ?? n)]));
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
    console.error("check-budget: the web client grew past web/budget.json (design lift, plan §9):");
    for (const line of broken) console.error(`  ${line}`);
    console.error("Delete what this replaces, or raise the ceiling in web/budget.json and say why in the PR.");
    process.exit(1);
  }
  const lower = Object.keys(budget.max).filter((key) => (now[key] ?? 0) < budget.max[key]);
  if (lower.length) console.log(`check-budget: ${lower.join(", ")} went down; \`node scripts/check-budget.mjs --write\` lowers the budget.`);
}
