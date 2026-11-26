#!/usr/bin/env node
// The design lift's numbers (plan §9), as JSON: how big the web client is,
// how many looks and tokens it has, how many e2e specs. `check-budget.mjs`
// holds each to `web/budget.json`. Run from anywhere:
// `node web/scripts/measure.mjs`.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const web = fileURLToPath(new URL("..", import.meta.url));
const src = join(web, "src");

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else out.push(path);
  }
  return out;
}

const lines = (text) => text.split("\n").length - (text.endsWith("\n") ? 1 : 0);
const rel = (path) => relative(src, path).split("\\").join("/");

// Generated or vendored code, kept out of the "core" count (Swarm stays in:
// the lift deletes it).
const VENDORED = [/^shared\/proto\.gen\.ts$/, /(^|\/)emoji-data\.ts$/, /^shell\/e2e\//];

export function measure() {
  const files = walk(src);
  const code = files.filter((f) => /\.tsx?$/.test(f));
  const css = files.filter((f) => f.endsWith(".css"));
  const read = new Map([...code, ...css].map((f) => [f, readFileSync(f, "utf8")]));

  let tsLines = 0;
  let tsCoreLines = 0;
  for (const f of code) {
    const n = lines(read.get(f));
    tsLines += n;
    if (!VENDORED.some((re) => re.test(rel(f)))) tsCoreLines += n;
  }

  // A component is a capitalised `function` in a .tsx file.
  const names = new Map();
  let components = 0;
  let exported = 0;
  for (const f of code.filter((f) => f.endsWith(".tsx"))) {
    for (const m of read.get(f).matchAll(/^\s*(export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Z]\w*)/gm)) {
      components++;
      if (m[1]) exported++;
      names.set(m[2], (names.get(m[2]) ?? 0) + 1);
    }
  }
  const duplicateNames = [...names.values()].filter((n) => n > 1).length;

  let cssLines = 0;
  let important = 0;
  const defined = new Set();
  for (const f of css) {
    const text = read.get(f).replace(/\/\*[\s\S]*?\*\//g, "");
    cssLines += lines(read.get(f));
    important += (text.match(/!important/g) ?? []).length;
    for (const m of text.matchAll(/(?:^|[{;\s])(--[\w-]+)\s*:/g)) defined.add(m[1]);
  }
  const referenced = new Set();
  for (const text of read.values()) for (const m of text.matchAll(/var\(\s*(--[\w-]+)/g)) referenced.add(m[1]);

  // Looks: palettes × finishes × corners × densities while they last; two
  // themes once Look is the sliders.
  const appearance = join(src, "shared/appearance.ts");
  let looks = 2;
  try {
    const text = readFileSync(appearance, "utf8");
    const count = (name) => {
      const block = text.match(new RegExp(`export const ${name}\\b[^=]*=\\s*\\[([\\s\\S]*?)\\n\\](?: as const)?;`));
      return block ? Math.max(1, (block[1].match(/\bid: "/g) ?? []).length) : 1;
    };
    looks = count("PALETTES") * count("FINISHES") * count("CORNERS") * count("DENSITIES");
    if (looks === 1) looks = 2;
  } catch {}

  const e2e = walk(join(web, "e2e")).filter((f) => f.endsWith(".spec.ts"));
  const pkg = JSON.parse(readFileSync(join(web, "package.json"), "utf8"));
  const html = readdirSync(web).filter((f) => f.endsWith(".html"));

  return {
    tsLines,
    tsCoreLines,
    components,
    exportedComponents: exported,
    duplicateComponentNames: duplicateNames,
    cssLines,
    cssFiles: css.length,
    customPropertiesDefined: defined.size,
    customPropertiesReferenced: referenced.size,
    important,
    looks,
    htmlEntries: html.length,
    e2eSpecs: e2e.length,
    e2eLines: e2e.reduce((n, f) => n + lines(readFileSync(f, "utf8")), 0),
    dependencies: Object.keys(pkg.dependencies ?? {}).length,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  console.log(JSON.stringify(measure(), null, 2));
}
