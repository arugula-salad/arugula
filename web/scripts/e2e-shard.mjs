// check.yml's e2e shards by time, not by count (#868): Playwright's --shard
// splits by test count, and six shards ran 155-316 s. This lists the spec
// files the config selects (E2E_SET and the rest, as `playwright test`
// would), weighs each by e2e/durations.json (what `just e2e-durations`
// recorded from a run), a file it doesn't know by its tests at the known
// files' average, and deals them out longest first to the lightest shard.
// Prints shard N's files as path filters for `playwright test`. With E2E_ONLY,
// spec files separated by spaces, it deals only those (a pull request's run,
// scripts/pr-plan), and a shard can come up empty.
//
//   node scripts/e2e-shard.mjs N M
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";

const [n, m] = process.argv.slice(2).map(Number);
if (!(n >= 1 && m >= n)) {
  console.error("usage: node scripts/e2e-shard.mjs N M");
  process.exit(2);
}

const list = JSON.parse(
  execFileSync("node_modules/.bin/playwright", ["test", "--list", "--reporter=json"], { encoding: "utf8", maxBuffer: 64 << 20 }),
);
const count = (s) => (s.specs ?? []).reduce((a, sp) => a + sp.tests.length, 0) + (s.suites ?? []).reduce((a, c) => a + count(c), 0);
const only = process.env.E2E_ONLY ? new Set(process.env.E2E_ONLY.split(/\s+/)) : null;
const files = list.suites.map((s) => ({ file: s.file, tests: count(s) })).filter((f) => !only || only.has(f.file));

const known = existsSync("e2e/durations.json") ? JSON.parse(readFileSync("e2e/durations.json", "utf8")) : {};
const seen = files.filter((f) => f.file in known);
const perTest = seen.length ? seen.reduce((a, f) => a + known[f.file], 0) / seen.reduce((a, f) => a + f.tests, 0) : 1;
const weight = (f) => known[f.file] ?? f.tests * perTest;

const shards = Array.from({ length: m }, () => ({ total: 0, files: [] }));
for (const f of [...files].sort((a, b) => weight(b) - weight(a) || a.file.localeCompare(b.file))) {
  const lightest = shards.reduce((a, s) => (s.total < a.total ? s : a));
  lightest.total += weight(f);
  lightest.files.push(f.file);
}
const mine = shards[n - 1];
console.error(`shard ${n}/${m}: ${mine.files.length} files, ~${Math.round(mine.total)} s (${shards.map((s) => Math.round(s.total)).join(" ")})`);
// A filter is a regular expression on the file's path: anchored, so one
// file's name never picks up another's.
for (const f of mine.files) console.log(`/e2e/${f.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
