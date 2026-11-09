#!/usr/bin/env node
// A release's notes, from what its pull requests brought (#722). Each pull
// request that people would notice adds a file to docs/notes/ (its README has
// the format), so nobody writes a release's notes at the end, and a release with
// nothing to say doesn't happen.
//
//   node scripts/release-notes.mjs FROM TO VERSION
//
// The notes FROM..TO add up to, for `scripts/release bump --notes`, on stdout.
// Exits 3, printing nothing, when no commit in the range added a note. With
// --check, it instead validates every note the range added and prints nothing.
//
// A note can name its pull request's number only through the commit that added
// it, so a note is read from the commit that added it, as TO has it.
import { execFileSync } from "node:child_process";

const SECTIONS = ["Added", "Changed", "Fixed", "Removed"];
const ID = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const MAX_HEADLINES = 5;

const git = (...args) => execFileSync("git", args, { encoding: "utf8", maxBuffer: 64 << 20 });

// One note's file: flat `key: value` lines between `---` fences, then its body.
export function parseNote(name, text) {
  const id = name.replace(/\.md$/, "");
  if (!ID.test(id)) throw new Error(`docs/notes/${name}: its name is the feature's id: lowercase words and dashes`);
  const m = text.match(/^---\n([\s\S]*?)^---\n?([\s\S]*)$/m);
  if (!m || text.indexOf("---") !== 0) throw new Error(`docs/notes/${name}: no --- block at the top (docs/notes/README.md)`);
  const fields = {};
  for (const line of m[1].split("\n")) {
    if (!line.trim()) continue;
    const kv = line.match(/^([a-z]+):\s*(.*)$/);
    if (!kv) throw new Error(`docs/notes/${name}: can't read the line "${line}"`);
    fields[kv[1]] = kv[2].trim();
  }
  const { section, title, line } = fields;
  if (!SECTIONS.includes(section)) throw new Error(`docs/notes/${name}: section is one of ${SECTIONS.join(", ")}, not ${section}`);
  if (!title) throw new Error(`docs/notes/${name}: no title`);
  if (!line) throw new Error(`docs/notes/${name}: no line`);
  if (title.length > 80) throw new Error(`docs/notes/${name}: title is over 80 characters`);
  if (line.length > 240) throw new Error(`docs/notes/${name}: line is over 240 characters`);
  let tryIt;
  if (fields.try) {
    try {
      tryIt = JSON.parse(fields.try);
    } catch {
      throw new Error(`docs/notes/${name}: try is JSON, like {"route": "/chat"}`);
    }
  }
  if (fields.headline && !["yes", "no"].includes(fields.headline)) throw new Error(`docs/notes/${name}: headline is yes or no`);
  const needs = fields.needs ? fields.needs.split(/\s*,\s*/) : undefined;
  return { id, section, title, line, try: tryIt, needs, headline: fields.headline !== "no", body: m[2].trim() };
}

// Each commit in the range, oldest first, on main's first parents.
function commits(from, to) {
  const out = git("log", "--first-parent", "--reverse", "--format=%H%x09%s", `${from}..${to}`).trim();
  return out ? out.split("\n").map((l) => l.split("\t")) : [];
}

function changed(sha) {
  return git("diff-tree", "--no-commit-id", "--name-status", "-r", "--no-renames", sha)
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((l) => l.split("\t"));
}

const number = (subject) => subject.match(/\(#(\d+)\)( \[skip ci\])?$/)?.[1];

export function collect(from, to) {
  const notes = [];
  const silent = [];
  for (const [sha, subject] of commits(from, to)) {
    const files = changed(sha);
    // What only records a release isn't anyone's change.
    if (files.every(([, f]) => /^(docs\/releases\/|crates\/proto\/fixtures\/releases\/)/.test(f))) continue;
    const pr = number(subject);
    const added = files.filter(([s, f]) => s === "A" && /^docs\/notes\/[^/]+\.md$/.test(f) && !f.endsWith("/README.md")).map(([, f]) => f);
    for (const f of added) {
      let text;
      try {
        text = git("show", `${to}:${f}`);
      } catch {
        continue; // taken out again before TO
      }
      notes.push({ ...parseNote(f.split("/").pop(), text), pr });
    }
    if (added.length === 0 && pr) silent.push(pr);
  }
  return { notes, silent };
}

// The markdown a release's tag carries: scripts/whats-new.mjs reads its block.
export function render(version, notes, silent) {
  const headlined = notes.filter((n) => n.headline);
  // Every release needs a headline: when no note wants one, the first note serves.
  const items = (headlined.length ? headlined : notes).slice(0, MAX_HEADLINES).map((n) => ({
    id: n.id,
    title: n.title,
    line: n.line,
    ...(n.try ? { try: n.try } : {}),
    ...(n.needs ? { needs: n.needs.length === 1 ? n.needs[0] : n.needs } : {}),
  }));
  const lines = [`# Arugula ${version}`, "", items.map((i) => i.line).join(" "), "", "```whats-new", JSON.stringify(items, null, 2), "```", ""];
  for (const section of SECTIONS) {
    const here = notes.filter((n) => n.section === section);
    if (here.length === 0) continue;
    lines.push(`## ${section}`, "");
    for (const n of here) {
      const pr = n.pr ? ` (#${n.pr})` : "";
      lines.push(`- **${n.title}**${pr}. ${n.body || n.line}`);
    }
    lines.push("");
  }
  if (silent.length > 0) lines.push("```no-notes", silent.map((n) => `#${n}`).join(" "), "```", "");
  return lines.join("\n");
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const args = process.argv.slice(2);
  const check = args.includes("--check");
  const [from, to, version] = args.filter((a) => a !== "--check");
  if (!from || !to || (!check && !version)) {
    console.error("usage: node scripts/release-notes.mjs [--check] FROM TO [VERSION]");
    process.exit(2);
  }
  try {
    const { notes, silent } = collect(from, to);
    if (check) process.exit(0);
    if (notes.length === 0) {
      console.error(`no pull request in ${from}..${to} added a note`);
      process.exit(3);
    }
    process.stdout.write(render(version, notes, silent));
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
