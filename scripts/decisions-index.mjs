#!/usr/bin/env node
// Writes DECISIONS.md, the index of the decision records in docs/decisions/,
// grouped by their `area` in the order the areas first appear by id. With
// --check, fails instead when DECISIONS.md isn't what it would write.
//
// The records are chant decision records (docs/decisions/decision.kind.mjs):
// Markdown files whose front matter is the JSON subset of YAML, so the
// top-level fields read here are one line each.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dir = "docs/decisions";
const out = join(root, "DECISIONS.md");
const check = process.argv.includes("--check");

const field = (front, name, file) => {
  const m = front.match(new RegExp(`^${name}: (.+)$`, "m"));
  if (!m) throw new Error(`${file}: no ${name}`);
  return JSON.parse(m[1]);
};

const records = readdirSync(join(root, dir))
  .filter((f) => /^[a-z][a-z0-9]{0,15}-[0-9]{3,}-.+\.md$/.test(f))
  .sort()
  .map((file) => {
    const text = readFileSync(join(root, dir, file), "utf8");
    const front = text.match(/^---\n([\s\S]*?)\n---\n/)?.[1];
    if (!front) throw new Error(`${file}: no front matter`);
    return {
      file,
      id: field(front, "id", file),
      title: field(front, "title", file),
      area: field(front, "area", file) ?? "Other",
      state: field(front, "state", file),
    };
  });

const areas = [];
for (const r of records) if (!areas.includes(r.area)) areas.push(r.area);

let md = `# Decisions

<!-- Written by scripts/decisions-index.mjs (\`just decisions\`) from
docs/decisions/. Edit the records, not this file. -->

The decisions that still constrain the code, one record each in
[docs/decisions/](docs/decisions/). Before you change one of these, read its
record; if you change it, change the record in the same PR, and run
\`just decisions\` to rewrite this index.

Each record is a chant decision record ([the kind](docs/decisions/decision.kind.mjs),
[its schema](docs/decisions/decision.schema.json)). Its body says what was
decided, why, where it lives in the code today ("Where:"), and where it came
from ("From:"). Its front matter holds the repository paths the body names
as \`constrains\`, the "From" links as \`evidence\`, and the section below as
\`area\`. \`chant workspace records --current\` lists them, and the workspace
block shows them. A new decision is a new record: the next id, in the area it
belongs to (\`chant workspace records new --prefix aru\` allocates the id).

Milestone and spike codes (M2, S5 …) are looked up in
[docs/plan-archive.md](docs/plan-archive.md), which is the old PLAN.md kept as
written. Anything there that a record says differently has changed since;
the records are what match the code.

Not here: what each milestone built (the archive), how to build and test
([docs/development.md](docs/development.md)), what the product does
([docs.arugula.io](https://docs.arugula.io/)).
`;

for (const area of areas) {
  md += `\n## ${area}\n\n`;
  for (const r of records.filter((r) => r.area === area)) {
    const state = r.state === "decided" ? "" : ` (${r.state})`;
    md += `- [${r.id}](${dir}/${r.file}): ${r.title}${state}\n`;
  }
}

if (check) {
  let have = "";
  try {
    have = readFileSync(out, "utf8");
  } catch {}
  if (have !== md) {
    console.error("DECISIONS.md is stale: run `just decisions`");
    process.exit(1);
  }
} else {
  writeFileSync(out, md);
}
