#!/usr/bin/env node
// Writes docs/capabilities/README.md, the index of the feature records, from
// the records beside it. With --check, fails instead when the index isn't
// what it would write or a record is malformed.
//
// The check doesn't look at whether a record's paths exist: a PR owes the
// records nothing, so moving a file never fails on one. `just map` reports
// those for the weekly agent (docs/capabilities/BRIEFING.md).
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const SECTIONS = ["What a person can do", "Where it runs", "Compatibility", "Not known"];
const LISTS = ["entry", "paths", "shared", "tests", "decisions"];

/** Every record in the checkout at root; throws on a malformed one. */
export function records(root) {
  const dir = join(root, "docs/capabilities");
  return readdirSync(dir)
    .filter((f) => /^[a-z0-9-]+\.md$/.test(f))
    .sort()
    .map((file) => {
      const text = readFileSync(join(dir, file), "utf8");
      const bad = (what) => {
        throw new Error(`docs/capabilities/${file}: ${what}`);
      };
      const front = text.match(/^---\n([\s\S]*?)\n---\n/)?.[1];
      if (!front) bad("no front matter");
      const field = (name) => {
        const m = front.match(new RegExp(`^${name}: (.+)$`, "m"));
        if (!m) bad(`no ${name}`);
        try {
          return JSON.parse(m[1]);
        } catch {
          bad(`${name} isn't JSON on one line`);
        }
      };
      const r = { file, id: field("id"), title: field("title"), verified_through: field("verified_through") };
      if (r.id !== file.slice(0, -3)) bad(`id ${r.id} isn't the file's name`);
      if (!/^[0-9a-f]{40}$/.test(r.verified_through)) bad("verified_through isn't a full commit hash");
      for (const name of LISTS) {
        r[name] = field(name);
        if (!Array.isArray(r[name])) bad(`${name} isn't a list`);
      }
      if (!r.entry.length) bad("entry is empty");
      if (!r.tests.length) bad("tests is empty: name what proves it, or say so under Not known and add the nearest");
      for (const s of SECTIONS) if (!text.includes(`\n## ${s}\n`)) bad(`no "## ${s}" section`);
      return r;
    });
}

function index(rs) {
  return `# Feature records

<!-- Written by scripts/capabilities.mjs (\`just decisions\`) from the records
beside it. Edit the records, not this file. -->

What a feature is now and where it lives, one record each. They're for
"where does this run, what does it depend on?". [BRIEFING.md](BRIEFING.md) is
the weekly job that keeps them and tells Jake what changed.

**A PR owes these nothing.** One weekly agent reads what changed since each
record's \`verified_through\` commit, then updates the record or moves that
commit forward. A record is only as current as that commit. \`just map\` lists
what's behind.

Not here: why a thing is the way it is ([DECISIONS.md](../../DECISIONS.md)),
and how to use it ([docs.arugula.io](https://docs.arugula.io/)).

${rs.map((r) => `- [${r.title}](${r.file}): checked at \`${r.verified_through.slice(0, 9)}\``).join("\n")}
`;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const out = join(root, "docs/capabilities/README.md");
  try {
    const md = index(records(root));
    if (!process.argv.includes("--check")) writeFileSync(out, md);
    else if (readFileSync(out, "utf8") !== md) throw new Error("docs/capabilities/README.md is out of date: run `just decisions`");
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
