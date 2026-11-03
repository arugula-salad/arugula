#!/usr/bin/env node
// `just map`: what the code and CI say right now, for keeping up
// (docs/capabilities/BRIEFING.md). Facts only, no judgement, and nothing
// here is committed: run it again rather than keep its output.
//
//   node scripts/map.mjs [--since "7 days ago"] [--no-ci] [--runs 300]
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { records } from "./capabilities.mjs";

const root = process.cwd();
const repo = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const opt = (name, dflt) => (args.includes(name) ? args[args.indexOf(name) + 1] : dflt);
const since = opt("--since", "7 days ago");
const runs = Number(opt("--runs", 300));

const git = (...a) => execFileSync("git", a, { cwd: root, encoding: "utf8", maxBuffer: 1 << 28 }).trim();
const lines = (s) => (s ? s.split("\n") : []);
const lineCount = (p) => {
  try {
    return readFileSync(join(root, p), "utf8").split("\n").length - 1;
  } catch {
    return 0;
  }
};
const head = (title) => console.log(`\n## ${title}\n`);
const table = (rows) => {
  if (!rows.length) return console.log("(none)");
  const w = rows[0].map((_, i) => Math.max(...rows.map((r) => String(r[i]).length)));
  for (const r of rows) console.log(r.map((c, i) => (i === r.length - 1 ? String(c) : String(c).padEnd(w[i]))).join("  ").trimEnd());
};
const GENERATED = /(\.gen\.ts$|(^|\/)THIRD_PARTY|Cargo\.lock$|pnpm-lock\.yaml$|package-lock\.json$|^DECISIONS\.md$|^docs\/capabilities\/README\.md$|durations\.json$|\/fixtures\/|^contract\/)/;

console.log(`# Map of ${git("rev-parse", "--short=9", "HEAD")}, commits since "${since}"`);

// The week's commits and the files in each.
const log = git("log", `--since=${since}`, "--no-merges", "--format=%x00%h %s", "--name-only");
const commits = log
  .split("\0")
  .filter(Boolean)
  .map((c) => {
    const [subject, ...files] = c.trim().split("\n");
    return { subject, files: files.filter(Boolean) };
  });
console.log(`${commits.length} commits.`);

