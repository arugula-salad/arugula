// e2e/durations.json from the JSON reports check.yml's e2e shards keep
// (E2E_TIMINGS, artifacts e2e-timings-N): each spec file's seconds, every
// attempt counted, for scripts/e2e-shard.mjs (#868). `just e2e-durations
// RUN` downloads a run's reports and writes it.
//
//   node scripts/e2e-durations.mjs REPORT.json... > e2e/durations.json
import { readFileSync } from "node:fs";

const secs = {};
const walk = (s, file) => {
  for (const sp of s.specs ?? []) for (const t of sp.tests) for (const r of t.results) secs[file] = (secs[file] ?? 0) + r.duration / 1000;
  for (const c of s.suites ?? []) walk(c, file);
};
for (const path of process.argv.slice(2)) for (const s of JSON.parse(readFileSync(path, "utf8")).suites) walk(s, s.file);
const out = Object.fromEntries(Object.keys(secs).sort().map((f) => [f, Math.round(secs[f] * 10) / 10]));
console.log(JSON.stringify(out, null, 2));
