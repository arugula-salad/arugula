// The perf job's timings (e2e/timing.ts's `timing` annotations), from
// Playwright's JSON report (E2E_TIMINGS), as a Markdown table: each
// attempt's number against its budget, over budget marked. check.yml
// appends it to the job's summary; the numbers don't fail the job.
//
//   node scripts/perf-summary.mjs REPORT.json >> "$GITHUB_STEP_SUMMARY"
import { existsSync, readFileSync } from "node:fs";

const [path] = process.argv.slice(2);
if (!path || !existsSync(path)) {
  console.log("### Timings\n\nNo report: the specs didn't run.");
  process.exit(0);
}
const report = JSON.parse(readFileSync(path, "utf8"));
const rows = [];
const walk = (s) => {
  for (const spec of s.specs ?? [])
    for (const t of spec.tests)
      for (const r of t.results)
        for (const a of r.annotations ?? []) {
          const m = a.type === "timing" && /^(.*): (\d+) ms \(budget (\d+) ms\)$/.exec(a.description ?? "");
          if (m) rows.push({ what: m[1], ms: +m[2], budget: +m[3], attempt: r.retry + 1, status: r.status });
        }
  for (const c of s.suites ?? []) walk(c);
};
for (const s of report.suites ?? []) walk(s);

const out = ["### Timings", ""];
if (!rows.length) out.push("No timings reported.");
else {
  out.push("| Timing | ms | Budget | | Attempt | Test |", "|---|--:|--:|---|--:|---|");
  for (const r of rows) out.push(`| ${r.what} | ${r.ms} | ${r.budget} | ${r.ms > r.budget ? "**over**" : "ok"} | ${r.attempt} | ${r.status} |`);
  const over = rows.filter((r) => r.ms > r.budget).length;
  out.push("", over ? `${over} of ${rows.length} over budget: reported, not failed.` : `All ${rows.length} within budget.`);
}
console.log(out.join("\n"));