// Feature records.
let recs = [];
if (existsSync(join(root, "docs/capabilities"))) recs = records(root);
const under = (file, p) => file === p || file.startsWith(p.endsWith("/") ? p : `${p}/`);
head("Feature records");
const decisions = existsSync(join(root, "docs/decisions")) ? readdirSync(join(root, "docs/decisions")) : [];
table(
  recs.map((r) => {
    const own = [...r.entry, ...r.paths];
    const there = own.filter((p) => existsSync(join(root, p)));
    let behind = "? (its paths are all gone)";
    if (!execOk("merge-base", "--is-ancestor", r.verified_through, "HEAD")) behind = "? (unknown commit)";
    else if (there.length) behind = git("rev-list", "--count", `${r.verified_through}..HEAD`, "--", ...there);
    const dead = [...own, ...r.shared, ...r.tests].filter((p) => !existsSync(join(root, p)));
    const lost = r.decisions.filter((d) => !decisions.some((f) => f.startsWith(`${d}-`)));
    const odd = [...dead.map((p) => `gone: ${p}`), ...lost.map((d) => `no decision: ${d}`)].join(", ");
    return [r.id, `${behind} behind`, odd];
  }),
);
function execOk(...a) {
  try {
    execFileSync("git", a, { cwd: root, stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

// Where the commits went, by record.
head("Where the commits went");
const byRecord = new Map(recs.map((r) => [r.id, 0]));
let mapped = 0;
for (const c of commits) {
  let hit = false;
  for (const r of recs) {
    if (c.files.some((f) => [...r.entry, ...r.paths].some((p) => under(f, p)))) {
      byRecord.set(r.id, byRecord.get(r.id) + 1);
      hit = true;
    }
  }
  if (hit) mapped++;
}
table([...byRecord].sort((a, b) => b[1] - a[1]).map(([id, n]) => [String(n).padStart(4), id]));
console.log(`\n${commits.length - mapped} of ${commits.length} commits touch no record's paths.`);

// Churn.
const churn = new Map();
for (const c of commits) for (const f of c.files) churn.set(f, (churn.get(f) ?? 0) + 1);
const touching = (re) => commits.filter((c) => c.files.some((f) => re.test(f))).length;

head("Hot files: lines × commits");
table(
  [...churn]
    .filter(([f]) => !GENERATED.test(f) && !f.startsWith("docs/"))
    .map(([f, n]) => ({ f, n, l: lineCount(f) }))
    .sort((a, b) => b.l * b.n - a.l * a.n)
    .slice(0, 10)
    .map(({ f, n, l }) => [String(l).padStart(6), `× ${String(n).padStart(3)}`, f]),
);

head("Generated files: commits touching each");
table(
  [...churn]
    .filter(([f]) => GENERATED.test(f))
    .sort((a, b) => b[1] - a[1])
    .slice(0, 6)
    .map(([f, n]) => [String(n).padStart(4), f]),
);

head("Keeping delivery working: commits touching");
table([
  [String(touching(/^\.github\/workflows\//)).padStart(4), ".github/workflows/"],
  [String(touching(/^scripts\//)).padStart(4), "scripts/"],
  [String(touching(/^(crates\/platform\/migrations|crates\/platform\/src\/db\/migrate)/)).padStart(4), "platform migrations"],
  [String(touching(/^docs\/decisions\//)).padStart(4), "docs/decisions/"],
]);

// Crates.
head("Crates: lines, commits, and how far the README is behind");
const crates = existsSync(join(root, "crates")) ? readdirSync(join(root, "crates")).filter((c) => statSync(join(root, "crates", c)).isDirectory()) : [];
table(
  crates
    .map((c) => {
      const dir = `crates/${c}`;
      const src = lines(git("ls-files", "--", `${dir}/*.rs`));
      const readme = `${dir}/README.md`;
      let lag = "no README";
      if (existsSync(join(root, readme))) {
        const last = git("log", "-1", "--format=%h", "--", readme);
        lag = last ? git("rev-list", "--count", `${last}..HEAD`, "--", dir) : "new";
      }
      return { c, l: src.reduce((n, f) => n + lineCount(f), 0), n: commits.filter((x) => x.files.some((f) => under(f, dir))).length, lag };
    })
    .sort((a, b) => b.l - a.l)
    .slice(0, 12)
    .map(({ c, l, n, lag }) => [String(l).padStart(7), `${String(n).padStart(4)} commits`, `README ${lag} behind`, c]),
);

// CI, from GitHub.
if (!args.includes("--no-ci")) {
  head(`CI: the last ${runs} check runs on main`);
  try {
    ci();
  } catch (e) {
    console.log(`Couldn't read GitHub: ${String(e.message).split("\n")[0]}`);
  }
}

function ci() {
  const gh = (...a) => execFileSync("gh", a, { cwd: root, encoding: "utf8", maxBuffer: 1 << 28 });
  const all = JSON.parse(gh("run", "list", "-w", "check.yml", "-b", "main", "-L", String(runs), "--json", "databaseId,conclusion,startedAt,updatedAt,createdAt")).reverse();
  const when = (r, k) => Date.parse(r[k]);
  const span = (when(all.at(-1), "createdAt") - when(all[0], "createdAt")) / 3.6e6;
  const count = {};
  for (const r of all) count[r.conclusion || "running"] = (count[r.conclusion || "running"] ?? 0) + 1;
  const cancelled = all.filter((r) => r.conclusion === "cancelled").map((r) => (when(r, "updatedAt") - when(r, "startedAt")) / 6e4).sort((a, b) => a - b);
  console.log(`Covers ${span.toFixed(0)} hours. ${Object.entries(count).map(([k, n]) => `${n} ${k}`).join(", ")}.`);
  if (cancelled.length) console.log(`Cancelled runs: median ${cancelled[cancelled.length >> 1].toFixed(0)} min, ${cancelled.reduce((a, b) => a + b, 0).toFixed(0)} min in all.`);
  // Red from a failure's start until the next success: main's hours in doubt.
  let red = 0;
  let from = null;
  for (const r of all) {
    if (r.conclusion === "failure" && from === null) from = when(r, "createdAt");
    if (r.conclusion === "success" && from !== null) {
      red += when(r, "createdAt") - from;
      from = null;
    }
  }
  if (from !== null) red += when(all.at(-1), "createdAt") - from;
  console.log(`Main red for ${(red / 3.6e6).toFixed(1)} of those hours.`);
  // What failed, against what a PR runs.
  const pr = readFileSync(join(repo, ".github/workflows/pull-request.yml"), "utf8");
  const gate = new Set([...pr.slice(pr.indexOf("\njobs:")).matchAll(/^ {2}([a-z0-9_-]+):$/gm)].map((m) => m[1]));
  const failures = all.filter((r) => r.conclusion === "failure").slice(-30);
  const jobs = new Map();
  let outside = 0;
  for (const r of failures) {
    const names = JSON.parse(gh("run", "view", String(r.databaseId), "--json", "jobs")).jobs.filter((j) => j.conclusion === "failure").map((j) => j.name.replace(/ *\(.*/, ""));
    for (const n of new Set(names)) jobs.set(n, (jobs.get(n) ?? 0) + 1);
    if (names.length && names.every((n) => !gate.has(n))) outside++;
  }
  console.log(`\nJobs failing in the newest ${failures.length} red runs (* = not run on PRs):`);
  table([...jobs].sort((a, b) => b[1] - a[1]).map(([n, c]) => [String(c).padStart(4), gate.has(n) ? n : `${n} *`]));
  console.log(`\n${outside} of ${failures.length} red runs failed only in jobs PRs don't run.`);
}
