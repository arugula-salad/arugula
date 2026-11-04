#!/usr/bin/env node
// Writes docs/gotchas/README.md, the index of the gotchas in docs/gotchas/,
// grouped by their `area` in AREAS' order, each area's pages by title. With
// --check, fails instead when the README isn't what it would write, or when a
// page lacks its front matter, its area or one of its three parts.
//
// A page's front matter is the JSON subset of YAML, as the decision records'
// is: `title`, `description` and `area`, one line each.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dir = join(root, "docs/gotchas");
const out = join(dir, "README.md");
const check = process.argv.includes("--check");

// The order the index shows them in. A page names one of these.
const AREAS = [
  "Install and updates",
  "Web and desktop",
  "Control and sign-in",
  "Agents and MCP",
  "Forges",
  "Labs",
  "Hosted workspaces",
];
const PARTS = ["## You see", "## Why", "## Do"];

const field = (front, name, file) => {
  const m = front.match(new RegExp(`^${name}: (.+)$`, "m"));
  if (!m) throw new Error(`docs/gotchas/${file}: no ${name}`);
  return JSON.parse(m[1]);
};

const pages = readdirSync(dir)
  .filter((f) => f.endsWith(".md") && f !== "README.md")
  .map((file) => {
    const text = readFileSync(join(dir, file), "utf8");
    const front = text.match(/^---\n([\s\S]*?)\n---\n/)?.[1];
    if (!front) throw new Error(`docs/gotchas/${file}: no front matter`);
    const page = {
      file,
      title: field(front, "title", file),
      description: field(front, "description", file),
      area: field(front, "area", file),
    };
    if (!AREAS.includes(page.area)) {
      throw new Error(
        `docs/gotchas/${file}: area ${JSON.stringify(page.area)} isn't one of scripts/gotchas-index.mjs' AREAS`,
      );
    }
    let at = -1;
    for (const part of PARTS) {
      const i = text.indexOf(`\n${part}\n`);
      if (i < 0 || i < at) throw new Error(`docs/gotchas/${file}: no "${part}" after the part before it`);
      at = i;
    }
    return page;
  });

let md = `# Gotchas

<!-- Written by scripts/gotchas-index.mjs (\`just gotchas\`) from the pages'
front matter. Edit the pages, not this file. -->

Sharp edges people have hit, one page each, found by what you see. Each page
has three parts: **You see** (the symptom, as it shows on screen or in a
terminal), **Why** (the cause, so you can tell whether it's yours) and **Do**.

A new one is a page in this folder with a \`title\`, a one-line
\`description\` and an \`area\` (one of the headings below) in its front
matter, and the three parts. Run \`just gotchas\` to rewrite this index;
\`just check\` fails while it's stale.

What the daemon sends off your machine is a page of its own:
[What leaves your machine](../what-leaves-your-machine.md).
`;

for (const area of AREAS) {
  const these = pages
    .filter((p) => p.area === area)
    .sort((a, b) => a.title.localeCompare(b.title));
  if (!these.length) continue;
  md += `\n## ${area}\n\n`;
  for (const p of these) md += `- [${p.title}](${p.file}): ${p.description}\n`;
}

if (check) {
  let have = "";
  try {
    have = readFileSync(out, "utf8");
  } catch {}
  if (have !== md) {
    console.error("docs/gotchas/README.md is stale: run `just gotchas`");
    process.exit(1);
  }
} else {
  writeFileSync(out, md);
}
