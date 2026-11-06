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
// The fewest help texts the CLI's source may give before the check says
// help() lost some.
const minHelp = 400;

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

// ID prefixes, minted in crates/ outside tests: test files and
// #[cfg(test)] items are left out. A mint is `format!("run_{...}`, a
// `new_id("ob_")` (or `new_id("ses")`), or a const holding the prefix that
// a format! starts with (`format!("{ID_PREFIX}...`). A string that only
// starts with the prefix ("run_grant") isn't one.
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
const code = sources.map((p) => withoutTests(readFileSync(join(root, p), "utf8"))).join("\n");
const prefixes = [...new Set(ticks.filter((t) => /^[a-z]{1,5}_$/.test(t)))];
for (const p of prefixes) if (!minted(p)) problems.push(`\`${p}\` isn't minted anywhere in crates/: say what the ID looks like now`);
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
const helpTexts = cliHelp.flatMap(rsFiles).flatMap(help);
// Reading help from source can drop a type without a sound (a derive it
// doesn't recognise), and then the words it holds go unchecked: so there's
// a floor, well under the 549 it read when this was written (the 1,334
// texts it says it checked include the MCP tool list).
if (helpTexts.length < minHelp)
  problems.push(
    `read only ${helpTexts.length} help texts from ${cliHelp.join(" and ")}, fewer than ${minHelp}: help() in scripts/data-model-check.mjs is missing some. Fix it, or lower minHelp if the help really shrank`,
  );
texts.push(...helpTexts);

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
  let attr = null; // an attribute's lines so far, until its brackets close
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
    // An attribute, which may run over several lines: a derive of clap's
    // marks the next struct or enum.
    if (attr !== null || t.startsWith("#[")) {
      attr = attr === null ? t : `${attr} ${t}`;
      const bare = attr.replace(/"(?:[^"\\]|\\.)*"/g, '""');
      if (bare.split("[").length > bare.split("]").length) continue;
      if (/^#\[derive\([^\]]*\b(Parser|Subcommand|Args|ValueEnum)\b/.test(attr)) derive = true;
      attr = null;
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

// Whether `p` (`run_`) is minted in `code`.
function minted(p) {
  const stem = p.slice(0, -1);
  if (code.includes(`format!("${p}{`)) return true;
  if (new RegExp(`\\bnew_id\\(\\s*"${stem}_?"\\s*\\)`).test(code)) return true;
  for (const m of code.matchAll(new RegExp(`\\bconst (\\w+): &str = "${p}";`, "g")))
    if (code.includes(`format!("{${m[1]}}`)) return true;
  return false;
}

// A Rust file without its #[cfg(test)] items: a `mod tests { ... }` (or
// any item with a body) up to its closing brace, an item up to its `;`, or
// a field up to its `,`.
// Strings, chars and comments are skipped when counting braces.
function withoutTests(src) {
  let out = "";
  let from = 0;
  for (let at = src.indexOf("#[cfg(test)]"); at >= 0; at = src.indexOf("#[cfg(test)]", from)) {
    out += src.slice(from, at);
    from = itemEnd(src, at + "#[cfg(test)]".length);
  }
  return out + src.slice(from);
}

// Where the item starting at `i` ends: after its body's closing brace,
// after its `;` or `,` if it has none, or before the brace that closes
// what it's in.
function itemEnd(src, i) {
  let depth = 0;
  while (i < src.length) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") i = (src.indexOf("\n", i) + 1 || src.length) - 1;
    else if (c === "/" && src[i + 1] === "*") i = (src.indexOf("*/", i + 2) + 2 || src.length) - 1;
    else if (c === '"' || (c === "r" && /^r#*"/.test(src.slice(i, i + 8)) && !/\w/.test(src[i - 1] ?? ""))) {
      if (c === "r") {
        const hashes = src.slice(i + 1).match(/^#*/)[0];
        const close = src.indexOf(`"${hashes}`, i + hashes.length + 2);
        i = close < 0 ? src.length : close + hashes.length;
      } else {
        for (i++; i < src.length && src[i] !== '"'; i++) if (src[i] === "\\") i++;
      }
    } else if (c === "'" && /^'(?:[^'\\]|\\.[^']*)'/.test(src.slice(i, i + 12))) {
      i += src.slice(i, i + 12).match(/^'(?:[^'\\]|\\.[^']*)'/)[0].length - 1;
    } else if ("([{".includes(c)) depth++;
    else if (")]}".includes(c)) {
      depth--;
      if (depth < 0) return i;
      if (depth === 0 && c === "}") return i + 1;
    } else if ((c === ";" || c === ",") && depth === 0) return i + 1;
    i++;
  }
  return src.length;
}
