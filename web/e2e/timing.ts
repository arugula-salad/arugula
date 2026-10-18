import { test } from "@playwright/test";

/** A timing measured against a budget, reported and not asserted: CI's
 * pods are shared, and specs that failed over the number turned main red
 * with nothing broken (10 red runs in a week, 2026-10-18). The number goes
 * in the log and in a `timing` annotation, which `scripts/perf-summary.mjs`
 * turns into the perf job's summary table, over budget marked. What the
 * spec checks around it (the file opens, at its line) still fails it. */
export function timing(what: string, ms: number, budgetMs: number): void {
  const description = `${what}: ${ms} ms (budget ${budgetMs} ms)`;
  console.log(ms > budgetMs ? `${description}: over budget` : description);
  test.info().annotations.push({ type: "timing", description });
}
