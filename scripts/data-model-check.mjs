#!/usr/bin/env node
// Checks that docs/data-model.md still matches the code (Arugula #751), the
// way scripts/decisions-index.mjs --check does for the decision records.
// `just decisions check` runs both, in CI's lint step. Fails when:
//
// - a backticked table name in the doc isn't in crates/control/tables.toml;
// - a repo path the doc names, as a link or in backticks, isn't there;
// - an ID prefix in backticks (`pdc_`) isn't minted anywhere in crates/,
//   outside tests;
// - a word in the doc's "Don't say" table is in the MCP tools' text (the
//   blessed tool list) or the CLI's help, outside the phrases the table
//   allows for it.
//
// The CLI's help is read from the source, since CI's lint step builds
// nothing: the doc comments of clap's derived types (Parser, Subcommand,
// Args, ValueEnum) and the `about` and `help` strings, in crates/cli/src
// and arugulad's crates/daemon/src/args.rs.
//
//   node scripts/data-model-check.mjs [--check]
//
// Without --check it also says what it checked.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const check = process.argv.includes("--check");
const docPath = "docs/data-model.md";
const doc = readFileSync(join(root, docPath), "utf8");
const fixture = "crates/arugulad-mcp/tests/fixtures/mcp-tool-list.json";
const cliHelp = ["crates/cli/src", "crates/daemon/src/args.rs"];

// Lowercase words the doc puts in backticks that aren't tables: commands,
// functions, settings, columns, stored values and wire names. A new one
// goes here.
const notTables = new Set([
  "arugulad",
  "chat",
  "deploy",
  "dropped",
  "gh",
  "glab",
  "group",
  "may_reach",
  "platform",
  "post_thread",
  "purpose",
  "read_thread",
  "role",
  "snapshot",
  "tea",
  "thread",
  "workspace",
  "zone",
  "zones",
]);

const problems = [];
const said = [];

// The doc's text, without its "Don't say" table, whose phrases aren't
// claims about the code.
const dontSay = doc.match(/^## Don't say\n[\s\S]*?(?=^## )/m)?.[0] ?? "";
const body = doc.replace(dontSay, "");
const ticks = [...body.matchAll(/`([^`\n]+)`/g)].map((m) => m[1]);

// Tables.
const tables = new Set(
  [...readFileSync(join(root, "crates/control/tables.toml"), "utf8").matchAll(/^name = "([^"]+)"$/gm)].map((m) => m[1]),
);
const named = [...new Set(ticks.filter((t) => /^[a-z][a-z0-9]*(_[a-z0-9]+)*$/.test(t) && !notTables.has(t)))];
for (const t of named) {
  if (!tables.has(t))
    problems.push(
      `\`${t}\` isn't a table in crates/control/tables.toml. Name the table it is now, or, if it isn't meant as one, add it to notTables in scripts/data-model-check.mjs`,
    );
}
said.push(`${named.length} tables`);

// Paths: links relative to the doc, and backticked paths from the repo's
// root (a first part that's one of the root's entries; `:12-30` dropped).
const top = new Set(readdirSync(root));
const paths = new Set();
for (const m of body.matchAll(/\]\(([^)\s]+)\)/g)) {
  const to = m[1].split("#")[0];
  if (!to || /^[a-z]+:/.test(to)) continue;
  paths.add(relative(root, join(root, dirname(docPath), to)));
}
for (const t of ticks) {
  const p = t.replace(/:\d+(-\d+)?$/, "");
  if (p.includes("/") && top.has(p.split("/")[0]) && /^[\w./-]+$/.test(p)) paths.add(p);
}
for (const p of paths) if (!existsSync(join(root, p))) problems.push(`${docPath} names ${p}, which isn't there`);
said.push(`${paths.size} paths`);

// ID prefixes, minted in crates/ outside tests.
const sources = [];
const walk = (dir) => {
  for (const e of readdirSync(join(root, dir), { withFileTypes: true })) {
    const p = `${dir}/${e.name}`;
    if (e.isDirectory()) {
      if (e.name !== "tests" && e.name !== "target" && e.name !== "node_modules") walk(p);
    } else if (e.name.endsWith(".rs") && !/(^|_)tests?\.rs$/.test(e.name)) sources.push(p);
  }
};
walk("crates");
const code = sources.map((p) => readFileSync(join(root, p), "utf8")).join("\n");
const prefixes = [...new Set(ticks.filter((t) => /^[a-z]{1,5}_$/.test(t)))];
for (const p of prefixes)
  if (!code.includes(`"${p}`)) problems.push(`\`${p}\` isn't minted anywhere in crates/: say what the ID looks like now`);
said.push(`${prefixes.length} ID prefixes`);

