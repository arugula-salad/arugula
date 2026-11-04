// Which Playwright specs a pull request's own run takes (pull-request.yml, #718).
// A guess on purpose: the merge queue runs every spec before anything lands,
// so a spec this leaves out costs a trip through the queue, not a red main.
//
// The changed files, one a line, on stdin. Prints JSON:
//   { "e2e": false }                                  nothing the specs read
//   { "e2e": true, "all": true, "shards": [1, ...] }  every spec
//   { "e2e": true, "all": false, "only": "a.spec.ts b.spec.ts", "shards": [1], "seconds": 136 }
//
// From web/, as e2e-shard.mjs runs (it reads e2e/ and e2e/durations.json).
//
//   node scripts/e2e-select.mjs < changed-files
import { existsSync, readdirSync, readFileSync } from "node:fs";

const FULL_SHARDS = 12;
// What one shard holds: ~190 s of specs (.github/actions/e2e-shard).
const SHARD_SECONDS = 190;
// Past this share of the full run, it's cheaper to know the answer: run them all.
const ALL_ABOVE = 0.7;
// A spec file with no recorded time: about the average one.
const UNKNOWN_SECONDS = 16;

// The paths no spec reads: scripts/suite-changed's list, with what it keeps.
const IGNORED = [/^docs\/.*\.md$/, /^spikes\//, /^(DECISIONS|AGENTS|CLAUDE|CONTRIBUTING|SECURITY)\.md$/, /^\.github\/ISSUE_TEMPLATE\//, /^scripts\/macos-sign/, /^crates\/desktop\//];
const KEPT = [/^crates\/desktop\/dist\//, /^docs\/control-e2e\.md$/];
const ignored = (f) => IGNORED.some((r) => r.test(f)) && !KEPT.some((r) => r.test(f));

// What every spec runs through, or how this run picks them: run them all.
const EVERY_SPEC = [
  /^web\/e2e\/(helpers|fake-fleet|a11y)[^/]*$/,
  /^web\/e2e\/fixtures\//,
  /^web\/playwright[^/]*$/,
  /^web\/package\.json$/,
  /^web\/pnpm-lock\.yaml$/,
  /^web\/scripts\/e2e-/,
  /^crates\/e2e\//,
  /^\.github\/actions\/(e2e-shard|setup|build-debug)\//,
  /^\.github\/workflows\/pull-request\.yml$/,
  /^scripts\/pr-plan$/,
];

// The specs that walk the product end to end: every run takes them, about one shard.
const ALWAYS = ["journey-j1", "journey-j2a", "journey-j5", "journey-j6", "blocks-control", "layout", "restore", "platform", "chrome"].map((s) => `${s}.spec.ts`);

// Words in paths that say nothing about which spec cares.
const STOP = new Set(
  "api app apps arugula arugulad base client common control core crates css daemon dialog e2e github index lib main mod page pages platform rs scripts server shared shell spec src state steps style test tests ts tsx types ui utils web cases".split(" "),
);

// A file's words: its directories and its name, in lower case, a plural as its singular.
const words = (path) =>
  path
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((w) => w.length > 2 && !STOP.has(w))
    .map((w) => (w.length > 3 && w.endsWith("s") ? w.slice(0, -1) : w));

export function select(changed, specs, durations) {
  const files = changed.filter((f) => f && !ignored(f));
  if (files.length === 0) return { e2e: false };
  const everyShard = Array.from({ length: FULL_SHARDS }, (_, i) => i + 1);
  if (files.some((f) => EVERY_SPEC.some((r) => r.test(f)))) return { e2e: true, all: true, shards: everyShard };

  const chosen = new Set(ALWAYS.filter((s) => specs.includes(s)));
  const wanted = new Set(files.flatMap(words));
  for (const spec of specs) {
    if (files.includes(`web/e2e/${spec}`) || words(spec).some((w) => wanted.has(w))) chosen.add(spec);
  }
  const seconds = (s) => durations[s] ?? UNKNOWN_SECONDS;
  const total = [...chosen].reduce((a, s) => a + seconds(s), 0);
  const whole = specs.reduce((a, s) => a + seconds(s), 0);
  if (total > whole * ALL_ABOVE) return { e2e: true, all: true, shards: everyShard };
  const n = Math.max(1, Math.min(FULL_SHARDS, Math.ceil(total / SHARD_SECONDS)));
  return { e2e: true, all: false, only: [...chosen].sort().join(" "), shards: Array.from({ length: n }, (_, i) => i + 1), seconds: Math.round(total) };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const specs = readdirSync("e2e").filter((f) => f.endsWith(".spec.ts") && !f.endsWith(".webkit.spec.ts"));
  const durations = existsSync("e2e/durations.json") ? JSON.parse(readFileSync("e2e/durations.json", "utf8")) : {};
  const changed = readFileSync(0, "utf8").split("\n").map((l) => l.trim());
  console.log(JSON.stringify(select(changed, specs, durations)));
}
