#!/usr/bin/env node
// Flaky tests CI leaves out for a while (docs/testing.md, "Quarantine"),
// from one manifest, .config/quarantine.toml. Each entry is a test, the
// issue that tracks fixing it, the day it comes back (`until`, at most 14
// days out) and why:
//
//   [[rust]]
//   test = "arugulad::integration reboot::a_restarted_box_brings_..."
//   issue = "https://github.com/arugula-salad/arugula/issues/123"
//   until = "2026-11-10"
//   why = "One line."
//
//   [[e2e]]
//   spec = "web/e2e/chat-control.spec.ts"
//   title = "polish: ↑ edits the last message and ..."
//   issue = "123"
//   until = "2026-11-10"
//   why = "One line."
//
// A Rust test is named as nextest prints it: its binary id, a space, its
// name. CI's nextest profile skips the Rust ones through each workspace's
// `default-filter` (.config/nextest.toml, server/.config/nextest.toml),
// which `write` sets from the manifest; Playwright's config skips the
// specs' under CI itself (web/playwright.config.ts, `e2eGrep` here).
// `just test` and `just e2e` on a machine of your own still run them all.
//
//   node scripts/quarantine.mjs check     the manifest is well formed, no
//                                         entry is past its `until` or more
//                                         than 14 days out or without an
//                                         issue or in a serial spec, every
//                                         test exists, and the
//                                         nextest configs match it (in
//                                         `just test-scripts`, so lint);
//   node scripts/quarantine.mjs write     set the nextest configs'
//                                         default-filter from it;
//   node scripts/quarantine.mjs nextest root|server
//                                         the quarantined tests of that
//                                         workspace as a filterset, or
//                                         nothing (quarantine-nightly.yml);
//   node scripts/quarantine.mjs e2e       the quarantined specs, one a line.
//
// ARUGULA_QUARANTINE_ROOT runs it on another checkout's files and
// ARUGULA_QUARANTINE_TODAY (YYYY-MM-DD) sets today, UTC by default
// (scripts/tests/quarantine.sh).
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const MAX_DAYS = 14;
const KEYS = { rust: ["test", "issue", "until", "why"], e2e: ["spec", "title", "issue", "until", "why"] };
const CONFIGS = { root: ".config/nextest.toml", server: "server/.config/nextest.toml" };
// Playwright's testDir (web/playwright.config.ts).
const SPECS = "web/e2e/";
const SERIAL = /\.configure\(\s*\{[^}]*\bmode:\s*["']serial["']/;

function repoRoot() {
  return process.env.ARUGULA_QUARANTINE_ROOT || join(dirname(fileURLToPath(import.meta.url)), "..");
}

// The manifest: TOML, but only `[[rust]]` and `[[e2e]]` tables of
// `key = "string"` lines, comments and blank lines; anything else is an
// error, so the parser stays this small.
export function parse(text) {
  const entries = [];
  const errors = [];
  let entry = null;
  text.split("\n").forEach((raw, i) => {
    const at = `line ${i + 1}`;
    const line = raw.trim();
    if (!line || line.startsWith("#")) return;
    const table = line.match(/^\[\[(\w+)\]\]\s*(#.*)?$/);
    if (table) {
      if (!KEYS[table[1]]) errors.push(`${at}: [[${table[1]}]]: only [[rust]] and [[e2e]]`);
      entry = { kind: table[1], line: i + 1 };
      entries.push(entry);
      return;
    }
    const kv = line.match(/^(\w+)\s*=\s*"((?:[^"\\]|\\.)*)"\s*(#.*)?$/);
    if (!kv) return errors.push(`${at}: not \`key = "string"\`: ${line}`);
    if (!entry) return errors.push(`${at}: ${kv[1]} before any [[rust]] or [[e2e]]`);
    if (KEYS[entry.kind] && !KEYS[entry.kind].includes(kv[1]))
      return errors.push(`${at}: [[${entry.kind}]] has no ${kv[1]} (${KEYS[entry.kind].join(", ")})`);
    if (kv[1] in entry) return errors.push(`${at}: ${kv[1]} twice`);
    try {
      entry[kv[1]] = JSON.parse(`"${kv[2]}"`);
    } catch {
      errors.push(`${at}: ${kv[1]}: an escape JSON doesn't have`);
    }
  });
  return { entries, errors };
}

function load(root) {
  const path = join(root, ".config/quarantine.toml");
  return existsSync(path) ? parse(readFileSync(path, "utf8")) : { entries: [], errors: [] };
}

function name(e) {
  return e.kind === "rust" ? e.test : `${e.spec} › ${e.title}`;
}

// The quoted strings of `key = [ ... ]` in a Cargo.toml.
function list(toml, key) {
  const m = toml.match(new RegExp(`^${key}\\s*=\\s*\\[([^\\]]*)\\]`, "m"));
  return m ? [...m[1].replace(/#.*$/gm, "").matchAll(/"([^"]+)"/g)].map((q) => q[1]) : [];
}

function packageName(dir) {
  const toml = join(dir, "Cargo.toml");
  if (!existsSync(toml)) return null;
  const m = readFileSync(toml, "utf8").match(/^\[package\][^[]*?^name\s*=\s*"([^"]+)"/m);
  return m ? m[1] : null;
}

// Each workspace's packages, name → directory: the root's are crates/*
// but its `exclude`s, the server's server/Cargo.toml's members.
export function workspaces(root) {
  const out = { root: new Map(), server: new Map() };
  const server = new Set();
  const serverToml = join(root, "server/Cargo.toml");
  if (existsSync(serverToml))
    for (const m of list(readFileSync(serverToml, "utf8"), "members")) {
      const dir = join(root, "server", m);
      server.add(relative(root, dir));
      const n = packageName(dir);
      if (n) out.server.set(n, dir);
    }
  const excluded = new Set(existsSync(join(root, "Cargo.toml")) ? list(readFileSync(join(root, "Cargo.toml"), "utf8"), "exclude") : []);
  const crates = join(root, "crates");
  if (existsSync(crates))
    for (const d of readdirSync(crates)) {
      const rel = `crates/${d}`;
      if (excluded.has(rel) || server.has(rel)) continue;
      const n = packageName(join(crates, d));
      if (n) out.root.set(n, join(crates, d));
    }
  return out;
}

// "arugulad::integration reboot::x" → its binary id, test name, package.
function rustParts(test) {
  const m = (test ?? "").match(/^(\S+) (\S+)$/);
  if (!m) return null;
  return { binary: m[1], test: m[2], pkg: m[1].split("::")[0] };
}

// A workspace's quarantined tests as a nextest filterset, or "" for none.
export function nextestSet(root, which) {
  const ws = workspaces(root)[which];
  const parts = load(root)
    .entries.filter((e) => e.kind === "rust")
    .map((e) => rustParts(e.test))
    .filter((p) => p && ws.has(p.pkg))
    .map((p) => `(binary_id(=${p.binary}) & test(=${p.test}))`);
  return parts.join(" | ");
}

function defaultFilter(root, which) {
  const set = nextestSet(root, which);
  return `default-filter = ${JSON.stringify(set ? `not (${set})` : "all()")}`;
}

const DEFAULT_FILTER = /^default-filter = .*$/m;

function escapeRegExp(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// The quarantined specs, as Playwright `grep`s: each matches its test's
// title path ("chrome x.spec.ts [describe…] title [@tags]", the file
// relative to web/e2e, the specs' testDir) by its file and its own title.
export function e2eGrep(root = repoRoot()) {
  return load(root)
    .entries.filter((e) => e.kind === "e2e" && e.spec?.startsWith(SPECS) && e.title)
    .map((e) => new RegExp(`(?:^| )${escapeRegExp(e.spec.slice(SPECS.length))} (?:.* )?${escapeRegExp(e.title)}(?: @\\S+)*$`));
}

// A binary id's target in its package: `pkg` its library, `pkg::bin/X` a
// binary, `pkg::X` an integration test (tests/X.rs, tests/X/main.rs, or a
// target named X in its Cargo.toml).
function hasBinary(dir, binary) {
  const [, target] = binary.split("::");
  if (target === undefined) return existsSync(join(dir, "src/lib.rs"));
  const named = new RegExp(`^name\\s*=\\s*"${escapeRegExp(target.replace(/^bin\//, ""))}"`, "m").test(readFileSync(join(dir, "Cargo.toml"), "utf8"));
  const bin = target.match(/^bin\/(.+)$/);
  if (bin) return named || existsSync(join(dir, `src/bin/${bin[1]}.rs`)) || existsSync(join(dir, "src/main.rs"));
  return named || existsSync(join(dir, `tests/${target}.rs`)) || existsSync(join(dir, `tests/${target}/main.rs`));
}

function day(s) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s ?? "")) return null;
  const t = Date.parse(`${s}T00:00:00Z`);
  return Number.isNaN(t) || new Date(t).toISOString().slice(0, 10) !== s ? null : t / 86_400_000;
}

function rsFiles(dir) {
  return readdirSync(dir, { recursive: true })
    .map(String)
    .filter((f) => f.endsWith(".rs") && !f.split("/").includes("target"))
    .map((f) => join(dir, f));
}

export function check(root = repoRoot(), today = process.env.ARUGULA_QUARANTINE_TODAY) {
  const { entries, errors } = load(root);
  const problems = [...errors];
  const now = day(today ?? new Date().toISOString().slice(0, 10));
  if (now === null) return [`today isn't a date: ${today}`];
  const ws = workspaces(root);
  const seen = new Set();
  for (const e of entries) {
    if (!KEYS[e.kind]) continue;
    const at = `line ${e.line}: ${name(e)}`;
    for (const k of KEYS[e.kind]) if (!e[k]) problems.push(`${at}: no ${k}${k === "issue" ? ": name the issue that tracks the fix" : ""}`);
    if (seen.has(name(e))) problems.push(`${at}: twice`);
    seen.add(name(e));
    if (e.issue && !/^(#?\d+|https?:\/\/\S+)$/.test(e.issue)) problems.push(`${at}: issue isn't a number or a URL: ${e.issue}`);
    if (e.until) {
      const until = day(e.until);
      if (until === null) problems.push(`${at}: until isn't a date (YYYY-MM-DD): ${e.until}`);
      else if (until < now) problems.push(`${at}: expired on ${e.until}: fix the test and delete the entry, or delete the test`);
      else if (until - now > MAX_DAYS) problems.push(`${at}: until ${e.until} is more than ${MAX_DAYS} days out`);
    }
    if (e.kind === "rust" && e.test) {
      const p = rustParts(e.test);
      const dir = p && (ws.root.get(p.pkg) ?? ws.server.get(p.pkg));
      if (!p) problems.push(`${at}: test isn't "BINARY-ID TEST-NAME", as nextest prints it`);
      else if (!dir) problems.push(`${at}: no package ${p.pkg} in either workspace`);
      // nextest refuses a default-filter naming a binary that isn't there,
      // in every profile: `just test` too.
      else if (!hasBinary(dir, p.binary)) problems.push(`${at}: no test binary ${p.binary}`);
      else {
        const fn = new RegExp(`\\bfn ${escapeRegExp(p.test.split("::").pop())}\\b`);
        if (!rsFiles(dir).some((f) => fn.test(readFileSync(f, "utf8")))) problems.push(`${at}: no such test in ${relative(root, dir)}`);
      }
    }
    if (e.kind === "e2e" && e.spec && e.title) {
      const spec = join(root, e.spec);
      if (!e.spec.startsWith(SPECS) || !existsSync(spec) || !statSync(spec).isFile()) problems.push(`${at}: no spec ${e.spec} (a path from the checkout's root, in ${SPECS})`);
      else if (!readFileSync(spec, "utf8").includes(e.title)) problems.push(`${at}: no test titled that in ${e.spec}`);
      // A serial spec's tests share what the earlier ones set up: leaving one
      // out breaks the ones after it (workspace.spec.ts, #1650 → #1651).
      else if (SERIAL.test(readFileSync(spec, "utf8")))
        problems.push(`${at}: ${e.spec} runs serially (describe.configure mode "serial"): leaving one test out breaks the ones after it; fix it, or make its timing report-only (web/e2e/timing.ts)`);
    }
  }
  if (!errors.length)
    for (const [which, path] of Object.entries(CONFIGS)) {
      const full = join(root, path);
      const text = existsSync(full) ? readFileSync(full, "utf8") : "";
      const lines = text.match(new RegExp(DEFAULT_FILTER, "gm")) ?? [];
      if (lines.length !== 1) problems.push(`${path}: ${lines.length} default-filter lines, not one`);
      else if (lines[0] !== defaultFilter(root, which)) problems.push(`${path}: its default-filter isn't the manifest's: run \`just quarantine\``);
    }
  return problems;
}

function write(root) {
  const { errors } = load(root);
  if (errors.length) return errors;
  const problems = [];
  for (const [which, path] of Object.entries(CONFIGS)) {
    const full = join(root, path);
    const text = readFileSync(full, "utf8");
    if ((text.match(new RegExp(DEFAULT_FILTER, "gm")) ?? []).length !== 1) {
      problems.push(`${path}: needs one default-filter line under [profile.ci]`);
      continue;
    }
    writeFileSync(full, text.replace(DEFAULT_FILTER, defaultFilter(root, which)));
  }
  return problems;
}

function main([cmd, arg]) {
  const root = repoRoot();
  const fail = (problems) => {
    for (const p of problems) console.error(`quarantine: ${p}`);
    process.exit(problems.length ? 1 : 0);
  };
  switch (cmd) {
    case "check": {
      const problems = check(root);
      if (!problems.length) {
        const n = load(root).entries.length;
        console.log(`quarantine: ${n} test${n === 1 ? "" : "s"} quarantined, every one current`);
      }
      return fail(problems);
    }
    case "write":
      return fail(write(root));
    case "nextest":
      if (!CONFIGS[arg]) return fail(["usage: nextest root|server"]);
      if (load(root).errors.length) return fail(load(root).errors);
      return void (nextestSet(root, arg) && console.log(nextestSet(root, arg)));
    case "e2e":
      for (const e of load(root).entries.filter((e) => e.kind === "e2e")) console.log(name(e));
      return;
    default:
      return fail(["usage: quarantine.mjs check | write | nextest root|server | e2e"]);
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2));