// Don't say.
const rows = dontSay
  .split("\n")
  .filter((l) => l.startsWith("|") && !/^\|\s*(Don't say|-)/.test(l))
  .map((l) => l.split("|").slice(1, -1).map((c) => c.trim()));
if (!rows.length) problems.push(`${docPath} has no "Don't say" table`);
const words = rows.map(([w, usedFor, say, allowed]) => ({
  usedFor,
  say,
  words: w.split(",").map((x) => x.trim()),
  re: new RegExp(`\\b(${w.split(",").map((x) => x.trim()).join("|")})\\b`, "gi"),
  allowed: [...(allowed ?? "").matchAll(/`([^`]+)`/g)].map((m) => m[1].toLowerCase()),
}));

const texts = [];
const strings = (v, where) => {
  if (typeof v === "string") texts.push({ where, text: v });
  else if (Array.isArray(v)) v.forEach((x, i) => strings(x, `${where}[${i}]`));
  else if (v && typeof v === "object") for (const [k, x] of Object.entries(v)) strings(x, `${where}.${k}`);
};
strings(JSON.parse(readFileSync(join(root, fixture), "utf8")), fixture);

const rsFiles = (p) =>
  p.endsWith(".rs")
    ? [p]
    : readdirSync(join(root, p), { recursive: true })
        .filter((f) => f.endsWith(".rs"))
        .map((f) => `${p}/${f}`);
for (const file of cliHelp.flatMap(rsFiles)) texts.push(...help(file));

const seen = new Set();
let read = 0;
for (const { where, text } of texts) {
  const flat = text.replace(/\s+/g, " ");
  if (seen.has(flat)) continue;
  seen.add(flat);
  read++;
  const lower = flat.toLowerCase();
  for (const w of words) {
    const ok = [];
    for (const a of w.allowed) for (let i = lower.indexOf(a); i >= 0; i = lower.indexOf(a, i + 1)) ok.push([i, i + a.length]);
    for (const m of flat.matchAll(w.re)) {
      const [s, e] = [m.index, m.index + m[0].length];
      if (ok.some(([a, b]) => a <= s && e <= b)) continue;
      problems.push(
        `"${m[0]}" in ${where}: ${flat.slice(Math.max(0, s - 50), e + 40).trim()}\n    For ${w.usedFor.toLowerCase()}, say ${w.say} (${docPath}, "Don't say"). A real use of the word: add its phrase to the table's allowed ones`,
      );
    }
  }
}
said.push(`${read} texts for ${words.map((w) => w.words[0]).join(", ")}`);

if (problems.length) {
  console.error(`${docPath} doesn't match the code:\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
if (!check) console.log(`${docPath}: checked ${said.join(", ")}`);

// A Rust file's help text: the doc comments of clap's derived types (the
// item's own, and those inside it), and `about`/`help` strings, each with
// the line it starts on.
function help(file) {
  const lines = readFileSync(join(root, file), "utf8").split("\n");
  const out = [];
  let docs = [];
  let derive = false;
  let depth = 0;
  let inside = null; // the depth a clap item's body opened at
  for (const [i, line] of lines.entries()) {
    const t = line.trim();
    for (const m of line.matchAll(/\b(?:about|long_about|help|long_help|after_help|after_long_help)\s*(?:=\s*|\(\s*)"((?:[^"\\]|\\.)*)"/g))
      out.push({ where: `${file}:${i + 1}`, text: m[1] });
    if (t.startsWith("///")) {
      const d = { where: `${file}:${i + 1}`, text: t.slice(3) };
      if (inside !== null) out.push(d);
      else docs.push(d);
      continue;
    }
    if (t.startsWith("#[")) {
      if (/^#\[derive\(.*\b(Parser|Subcommand|Args|ValueEnum)\b/.test(t)) derive = true;
      continue;
    }
    if (t.startsWith("//") || t === "") continue;
    if (derive && inside === null && /\b(struct|enum)\b/.test(t)) {
      out.push(...docs);
      inside = depth;
      derive = false;
    }
    docs = [];
    const bare = t
      .replace(/"(?:[^"\\]|\\.)*"/g, '""')
      .replace(/'(?:[^'\\]|\\.)'/g, "''")
      .replace(/\/\/.*$/, "");
    for (const c of bare) {
      if (c === "{") depth++;
      if (c === "}") {
        depth--;
        if (inside !== null && depth <= inside) inside = null;
      }
    }
    if (inside !== null && depth === inside && /;\s*$/.test(bare)) inside = null;
  }
  // Join each run of lines into one text, so a phrase may span lines.
  const joined = [];
  for (const d of out) {
    const last = joined.at(-1);
    const n = Number(d.where.split(":").at(-1));
    if (last && last.end === n - 1) {
      last.text += ` ${d.text}`;
      last.end = n;
    } else joined.push({ ...d, end: n });
  }
  return joined.map(({ where, text }) => ({ where, text }));
}
